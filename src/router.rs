use crate::config::{GithubHook, RookHook, RouteConfig};
use futures::stream::TryStreamExt;
use hmac::{Hmac, Mac, NewMac};
use hyper::{
    header::{HeaderMap, HeaderValue},
    Body, Request, Response, StatusCode,
};
use serde::Deserialize;
use serde_json;
use sha2::Sha256;
use shlex;
use std::{
    convert::Infallible,
    process::{Command, Stdio},
    str::{self, FromStr},
};

type Headers = HeaderMap<HeaderValue>;

pub async fn handle(req: Request<Body>, cfg: &RouteConfig) -> Result<Response<Body>, Infallible> {
    Ok::<_, Infallible>(match route(req, cfg).await {
        Ok(o) => o,
        Err(e) => e,
    })
}

async fn route(req: Request<Body>, cfg: &RouteConfig) -> Result<Response<Body>, Response<Body>> {
    const OK_EMPTY: HttpResponse = HttpResponse::Ok("");
    const BAD_ROUTE: HttpResponse = HttpResponse::BadRequest("bad route");

    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let headers = &parts.headers;

    guard_content_length(headers)?;
    let body = &parse_body(body).await?;

    let resp = if let Some(hooks) = cfg.gh_hooks.get(&path) {
        exec_gh_hooks(hooks, headers, body).await
    } else if let Some(hooks) = cfg.rook_hooks.get(&path) {
        exec_rook_hooks(hooks, headers, body).await
    } else {
        Err(BAD_ROUTE)
    };
    // using Result<T,E> for early exit control flow, flatten both branches
    resp.map(|_| OK_EMPTY.into()).map_err(|e| e.into())
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
    hooks: &Vec<GithubHook>,
    headers: &Headers,
    body: &Vec<u8>,
) -> Result<(), HttpResponse> {
    const GH_DIGEST_HEADER: &'static str = "x-hub-signature-256";

    let payload: GithubPayload = serde_json::from_slice(body).map_err(|_| BODY_MALFORMED)?;
    let hmac_claim = extract_hmac(headers, GH_DIGEST_HEADER, DIGEST_PREFIX)?;
    for hook in hooks.iter().filter(|h| h.repo == payload.repo.full_name) {
        check_hmac(&hook.secret, body, &hmac_claim)?;
        Command::new(&hook.command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .arg(&payload.repo.full_name)
            .arg(&payload.commit)
            .arg(&payload.reference)
            .spawn()
            .map_err(|_| SERVER_ERR)?;
    }
    Ok(())
}

async fn exec_rook_hooks(
    hooks: &Vec<RookHook>,
    headers: &Headers,
    body: &Vec<u8>,
) -> Result<(), HttpResponse> {
    const ROOK_DIGEST_HEADER: &'static str = "x-rook-signature-256";

    let args = shlex::split(str::from_utf8(body).map_err(|_| BODY_MALFORMED)?.trim())
        .ok_or_else(|| BODY_MALFORMED)?;
    let hmac_claim = extract_hmac(headers, ROOK_DIGEST_HEADER, DIGEST_PREFIX)?;
    for hook in hooks {
        check_hmac(&hook.secret, body, &hmac_claim)?;
        Command::new(&hook.command)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args(&args)
            .spawn()
            .map_err(|_| SERVER_ERR)?;
    }
    Ok(())
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

fn check_hmac(secret: &Vec<u8>, body: &[u8], signature: &Vec<u8>) -> Result<(), HttpResponse> {
    const SIGNATURE_MISMATCH: HttpResponse = HttpResponse::BadRequest("signature mismatch");

    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("error initializing hmac");
    mac.update(body);
    match mac.verify(signature) {
        Ok(_) => Ok(()),
        Err(_) => Err(SIGNATURE_MISMATCH),
    }
}

const DIGEST_PREFIX: &'static str = "sha256=";
const SERVER_ERR: HttpResponse = HttpResponse::ServerError;
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

enum HttpResponse {
    BadRequest(&'static str),
    ServerError,
    Ok(&'static str),
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
