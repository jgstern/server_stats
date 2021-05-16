#![deny(unsafe_code)]

use crate::{
    appservice::generate_appservice,
    config::Config,
    database::cache::CacheDb,
    scraping::InfluxDb,
    webpage::{
        ar_page, assets, index_page, two_d_page, vr_page, webpage,
        ws::{websocket, Ws},
    },
};
use actix::Addr;
use actix_web::{
    get,
    middleware::{Compress, Logger},
    web::{self},
    App, HttpResponse, HttpServer, Responder,
};
use actix_web_prom::PrometheusMetricsBuilder;
use chrono::{prelude::*, Duration};
use clap::Clap;
use color_eyre::eyre::Result;
use matrix_sdk::{
    events::{custom::CustomEventContent, AnyStateEventContent},
    identifiers::RoomId,
    Client,
};
use once_cell::sync::{Lazy, OnceCell};
use prometheus::{opts, register_gauge, Gauge, Registry};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::{BTreeMap, HashMap},
    convert::TryFrom,
    sync::Arc,
};
use tokio::sync::{RwLock, Semaphore};
use tokio::time::sleep;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod appservice;
mod config;
mod database;
mod errors;
mod jobs;
mod matrix;
mod scraping;
mod webpage;

#[derive(Clap)]
struct Opts {
    #[clap(short, long, default_value = "./config.yml")]
    config: String,
}

pub static CONFIG: OnceCell<Config> = OnceCell::new();
pub static ROOMS_TOTAL_COUNTER: Lazy<Gauge> = Lazy::new(|| {
    let opts = opts!("rooms_total", "Rooms statistics").namespace("server_stats");
    register_gauge!(opts).unwrap()
});
pub static WEBSOCKET_CLIENTS: Lazy<RwLock<HashMap<String, Addr<Ws>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
pub static MATRIX_CLIENT: OnceCell<Client> = OnceCell::new();
pub static PG_POOL: OnceCell<PgPool> = OnceCell::new();
pub static CACHE_DB: Lazy<CacheDb> = Lazy::new(CacheDb::new);
pub static MESSAGES_SEMPAHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(300)));

pub static APP_USER_AGENT: &str = concat!("MTRNord/", env!("CARGO_PKG_NAME"),);

pub static INFLUX_CLIENT: Lazy<InfluxDb> = Lazy::new(InfluxDb::new);

// Marks all rooms to have history purged
async fn force_cleanup() -> Result<()> {
    let now = Utc::now();
    let time = now - Duration::days(2);
    let timestamp = time.timestamp_millis();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let room_ids: Vec<String> = CACHE_DB
        .graph
        .get_all_room_ids()
        .map(|val| {
            let room_id_bytes = val.expect("unable to get room_id from sled");
            let room_id = std::str::from_utf8(room_id_bytes.as_ref())
                .unwrap()
                .to_owned();
            room_id
        })
        .collect();
    let server_address = &crate::CONFIG.get().unwrap().bot.homeserver_url;
    let map = serde_json::json!({"delete_local_events": false, "purge_up_to_ts":timestamp});
    let auth_header = format!(
        "Bearer {}",
        crate::CONFIG.get().unwrap().bot.admin_access_token
    );

    for room_id in room_ids {
        let url = format!(
            "{}/_synapse/admin/v1/purge_history/{}",
            server_address, room_id
        );
        info!("{}", url);
        let body = client
            .post(url.clone())
            .header("Authorization", auth_header.clone())
            .json(&map)
            .send()
            .await?
            .text()
            .await?;

        info!("{} = {:?}", url, body);
        sleep(std::time::Duration::from_secs(5)).await;
    }

    Ok(())
}

#[get("/relations")]
async fn relations() -> impl Responder {
    let data = crate::CACHE_DB.graph.get_json_relations();
    HttpResponse::Ok().json(data.await)
}

#[actix_web::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let filter = EnvFilter::from_default_env()
        // Set the base level when not matched by other directives to WARN.
        .add_directive(LevelFilter::ERROR.into())
        // Set the max level for `my_crate::my_mod` to DEBUG, overriding
        // any directives parsed from the env variable.
        .add_directive("server_stats=info".parse()?)
        .add_directive("sled=info".parse()?)
        //.add_directive("matrix_sdk=debug".parse()?)
        //.add_directive("actix_server=info".parse()?)
        //.add_directive("actix_web=info".parse()?)
        .add_directive("rustls::session=off".parse()?);

    tracing_subscriber::fmt()
        .pretty()
        .with_thread_names(true)
        .with_env_filter(filter)
        .init();

    info!("Starting...");
    let opts: Opts = Opts::parse();

    info!("Loading Configs...");
    let config = Config::load(opts.config)?;
    CONFIG.set(config);

    if crate::CONFIG.get().unwrap().bot.force_cleanup {
        force_cleanup().await?;
        return Ok(());
    }

    info!("Setting up prometheus...");

    let registry = Registry::new();
    registry
        .register(Box::new(ROOMS_TOTAL_COUNTER.clone()))
        .expect("Creating a prometheus registry");

    let prometheus = PrometheusMetricsBuilder::new("api")
        .registry(registry)
        .endpoint("/metrics")
        .build()
        .unwrap();

    tokio::spawn(async move {
        info!("Connecting to postgres...");
        let config = crate::CONFIG.get().expect("unable to get config");
        let postgres_url = config.postgres.url.as_ref();
        let pool = PgPoolOptions::new()
            .max_connections(100)
            .connect(postgres_url)
            .await;
        if let Ok(pool) = pool {
            // Get servers once
            if let Err(e) = crate::jobs::find_servers(&pool).await {
                error!("Error servers: {}", e);
            }

            if let Err(e) = crate::jobs::update_versions().await {
                error!("Error versions: {}", e);
            }

            // Starting sheduler
            info!("Starting sheduler");
            PG_POOL.set(pool);

            start_queue().await.unwrap();
        };
    });

    info!("Starting appservice...");
    let config = crate::CONFIG.get().expect("unable to get config");

    let appservice = generate_appservice(&config).await;

    info!("Starting webserver...");
    if let Err(e) = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .service(web::resource("/ws").to(websocket))
            .wrap(prometheus.clone())
            .wrap(Compress::default())
            .service(web::resource("/health").to(|| actix_web::HttpResponse::Ok().finish()))
            .service(web::resource("/").to(index_page))
            .service(web::resource("/2d").to(two_d_page))
            .service(web::resource("/vr").to(vr_page))
            .service(web::resource("/ar").to(ar_page))
            .route("/assets/{filename:.*}", web::get().to(assets))
            .service(relations)
            .service(appservice.actix_service())
            .route("/{filename:.*}", web::get().to(webpage))
    })
    .bind((config.api.ip.to_string(), config.api.port))?
    .run()
    .await
    {
        error!("Failed to start webserver because: {}", e);
    }

    if let Some(pool) = PG_POOL.get() {
        pool.close().await;
    }
    std::process::exit(0);
}

async fn start_queue() -> Result<()> {
    let mut sched = JobScheduler::new();

    sched
        .add(
            //Should be */30
            Job::new("0 */30 * * * *", |_, _| {
                tokio::spawn(async {
                    if let Err(e) = crate::jobs::find_servers(&PG_POOL.get().unwrap()).await {
                        error!("Error: {}", e);
                    }
                });
            })
            .unwrap(),
        )
        .expect("failed to shedule job");

    sched
        .add(
            //Should be 5m
            Job::new("0 */5 * * * *", |_, _| {
                tokio::spawn(async move {
                    if let Err(e) = crate::jobs::update_versions().await {
                        error!("Error: {}", e);
                    }
                });
            })
            .unwrap(),
        )
        .expect("failed to shedule job");

    sched
        .add(
            //Should be */5
            Job::new("0 */5 * * * *", |_, _| {
                tokio::spawn(async {
                    if let Some(client) = crate::MATRIX_CLIENT.get() {
                        let joined_rooms = client.joined_rooms().len();
                        //TODO make sure to filter only banned ones here .iter().filter(|x|{x.})
                        let banned_rooms = client.left_rooms().len();
                        let total = joined_rooms + banned_rooms;
                        crate::ROOMS_TOTAL_COUNTER.set(total as f64);
                        assert_eq!(crate::ROOMS_TOTAL_COUNTER.get() as i64, total as i64);
                        //TODO allow configuration
                        let room = crate::MATRIX_CLIENT.get().unwrap().get_joined_room(
                            &RoomId::try_from("!zeFBFCASPaEUIHzbqj:nordgedanken.dev").unwrap(),
                        );
                        if let Some(room) = room {
                            info!("Updating counter in public room");
                            let mut data = BTreeMap::new();
                            data.insert("link".to_string(), serde_json::json!(""));
                            data.insert("severity".to_string(), serde_json::json!("normal"));
                            data.insert("title".to_string(), serde_json::json!("Rooms found"));
                            data.insert("value".to_string(), serde_json::json!(total));
                            let state_event = AnyStateEventContent::Custom(CustomEventContent {
                                event_type: "re.jki.counter".into(),
                                data,
                            });
                            if let Err(e) = room.send_state_event(state_event, "rooms_found").await
                            {
                                error!("Failed to update room counter: {}", e);
                            }
                        }
                    }
                });
            })
            .unwrap(),
        )
        .expect("failed to shedule job");

    sched.start().await?;
    Ok(())
}
