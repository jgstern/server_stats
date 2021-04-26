use crate::errors::Errors;
use color_eyre::eyre::Result;
use futures::TryStreamExt;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info};

mod discover;

#[derive(sqlx::FromRow)]
struct DestinationKey {
    destination: String,
}

pub async fn fetch_servers_from_db(pool: &PgPool) -> Result<()> {
    let config = crate::CONFIG.get().expect("unable to get config");
    let postgres_query = config.postgres.query.as_ref();
    let rows = sqlx::query_as::<_, DestinationKey>(postgres_query)
        .fetch(pool)
        .map_err(|e| Errors::DatabaseError(e.to_string()));

    if let Err(e) = rows
        .try_for_each_concurrent(None, |row| async move {
            {
                if crate::CACHE_DB.contains_server(&row.destination) {
                    return Ok(());
                }
            }
            let server_url = crate::matrix::discover::resolve_server_name(&row.destination).await;

            if let Ok(ref server_url) = server_url {
                crate::CACHE_DB
                    .set_server_address(&row.destination, server_url.to_string())
                    .expect("Unable to write to sled");
            }
            Ok(())
        })
        .await
    {
        info!("Error 1: {:?}", e)
    };
    Ok(())
}

pub async fn get_server_version(server_name: &str, client: &reqwest::Client) -> Result<()> {
    let address = crate::CACHE_DB.get_server_address(server_name);
    if let Some(address) = address {
        let address = String::from_utf8_lossy(address.as_ref());

        let resp = client
            .get(format!("https://{}/_matrix/federation/v1/version", address))
            .send()
            .await;
        if let Ok(resp) = resp {
            if resp.status() == StatusCode::OK {
                let body = resp
                    .json::<MatrixVersion>()
                    .await
                    .map_err(|_| Errors::MatrixFederationVersionWronglyConfigured);
                if let Ok(body) = body {
                    debug!(
                        "{}: {} {}",
                        server_name, body.server.name, body.server.version
                    );
                    {
                        crate::CACHE_DB.set_server_version(server_name, body.server)?;
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct MatrixVersion {
    pub(crate) server: MatrixVersionServer,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MatrixVersionServer {
    pub(crate) name: String,
    pub(crate) version: String,
}
