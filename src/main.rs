mod config;
mod logging;
mod router;
use hyper::{
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Server,
};
use std::{convert::Infallible, net::SocketAddr, process::exit, sync::Arc};

#[tokio::main]
async fn main() {
    logging::init_logging();
    let cfg = match config::from_file("./sample_config.toml") {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("{}", e);
            exit(1);
        }
    };
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let make_svc = make_service_fn(move |conn: &AddrStream| {
        let cfg = cfg.clone();
        let log = logging::log_context(&conn.remote_addr());
        async {
            Ok::<_, Infallible>(service_fn(move |req| {
                let cfg = cfg.clone();
                let mut log = log.clone();
                async move {
                    log.start().req(&req);
                    let res = router::handle(req, &cfg).await;
                    log.res(&res).end();
                    logging::info!("{}", log.clf_with_timing());
                    res
                }
            }))
        }
    });
    let server = Server::bind(&addr).serve(make_svc);
    logging::info!("listening on port {}", addr.port());
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
        exit(1);
    }
}
