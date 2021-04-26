use std::time::Duration;

use futures::stream::{self, StreamExt};
use sqlx::PgPool;
use tracing::{error, info};

pub async fn find_servers(pool: &PgPool) -> color_eyre::Result<()> {
    info!("Started find_servers task");

    crate::matrix::fetch_servers_from_db(pool).await?;
    info!("Finished find_servers task");
    Ok(())
}

pub async fn update_versions() -> color_eyre::Result<()> {
    info!("Started update_versions task");

    let servers = crate::CACHE_DB.get_all_servers();
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
            if let Err(e) = crate::matrix::get_server_version(&server_address.await, &client).await
            {
                error!("Failed to get version: {}", e);
            };
        })
        .await;
    info!("Pushing updated versions");
    crate::INFLUX_CLIENT.push_versions().await?;
    info!("Finished update_versions task");
    Ok(())
}
