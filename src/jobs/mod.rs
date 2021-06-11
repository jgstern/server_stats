use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use tracing::{error, info};

use crate::{config::Config, database::cache::CacheDb, scraping::InfluxDb};

pub async fn find_servers(
    pool: &PgPool,
    cache: &CacheDb,
    config: &Config,
) -> color_eyre::Result<()> {
    info!("Started find_servers task");

    crate::matrix::fetch_servers_from_db(pool, config, cache).await?;
    info!("Finished find_servers task");
    Ok(())
}

pub async fn update_versions(cache: &CacheDb, influx_db: InfluxDb) -> color_eyre::Result<()> {
    info!("Started update_versions task");

    let servers = cache.get_all_servers();
    let stream = stream::iter(servers);

    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(crate::APP_USER_AGENT)
        .build()?;

    stream
        .map(|val| async move {
            let server_address_bytes = val.expect("unable to get server_address from sled");
            let server_address = std::str::from_utf8(server_address_bytes.as_ref())
                .unwrap()
                .replace("address/", "");
            server_address
        })
        .for_each_concurrent(None, |server_address| async {
            if let Err(e) =
                crate::matrix::fetch_server_version(&server_address.await, &client, &cache.clone())
                    .await
            {
                error!("Failed to get version: {}", e);
            };
        })
        .await;
    info!("Pushing updated versions");
    influx_db.push_versions(&cache).await?;
    info!("Finished update_versions task");
    Ok(())
}
