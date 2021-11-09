mod config;
mod logging;
mod router;
use hyper::{
    server::conn::AddrStream,
    service::{make_service_fn, service_fn},
    Server,
};
use std::{convert::Infallible, env, net::SocketAddr, process, sync::Arc};

#[tokio::main]
async fn main() {
    logging::init_logging();
    let cfg_path = env::args().nth(1).unwrap_or_else(|| {
        eprintln!(
            "usage: {} your_config_file.toml",
            env::args().nth(0).unwrap()
        );
        process::exit(1);
    });
    let cfg = match config::from_file(&cfg_path) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("{}", e);
            process::exit(1);
        }
    };
    let svc_cfg = cfg.clone();
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let make_svc = make_service_fn(move |conn: &AddrStream| {
        let conn_cfg = svc_cfg.clone();
        let log = logging::log_context(&conn.remote_addr());
        async {
            Ok::<_, Infallible>(service_fn(move |req| {
                let req_cfg = conn_cfg.clone();
                let mut log = log.clone();
                async move {
                    log.start().req(&req);
                    let res = router::handle(req, &req_cfg).await;
                    log.res(&res).end();
                    logging::info!("{}", log.clf_with_timing());
                    res
                }
            }))
        }
    });
    let server = Server::bind(&addr).serve(make_svc);
    logging::info!("listening on port {}", cfg.port);
    match server.await {
        Ok(_) => {
            println!("shutting down");
        }
        Err(e) => {
            eprintln!("server error: {}", e);
            process::exit(1);
        }
    }
}
