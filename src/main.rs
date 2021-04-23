use crate::{config::Config, scraping::InfluxDb};
use clap::Clap;
use color_eyre::eyre::Result;
use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashMap;
use tokio::sync::RwLock;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

mod config;
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

pub static APP_USER_AGENT: &str = concat!("MTRNord/", env!("CARGO_PKG_NAME"),);

pub static SERVERS_CACHE: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub static VERSIONS_CACHE: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

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

    // Get servers once
    if let Err(e) = crate::jobs::find_servers().await {
        error!("Error: {}", e);
    }

    if let Err(e) = crate::jobs::update_versions().await {
        error!("Error: {}", e);
    }

    // Starting sheduler
    info!("Starting sheduler");
    start_queue().await?;
    Ok(())
}

async fn start_queue() -> Result<()> {
    let mut sched = JobScheduler::new();

    sched
        .add(
            //Should be */30
            Job::new("0 */30 * * * *", |_, _| {
                tokio::spawn(async move {
                    if let Err(e) = crate::jobs::find_servers().await {
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
