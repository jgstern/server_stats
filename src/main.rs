#![deny(unsafe_code)]
use crate::{bot::login_and_sync, config::Config, database::cache::CacheDb, scraping::InfluxDb};
use clap::Clap;
use color_eyre::eyre::Result;
use once_cell::sync::{Lazy, OnceCell};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod bot;
mod config;
mod database;
mod errors;
mod jobs;
mod matrix;
mod scraping;

#[derive(Clap)]
struct Opts {
    #[clap(short, long, default_value = "config.yml")]
    config: String,
}

pub static CONFIG: OnceCell<Config> = OnceCell::new();
pub static PG_POOL: OnceCell<PgPool> = OnceCell::new();
pub static CACHE_DB: Lazy<CacheDb> = Lazy::new(CacheDb::new);

pub static APP_USER_AGENT: &str = concat!("MTRNord/", env!("CARGO_PKG_NAME"),);

pub static INFLUX_CLIENT: Lazy<InfluxDb> = Lazy::new(InfluxDb::new);
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
        //.add_directive("matrix_sdk=debug".parse()?)
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
    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        handle.spawn(async move {
            if let Some(ref bot_config) = crate::CONFIG.get().unwrap().bot {
                info!("Starting Bot...");
                if let Err(e) = login_and_sync(
                    bot_config.homeserver_url.to_string(),
                    bot_config.mxid.to_string(),
                    bot_config.password.to_string(),
                )
                .await
                {
                    error!("Failed to login or start sync: {}", e);
                };
            }
        });
    });

    let config = crate::CONFIG.get().expect("unable to get config");
    let postgres_url = config.postgres.url.as_ref();
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .connect(postgres_url)
        .await?;

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

    let handle = tokio::runtime::Handle::current();
    ctrlc::set_handler(move || {
        handle.spawn(async {
            PG_POOL.get().unwrap().close().await;
            std::process::exit(0);
        });
    })
    .expect("Error setting Ctrl-C handler");

    start_queue().await.unwrap();

    Ok(())
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
    sched.start().await?;
    Ok(())
}
