use actix::prelude::*;
use actix::{Actor, Handler, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::time::{Duration, Instant};
use tracing::info;

use crate::WEBSOCKET_CLIENTS;

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Define HTTP actor
pub struct Ws {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
    id: String,
}

impl Ws {
    fn new(id: String) -> Self {
        Self {
            hb: Instant::now(),
            id,
        }
    }

    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                info!("Websocket Client heartbeat failed, disconnecting!");

                let id = act.id.clone();
                let remove_client = async move {
                    // remove from clients
                    WEBSOCKET_CLIENTS.write().await.remove(&id);
                };

                let remove_client = actix::fut::wrap_future::<_, Self>(remove_client);
                ctx.wait(remove_client);

                // stop actor
                ctx.stop();

                // don't try to send a ping
                return;
            }

            ctx.ping(b"");
        });
    }
}

impl Actor for Ws {
    type Context = ws::WebsocketContext<Self>;
    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
    }
}

/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for Ws {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            _ => (),
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct WsMessage {
    pub msg: String,
}

impl Handler<WsMessage> for Ws {
    type Result = ();
    fn handle(&mut self, msg: WsMessage, ctx: &mut Self::Context) {
        ctx.text(msg.msg);
    }
}

pub async fn websocket(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(30)
        .map(char::from)
        .collect();
    let (addr, resp) = ws::start_with_addr(Ws::new(rand_string.clone()), &req, stream)?;
    WEBSOCKET_CLIENTS.write().await.insert(rand_string, addr);
    info!("Websocket Resp: {:?}", resp);
    Ok(resp)
}
