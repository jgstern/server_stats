use crate::{
    appservice::generate_appservice, config::Config, database::cache::CacheDb,
    webpage::api::SSEJson,
};
use futures::{SinkExt, StreamExt};
use opentelemetry_prometheus::PrometheusExporter;
use prometheus::{
    core::{AtomicF64, GenericGauge},
    opts, register_gauge, Encoder, Registry, TextEncoder,
};
use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tokio::sync::watch::Receiver;
use tracing::{error, info};
use warp::{filters::BoxedFilter, http::StatusCode, ws::Message, Filter, Rejection, Reply};

pub mod api;

pub async fn run_server(
    config: &Config,
    cache: CacheDb,
    rx: Receiver<Option<SSEJson>>,
    exporter: PrometheusExporter,
) {
    info!("Starting appservice...");
    let appservice = generate_appservice(&config, cache.clone()).await;
    let addr = IpAddr::from_str(config.api.ip.as_ref());
    let path = format!("{}index.html", config.api.webpage_path);
    info!("Path is: {} and {}", config.api.webpage_path, path);
    let routes = warp::any()
        .and(appservice.warp_filter())
        .or(warp::get()
            .and(prometheus_route(exporter))
            .or(websocket(rx.clone()))
            .or(warp::path("health")
                .and(warp::path::end())
                .map(|| "Hello World"))
            .or(warp::path("relations").and_then(move || {
                let cache = cache.clone();
                async move { relations(cache.clone()).await }
            }))
            .or(warp::fs::dir(config.api.webpage_path.to_string()))
            .or(warp::path("spaces")
                .and(warp::path::end())
                .and(warp::fs::file(path.clone())))
            .or(warp::path("links")
                .and(warp::path::end())
                .and(warp::fs::file(path.clone())))
            .or(warp::path("3d")
                .and(warp::path::end())
                .and(warp::fs::file(path.clone())))
            .or(warp::path("faq")
                .and(warp::path::end())
                .and(warp::fs::file(path.clone())))
            .or(warp::path::end().and(warp::get()).and(warp::fs::file(path))))
        .recover(handle_rejection)
        .with(warp::trace::request());
    if let Ok(addr) = addr {
        let socket_addr = SocketAddr::new(addr, config.api.port);
        warp::serve(routes).run(socket_addr).await;
    } else {
        error!("Unable to start webserver: Invalid IP Address")
    }
}
async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    // We should have expected this... Just log and say its a 500
    error!("unhandled rejection: {:?}", err);
    let code = StatusCode::INTERNAL_SERVER_ERROR;
    let message = "UNHANDLED_REJECTION";
    Ok(warp::reply::with_status(message, code))
}
pub fn init_prometheus() -> (PrometheusExporter, GenericGauge<AtomicF64>) {
    let registry = Registry::new();
    let opts = opts!("rooms_total", "Rooms statistics").namespace("server_stats");
    let gauge = register_gauge!(opts).unwrap();
    registry
        .register(Box::new(gauge.clone()))
        .expect("Creating a prometheus registry");
    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(registry)
        .init();
    (exporter, gauge)
}

fn prometheus_route(exporter: PrometheusExporter) -> BoxedFilter<(impl Reply,)> {
    warp::path("metrics")
        .and(warp::path::end())
        .map(move || {
            // Encode data as text or protobuf
            let encoder = TextEncoder::new();
            let metric_families = exporter.registry().gather();
            let mut result = Vec::new();
            if let Err(e) = encoder.encode(&metric_families, &mut result) {
                error!("Failed to encode prometheus data: {:?}", e);
            }

            result
        })
        .boxed()
}

fn websocket(broadcast_rx: Receiver<Option<SSEJson>>) -> BoxedFilter<(impl Reply,)> {
    warp::path("ws")
        .and(warp::path::end())
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let broadcast_rx = broadcast_rx.clone();
            // And then our closure will be called when it completes...
            ws.on_upgrade(|websocket| do_websocket(websocket, broadcast_rx))
        })
        .boxed()
}

async fn do_websocket(websocket: warp::ws::WebSocket, mut broadcast_rx: Receiver<Option<SSEJson>>) {
    // Just echo all messages back...
    let (mut tx, _rx) = websocket.split();
    while broadcast_rx.changed().await.is_ok() {
        let json = (*broadcast_rx.borrow()).clone();
        if let Some(json) = json {
            let j = serde_json::to_string(&json).unwrap();
            if let Err(e) = tx.send(Message::text(j.clone())).await {
                error!("Failed to send via websocket: {:?}", e);
                return;
            }
        }
    }
}

async fn relations(cache: CacheDb) -> Result<impl Reply, Infallible> {
    let relations = cache.graph.get_json_relations().await;
    Ok(warp::reply::json(&relations))
}
