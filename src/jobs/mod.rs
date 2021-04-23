use futures::stream::{self, StreamExt};
use tracing::{error, info};

pub async fn find_servers() -> color_eyre::Result<()> {
    info!("Started find_servers task");
    crate::matrix::fetch_servers_from_db().await?;
    info!("Finished find_servers task");
    Ok(())
}

pub async fn update_versions() -> color_eyre::Result<()> {
    info!("Started update_versions task");

    let servers_map = crate::SERVERS_CACHE.read().await.clone();
    let servers: Vec<String> = servers_map
        .keys()
        .into_iter()
        .map(|s| s.to_owned())
        .collect();
    let stream = stream::iter(servers);

    stream
        .for_each_concurrent(None, |val| async move {
            if let Err(e) = crate::matrix::get_server_version(val.to_string()).await {
                error!("Failed to get version: {}", e);
            };
        })
        .await;
    info!("Pushing updated versions");
    crate::INFLUX_CLIENT.push_versions().await?;
    info!("Finished update_versions task");
    Ok(())
}
