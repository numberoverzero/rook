use std::str;
use std::convert::Infallible;
use hyper::header::HeaderValue;
use log::info;
use sha2::Sha256;
use hmac::{Hmac, Mac, NewMac};
use hmac::crypto_mac::Output;
use hyper::{HeaderMap, body};
use hyper::{Body, Request, Response, StatusCode};

type HmacSha256 = Hmac<Sha256>;
type HttpResult = Result<Response<Body>, Infallible>;

enum ValidationError {
    HeaderMissing,
    HeaderMalformed,
}

macro_rules! unwrap_or_fail {
    ( $e:expr ) => {
        match $e {
            Ok(x) => x,
            Err(_) => return resp(StatusCode::BAD_REQUEST)
        }
    };
}

pub async fn route_hook(mut req: Request<Body>) -> HttpResult {
    // TODO load from config
    let secret = "14c319df59e71d6c6e6feda42f6c884ed7d6d4e786780077f758852f5e3a2a5566d86d0ada29ccf8";
    // TODO check uri and parse body for repo, branch, commit id
    // TODO look up hook config for repo, then join repo/branch/commit into args and run command

    // TODO clean up, only diagnostic
    info!("starting req");
    
    let payload = unwrap_or_fail!(body::to_bytes(req.body_mut()).await);
    let signature = unwrap_or_fail!(get_hmac_value(req.headers()));
    let matches = check_hmac(&secret, &payload, &signature);
    
    // TODO clean up, only diagnostic {START}
    let payload_str = unwrap_or_fail!(str::from_utf8(payload.as_ref()));
    let hmac = calc_hmac(&secret, &payload);
    info!("path: {}", req.uri());
    info!("body: {}", payload_str);
    info!(" sig: {:02x?}", signature);
    info!(" mac: {:02x?}", &hmac.into_bytes());
    info!("  eq: {}", matches);
    // TODO clean up, only diagnostic {END}
    
    resp(StatusCode::OK)
}

fn get_hmac_value(headers: &HeaderMap<HeaderValue>) -> Result<Vec<u8>, ValidationError> {
    const GH_PREFIX: &str = "sha256=";
    let header = match headers.get("x-hub-signature-256") {
        Some(h) => match h.to_str() {
            Ok(s) => s,
            _ => return Err(ValidationError::HeaderMalformed)
        },
        None => return Err(ValidationError::HeaderMissing)
    };
    if !header.starts_with(GH_PREFIX) {
        return Err(ValidationError::HeaderMalformed)
    }
    if (header.len()-GH_PREFIX.len()) % 2 != 0 {
        return Err(ValidationError::HeaderMalformed)
    }
    (GH_PREFIX.len()..header.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&header[i..i+2], 16).map_err(|_| ValidationError::HeaderMalformed))
        .collect()
}

fn check_hmac(secret: &str, body: &[u8], signature: &Vec<u8>) -> bool {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    mac.verify(signature).is_ok()
}

// TODO clean up, only diagnostic
fn calc_hmac(secret: &str, body: &[u8]) -> Output<HmacSha256> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    mac.finalize()
}

fn resp(code: StatusCode) -> HttpResult {
    Ok(Response::builder().status(code).body("yeah sure".into()).unwrap())
}
