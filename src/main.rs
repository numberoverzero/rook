use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use log::{error, Record, Level, Metadata, LevelFilter};
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};

struct SimpleLogger;
static LOGGER: SimpleLogger = SimpleLogger;

#[tokio::main]
async fn main() {
    init_logging();
    let addr = SocketAddr::from(([0, 0, 0, 0], 9000));
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(rook::route_hook))
    });
    let server = Server::bind(&addr).serve(make_svc);
    if let Err(e) = server.await {
        error!("server error: {}", e);
    }
}

impl log::Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("{}:rook:{}:{}",
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                record.level(),
                record.args());
        }
    }
    fn flush(&self) {}
}

fn init_logging() {
    log::set_logger(&LOGGER)
        .map(|()| log::set_max_level(LevelFilter::Info))
        .unwrap()
}
