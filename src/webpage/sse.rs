use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{pin::Pin, sync::Arc};

use actix_web::web::{Bytes, Data};
use actix_web::Error;
use actix_web::{
    rt::time::{interval_at, Instant},
    HttpResponse, Responder,
};
use futures::Stream;
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct Broadcaster {
    clients: Vec<Sender<Bytes>>,
}

pub async fn new_client(broadcaster: Data<Mutex<Broadcaster>>) -> impl Responder {
    let rx = broadcaster.lock().unwrap().new_client();

    HttpResponse::Ok()
        .append_header(("content-type", "text/event-stream"))
        .streaming(rx)
}

impl Broadcaster {
    pub fn create() -> (Arc<Mutex<Self>>, Data<Arc<Mutex<Self>>>) {
        // Data ≃ Arc
        let broadcaster = Arc::new(Mutex::new(Broadcaster::new()));
        let me = Data::new(broadcaster.clone());

        // ping clients every 10 seconds to see if they are alive
        Broadcaster::spawn_ping(me.clone());

        (broadcaster, me)
    }

    fn new() -> Self {
        Broadcaster {
            clients: Vec::new(),
        }
    }

    fn spawn_ping(me: Data<Arc<Mutex<Self>>>) {
        actix_web::rt::spawn(async move {
            let mut task = interval_at(Instant::now(), Duration::from_secs(10));
            loop {
                task.tick().await;
                me.lock().unwrap().remove_stale_clients();
            }
        });
    }

    fn remove_stale_clients(&mut self) {
        let mut ok_clients = Vec::new();
        for client in self.clients.iter() {
            let result = client.clone().try_send(Bytes::from("data: ping\n\n"));

            if let Ok(()) = result {
                ok_clients.push(client.clone());
            }
        }
        self.clients = ok_clients;
    }

    pub fn new_client(&mut self) -> Client {
        let (tx, rx) = channel(100);

        tx.try_send(Bytes::from("data: connected\n\n")).unwrap();

        self.clients.push(tx);
        Client(rx)
    }

    pub fn send(&self, msg: &str) {
        let msg = Bytes::from(["data: ", msg, "\n\n"].concat());

        for client in self.clients.iter() {
            client.clone().try_send(msg.clone()).unwrap_or(());
        }
    }
}
// wrap Receiver in own type, with correct error type
pub struct Client(Receiver<Bytes>);

impl Stream for Client {
    type Item = Result<Bytes, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.0).poll_recv(cx) {
            Poll::Ready(Some(v)) => Poll::Ready(Some(Ok(v))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
