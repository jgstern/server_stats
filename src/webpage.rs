use crate::{
    appservice::generate_appservice, config::Config, database::cache::CacheDb,
    webpage::api::SSEJson,
};
use futures::{SinkExt, StreamExt};
use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    str::FromStr,
};
use tokio::sync::watch::Receiver;
use tracing::{error, info};
use warp::{filters::BoxedFilter, ws::Message, Filter, Reply};

pub mod api;

pub async fn run_server(config: &Config, cache: CacheDb, rx: Receiver<Option<SSEJson>>) {
    info!("Starting appservice...");
    let appservice = generate_appservice(&config, cache.clone()).await;
    let addr = IpAddr::from_str(config.api.ip.as_ref());
    let routes = warp::any()
        .and(webpage(&config))
        .or(websocket(rx.clone()))
        .or(warp::path("relations").and_then(move || {
            let cache = cache.clone();
            async move { relations(cache.clone()).await }
        }))
        .or(appservice.warp_filter());
    if let Ok(addr) = addr {
        let socket_addr = SocketAddr::new(addr, config.api.port);
        warp::serve(routes).run(socket_addr).await;
    } else {
        error!("Unable to start webserver: Invalid IP Address")
    }
}

fn websocket(broadcast_rx: Receiver<Option<SSEJson>>) -> BoxedFilter<(impl Reply,)> {
    warp::path("ws")
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let mut broadcast_rx = broadcast_rx.clone();
            // And then our closure will be called when it completes...
            ws.on_upgrade(|websocket| {
                // Just echo all messages back...
                let (mut tx, _rx) = websocket.split();
                async move {
                    while broadcast_rx.changed().await.is_ok() {
                        let json = (*broadcast_rx.borrow()).clone();
                        if let Some(json) = json {
                            let j = serde_json::to_string(&json).unwrap();
                            if let Err(e) = tx.send(Message::text(j.clone())).await {
                                error!("Failed to send via websocket: {:?}", e);
                            }
                        }
                    }
                }
            })
        })
        .boxed()
}

fn webpage(config: &Config) -> BoxedFilter<(impl Reply,)> {
    warp::path::end()
        .and(warp::fs::dir(config.api.webpage_path.to_string()))
        .boxed()
}

async fn relations(cache: CacheDb) -> Result<impl Reply, Infallible> {
    let relations = cache.graph.get_json_relations().await;
    Ok(warp::reply::json(&relations))
}
