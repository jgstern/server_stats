#![deny(unsafe_code)]

use crate::{
    config::Config,
    database::cache::CacheDb,
    scraping::InfluxDb,
    webpage::{init_prometheus, run_server},
};
use chrono::{prelude::*, Duration};
use clap::Clap;
use color_eyre::eyre::Result;
use matrix_sdk::{
    events::{custom::CustomEventContent, AnyStateEventContent},
    identifiers::RoomId,
    Client,
};
use once_cell::sync::{Lazy, OnceCell};
use opentelemetry::metrics::ValueRecorder;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    sync::Arc,
};
use tokio::sync::{watch, Semaphore};
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

pub static MATRIX_CLIENT: OnceCell<Client> = OnceCell::new();
pub static PG_POOL: OnceCell<PgPool> = OnceCell::new();
pub static MESSAGES_SEMPAHORE: Lazy<Arc<Semaphore>> = Lazy::new(|| Arc::new(Semaphore::new(50)));

pub static APP_USER_AGENT: &str = concat!("MTRNord/", env!("CARGO_PKG_NAME"),);

// Marks all rooms to have history purged
async fn force_cleanup(cache: &CacheDb, config: &Config) -> Result<()> {
    let now = Utc::now();
    let time = now - Duration::days(2);
    let timestamp = time.timestamp_millis();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let room_ids: Vec<String> = cache
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
    let server_address = &config.bot.homeserver_url;
    let map = serde_json::json!({"delete_local_events": false, "purge_up_to_ts":timestamp});
    let auth_header = format!("Bearer {}", config.bot.admin_access_token);

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
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let filter = EnvFilter::from_default_env()
        // Set the base level when not matched by other directives to WARN.
        .add_directive(LevelFilter::ERROR.into())
        // Set the max level for `my_crate::my_mod` to DEBUG, overriding
        // any directives parsed from the env variable.
        .add_directive("server_stats=info".parse()?)
        .add_directive("sled=info".parse()?)
        //.add_directive("matrix_sdk=info".parse()?)
        //.add_directive("matrix_sdk_base::client=off".parse()?)
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

    let (tx, rx) = watch::channel(None);
    let cache = CacheDb::new(tx);

    if config.bot.force_cleanup {
        force_cleanup(&cache, &config).await?;
        return Ok(());
    }

    info!("Setting up prometheus...");

    let cloned_cache = cache.clone();
    let cloned_config = config.clone();
    let (exporter, recorder) = init_prometheus();
    tokio::spawn(async move {
        let influx_db = InfluxDb::new(&cloned_config);
        let cache = cloned_cache.clone();
        let config = cloned_config.clone();
        info!("Connecting to postgres...");
        let postgres_url = config.postgres.url.as_ref();
        let pool = PgPoolOptions::new()
            .max_connections(100)
            .connect(postgres_url)
            .await;
        if let Ok(pool) = pool {
            // Get servers once
            if let Err(e) = crate::jobs::find_servers(&pool, &cache, &config).await {
                error!("Error servers: {}", e);
            }

            if let Err(e) = crate::jobs::update_versions(&cache, influx_db.clone()).await {
                error!("Error versions: {}", e);
            }

            // Starting sheduler
            info!("Starting sheduler");
            PG_POOL.set(pool);

            start_queue(cache, influx_db, config.clone(), Arc::new(recorder))
                .await
                .unwrap();
        };
    });

    info!("Starting webserver...");
    run_server(&config, cache, rx, exporter).await;

    if let Some(pool) = PG_POOL.get() {
        pool.close().await;
    }
    std::process::exit(0);
}

async fn start_queue(
    cache: CacheDb,
    influx_db: InfluxDb,
    config: Config,
    recorder: Arc<ValueRecorder<i64>>,
) -> Result<()> {
    let mut sched = JobScheduler::new();

    let cache_two = cache.clone();
    sched
        .add(
            //Should be */30
            Job::new("0 */30 * * * *", move |_, _| {
                let cache = cache_two.clone();
                let config = config.clone();
                tokio::spawn(async move {
                    if let Err(e) = crate::jobs::find_servers(
                        &PG_POOL.get().unwrap(),
                        &cache.clone(),
                        &config.clone(),
                    )
                    .await
                    {
                        error!("Error: {}", e);
                    }
                });
            })
            .unwrap(),
        )
        .expect("failed to shedule job");

    let cache_three = cache.clone();
    sched
        .add(
            //Should be 5m
            Job::new("0 */5 * * * *", move |_, _| {
                let cache = cache_three.clone();
                let influx_db = influx_db.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        crate::jobs::update_versions(&cache.clone(), influx_db.clone()).await
                    {
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
            Job::new("0 */5 * * * *", move |_, _| {
                let recorder = recorder.clone();
                tokio::spawn(async move {
                    if let Some(client) = crate::MATRIX_CLIENT.get() {
                        let joined_rooms = client.joined_rooms().len();
                        //TODO make sure to filter only banned ones here .iter().filter(|x|{x.})
                        let banned_rooms = client.left_rooms().len();
                        let total = joined_rooms + banned_rooms;
                        recorder.record(total.try_into().unwrap(), &[]);
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
