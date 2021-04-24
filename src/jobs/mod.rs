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

    let servers = crate::CACHE_DB.get_all_servers();
    let stream = stream::iter(servers);

    stream
        .for_each_concurrent(None, |val| async move {
            let server_address_bytes = val.expect("unable to get server_address from sled");
            let server_address =
                String::from_utf8_lossy(server_address_bytes.as_ref()).replace("address/", "");
            if let Err(e) = crate::matrix::get_server_version(server_address.to_string()).await {
                error!("Failed to get version: {}", e);
            };
        })
        .await;
    info!("Pushing updated versions");
    crate::INFLUX_CLIENT.push_versions().await?;
    info!("Finished update_versions task");
    Ok(())
}
