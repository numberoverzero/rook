use crate::config::{GithubHook, RookHook, RouteConfig};
use fork::Fork;
use futures::stream::TryStreamExt;
use hmac::{Hmac, Mac};
use hyper::{
    header::{HeaderMap, HeaderValue},
    Body, Request, Response, StatusCode,
};
use serde::Deserialize;
use serde_json;
use sha2::Sha256;
use std::{
    convert::Infallible,
    fmt,
    process::{self, Command, Stdio},
    str::{self, FromStr},
};

type Headers = HeaderMap<HeaderValue>;

macro_rules! debug {
    ($($tts:tt)*) => {
        #[cfg(debug_assertions)]
        log::debug!($($tts)*)
    }
}

pub async fn handle(req: Request<Body>, cfg: &RouteConfig) -> Result<Response<Body>, Infallible> {
    Ok::<_, Infallible>(match route(req, cfg).await {
        Ok(o) => o,
        Err(e) => e,
    })
}

async fn route(req: Request<Body>, cfg: &RouteConfig) -> Result<Response<Body>, Response<Body>> {
    const OK_EMPTY: HttpResponse = HttpResponse::Ok("");

    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let headers = &parts.headers;

    debug!("incoming request");
    debug!("<<<{} {}", parts.method, path);
    #[cfg(debug_assertions)]
    for (k, v) in headers {
        log::debug!("<<<{}: {:?}", k, v);
    }

    guard_content_length(headers)?;
    let body = &parse_body(body).await?;
    let resp = if let Some(hooks) = cfg.gh_hooks.get(&path) {
        debug!("dispatch '{}' as github", path);
        exec_gh_hooks(hooks, headers, body).await
    } else if let Some(hooks) = cfg.rook_hooks.get(&path) {
        debug!("dispatch '{}' as rook", path);
        exec_rook_hooks(hooks, headers, body).await
    } else {
        debug!("no route for '{}'", path);
        Err(BAD_ROUTE)
    };
    // using Result<T,E> for early exit control flow, flatten both branches
    match resp {
        Ok(_) => {
            debug!("path dispatched successfully");
            Ok(OK_EMPTY.into())
        }
        Err(e) => {
            debug!("path dispatch failed: {:?}", e);
            Err(e.into())
        }
    }
}

fn get_header<T: FromStr>(headers: &Headers, key: &str) -> Result<T, HttpResponse> {
    const HEADER_MISSING: HttpResponse = HttpResponse::BadRequest("missing header");

    headers
        .get(key)
        .ok_or_else(|| HEADER_MISSING)?
        .to_str()
        .map_err(|_| HEADER_MALFORMED)?
        .parse()
        .map_err(|_| HEADER_MALFORMED)
}

fn guard_content_length(headers: &Headers) -> Result<(), HttpResponse> {
    const MAX_BODY_LENGTH: u32 = 1 << 21; // 2 MiB is enough for anyone
    const BODY_TOO_LARGE: HttpResponse = HttpResponse::BadRequest("body too large");

    let len: u32 = get_header(headers, "content-length")?;
    if len > MAX_BODY_LENGTH {
        return Err(BODY_TOO_LARGE);
    }
    Ok(())
}

async fn parse_body(body: Body) -> Result<Vec<u8>, HttpResponse> {
    const BODY_READ_FAILED: HttpResponse = HttpResponse::BadRequest("body read error");

    // avoid a mutable ref to the req object.  compare to:
    //   let bytes = body::to_bytes(req.body_mut()).await?.to_vec();
    body.try_fold(Vec::new(), |mut data, chunk| async move {
        data.extend_from_slice(&chunk);
        Ok(data)
    })
    .await
    .map_err(|_| BODY_READ_FAILED)
}

async fn exec_gh_hooks(
    hooks: &[GithubHook],
    headers: &Headers,
    body: &[u8],
) -> Result<(), HttpResponse> {
    const GH_DIGEST_HEADER: &'static str = "x-hub-signature-256";
    struct State {
        m: usize, // matching hooks
        v: usize, // verified hmac
        s: usize, // started cmd
    }

    let payload: GithubPayload = serde_json::from_slice(body).map_err(|_| BODY_MALFORMED)?;
    debug!(
        "github payload: ({}, {}, {})",
        payload.repo.full_name,
        payload.commit,
        payload.reference
    );
    let hmac_claim = extract_hmac(headers, GH_DIGEST_HEADER, DIGEST_PREFIX)?;
    let mut state = State { m: 0, v: 0, s: 0 };
    for hook in hooks.iter().filter(|h| h.repo == payload.repo.full_name) {
        debug!("matched repo {}", hook.repo);
        state.m += 1;

        if let Ok(_) = check_hmac(&hook.secret, body, &hmac_claim) {
            state.v += 1;
        } else {
            continue;
        }

        if run_forked(|| {
            Command::new(&hook.command)
                .stdin(Stdio::null())
                .stdout(
                    if cfg!(debug_assertions) {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                })
                .stderr(
                    if cfg!(debug_assertions) {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                })
                // https://security.stackexchange.com/a/14009
                .env("GITHUB_REPO", &payload.repo.full_name)
                .env("GITHUB_COMMIT", &payload.commit)
                .env("GITHUB_REF", &payload.reference)
                .spawn()
        }) {
            state.s += 1;
        }
    }
    match state {
        // no hooks listening for this event's repo
        State { m: 0, v: _v, s: _s } => Err(BAD_ROUTE),
        // some listening but every signature check failed
        State { m: _m, v: 0, s: _s } => Err(SIGNATURE_MISMATCH),
        // some signature checks passed but we failed to start any processes
        State { m: _m, v: _v, s: 0 } => Err(SERVER_ERR),
        // some processes started
        _ => Ok(()),
    }
}

async fn exec_rook_hooks(
    hooks: &[RookHook],
    headers: &Headers,
    body: &[u8],
) -> Result<(), HttpResponse> {
    const ROOK_DIGEST_HEADER: &'static str = "x-rook-signature-256";
    struct State {
        v: usize, // verified hmac
        s: usize, // started cmd
    }

    let body_string = str::from_utf8(body).map_err(|_| BODY_MALFORMED)?.trim();
    debug!("rook payload ({}b): {:?}", body_string.len(), body_string);
    let hmac_claim = extract_hmac(headers, ROOK_DIGEST_HEADER, DIGEST_PREFIX)?;
    let mut state = State { v: 0, s: 0 };
    for hook in hooks {
        if let Ok(_) = check_hmac(&hook.secret, body, &hmac_claim) {
            state.v += 1;
        } else {
            continue;
        }

        if run_forked(|| {
            Command::new(&hook.command)
                .stdin(Stdio::null())
                .stdout(
                    if cfg!(debug_assertions) {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                })
                .stderr(
                    if cfg!(debug_assertions) {
                        Stdio::inherit()
                    } else {
                        Stdio::null()
                })
                .env("ROOK_INPUT", body_string)
                .spawn()
        }) {
            state.s += 1;
        }
    }
    match state {
        // every signature check failed
        State { v: 0, s: _s } => Err(SIGNATURE_MISMATCH),
        // some signature checks passed but we failed to start any processes
        State { v: _v, s: 0 } => Err(SERVER_ERR),
        // some processes started
        _ => Ok(()),
    }
}

fn extract_hmac(
    headers: &Headers,
    name: &'static str,
    prefix: &'static str,
) -> Result<Vec<u8>, HttpResponse> {
    let header: String = get_header(headers, name)?;
    if !header.starts_with(prefix) {
        return Err(HEADER_MALFORMED);
    }
    if (header.len() - prefix.len()) % 2 != 0 {
        return Err(HEADER_MALFORMED);
    }
    (prefix.len()..header.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&header[i..i + 2], 16).map_err(|_| HEADER_MALFORMED))
        .collect()
}

fn check_hmac(secret: &[u8], body: &[u8], signature: &[u8]) -> Result<(), HttpResponse> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("error initializing hmac");
    mac.update(body);
    match mac.verify_slice(signature) {
        Ok(_) => {
            debug!("hmac check success");
            Ok(())
        }
        Err(_) => {
            debug!("hmac check failed");
            Err(SIGNATURE_MISMATCH)
        }
    }
}

/// be **very** careful that the forked function does not panic.
///
/// no logging on any failure, just one shot to run in a forked process
/// process is detached with setsid after fork
fn run_forked<F, T>(f: F) -> bool
where
    F: Fn() -> std::io::Result<T>,
{
    match fork::fork() {
        Ok(Fork::Parent(_)) => {
            // we're in the parent process
            debug!("hook forked");
            return true;
        }
        Ok(Fork::Child) => {
            // we're in the child process
            if fork::setsid().is_err() {
                // if we can't change our session id, don't try to start.
                process::exit(0)
            }
            // discard the result since we're always exiting the child thread
            let _unused: Result<T, _> = f();
            process::exit(0);
        }
        Err(_) => {
            // failed to fork
            debug!("failed to fork");
            return false;
        }
    }
}

const DIGEST_PREFIX: &'static str = "sha256=";
const SERVER_ERR: HttpResponse = HttpResponse::ServerError;
const BAD_ROUTE: HttpResponse = HttpResponse::BadRequest("bad route");
const SIGNATURE_MISMATCH: HttpResponse = HttpResponse::BadRequest("signature mismatch");
const HEADER_MALFORMED: HttpResponse = HttpResponse::BadRequest("malformed header");
const BODY_MALFORMED: HttpResponse = HttpResponse::BadRequest("malformed body");

impl From<HttpResponse> for Response<Body> {
    fn from(error: HttpResponse) -> Self {
        let (status, body) = match error {
            HttpResponse::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            HttpResponse::ServerError => (StatusCode::INTERNAL_SERVER_ERROR, ""),
            HttpResponse::Ok(msg) => (StatusCode::OK, msg),
        };
        Response::builder()
            .status(status)
            .body(body.into())
            .expect("error building body")
    }
}

#[derive(Clone)]
enum HttpResponse {
    BadRequest(&'static str),
    ServerError,
    Ok(&'static str),
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            HttpResponse::BadRequest(msg) => msg,
            HttpResponse::ServerError => "internal error",
            HttpResponse::Ok(_) => "ok",
        };
        write!(f, "HttpResponse<{}>", msg)
    }
}

#[derive(Deserialize)]
struct GithubPayload {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(rename = "after")]
    commit: String,
    #[serde(rename = "repository")]
    repo: GithubRepo,
}

#[derive(Deserialize)]
struct GithubRepo {
    full_name: String,
}
