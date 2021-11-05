use hyper::{Body, Method, Request, Response, StatusCode, Version};
pub use log::info;
use log::{Level, LevelFilter, Metadata, Record};
use std::{convert::Infallible, net::SocketAddr, process::exit};
use time::{format_description::FormatItem, macros::format_description, OffsetDateTime};

// time crate does not support strftime
// original: "%d/%b/%Y:%H:%M:%S %z"
// see also:
//   https://github.com/time-rs/time/issues/341
//   https://time-rs.github.io/format-converter/
//   https://time-rs.github.io/book/api/format-description.html
const CLF_TIME_FORMAT: &[FormatItem] = format_description!("[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour sign:mandatory][offset_minute]");

#[derive(Clone)]
pub struct LoggingCtx {
    addr: SocketAddr,
    req_method: Option<Method>,
    req_path: Option<String>,
    req_version: Option<Version>,
    resp_status: Option<StatusCode>,
    resp_size: Option<u32>,
    timing_start: Option<OffsetDateTime>,
    timing_end: Option<OffsetDateTime>,
}

pub fn init_logging() {
    log::set_logger(&LOGGER)
        .map(|_| log::set_max_level(LevelFilter::Info))
        .unwrap_or_else(|_| {
            eprintln!("failed to init logging");
            exit(1)
        })
}

pub fn log_context(remote: &SocketAddr) -> LoggingCtx {
    LoggingCtx {
        addr: remote.clone(),
        req_method: None,
        req_path: None,
        req_version: None,
        resp_status: None,
        resp_size: None,
        timing_start: None,
        timing_end: None,
    }
}

impl LoggingCtx {
    pub fn start(&mut self) -> &mut Self {
        self.timing_start = Some(OffsetDateTime::now_utc());
        self
    }
    pub fn end(&mut self) -> &mut Self {
        self.timing_end = Some(OffsetDateTime::now_utc());
        self
    }
    pub fn req(&mut self, req: &Request<Body>) -> &mut Self {
        self.req_method = Some(req.method().clone());
        self.req_path = Some(req.uri().path().to_string());
        self.req_version = Some(req.version());
        self
    }
    pub fn res(&mut self, res: &Result<Response<Body>, Infallible>) -> &mut Self {
        self.resp_status = match res.as_ref() {
            Ok(r) => Some(r.status()),
            Err(_) => None,
        };
        // response bytes are opaque here, would need to use body::to_bytes()
        self.resp_size = None;
        self
    }

    fn internal_clf_with_timing(&self) -> Result<String, &'static str> {
        let start = self.timing_start.ok_or("start timing not set")?;
        let end = self.timing_end.ok_or("end timing not set")?;
        let method = self.req_method.as_ref().ok_or("method not set")?;
        let path = self.req_path.as_ref().ok_or("path not set")?;
        let version = self.req_version.ok_or("version not set")?;
        let status = match self.resp_status {
            Some(s) => s.to_string(),
            None => "-".to_string(),
        };
        let elapsed = (end - start).whole_microseconds();

        Ok(format!(
            r#"{} - - [{}] "{} {} {:?}" {} - {}µs"#,
            self.addr,
            end.format(CLF_TIME_FORMAT)
                .map_err(|_| "bad time fmt str")?,
            method,
            path,
            version,
            status,
            elapsed
        ))
    }

    /// Render the request context in [CLF](https://en.wikipedia.org/wiki/Common_Log_Format)
    /// with an extra field for timing information.
    pub fn clf_with_timing(&self) -> String {
        self.internal_clf_with_timing()
            .expect("error formatting log line")
    }
}

struct SimpleLogger;
static LOGGER: SimpleLogger = SimpleLogger;

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("{}", record.args());
        }
    }
    fn flush(&self) {}
}
