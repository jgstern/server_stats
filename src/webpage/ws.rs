use crate::{database::graph::SSEJson, WEBSOCKET};
use actix::prelude::*;
use actix::{Actor, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use std::{collections::HashSet, time::Duration};
use tracing::info;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Define HTTP actor
pub struct Ws {
    already_send_data: HashSet<SSEJson>,
}

impl Ws {
    pub fn send_data(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |a, ctx| {
            let data = WEBSOCKET.read().unwrap();
            let iterator: HashSet<&SSEJson> = data
                .iter()
                .filter(|x| !a.already_send_data.contains(x))
                .collect();
            let mut iterated = HashSet::new();
            for item in iterator.into_iter() {
                iterated.insert((*item).clone());
                if let Ok(j) = serde_json::to_string(&item) {
                    ctx.text(j);
                }
            }
            for stuff in iterated {
                a.already_send_data.insert(stuff);
            }
        });
    }
}

impl Actor for Ws {
    type Context = ws::WebsocketContext<Self>;
    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.send_data(ctx);
    }
}

/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Ws {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(ws::Message::Ping(msg)) = msg {
            ctx.pong(&msg)
        }
    }
}

pub async fn websocket(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    info!("Got websocket req");
    let resp = ws::start(
        Ws {
            already_send_data: HashSet::new(),
        },
        &req,
        stream,
    );
    info!("Websocket Resp: {:?}", resp);
    resp
}
