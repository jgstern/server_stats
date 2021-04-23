use crate::errors::Errors;
use color_eyre::eyre::Result;
use futures::TryStreamExt;
use reqwest::StatusCode;
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tracing::{debug, error};

mod discover;

#[derive(sqlx::FromRow)]
struct DestinationKey {
    destination: String,
}

pub async fn fetch_servers_from_db() -> Result<()> {
    let config = crate::CONFIG.get().expect("unable to get config");
    let postgres_url = config.postgres.url.clone();
    let postgres_query = config.postgres.query.clone();
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .connect(&postgres_url)
        .await?;

    let rows = sqlx::query_as::<_, DestinationKey>(&postgres_query)
        .fetch(&pool)
        .map_err(|e| Errors::DatabaseError(e.to_string()));

    if let Err(e) = rows
        .try_for_each_concurrent(None, |row| async move {
            {
                if crate::SERVERS_CACHE
                    .read()
                    .await
                    .contains_key(&row.destination)
                {
                    return Ok(());
                }
            }
            let server_url =
                crate::matrix::discover::resolve_server_name(row.destination.clone()).await;

            if let Ok(ref server_url) = server_url {
                crate::SERVERS_CACHE
                    .write()
                    .await
                    .insert(row.destination.clone(), server_url.to_string());
            }
            Ok(())
        })
        .await
    {
        error!("{:?}", e)
    };
    pool.close().await;
    Ok(())
}

pub async fn get_server_version(server_name: String) -> Result<()> {
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(crate::APP_USER_AGENT)
        .build()?;

    let read_lock = crate::SERVERS_CACHE.read().await;
    let address = read_lock.get(&server_name).unwrap();

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
                    crate::VERSIONS_CACHE.write().await.insert(
                        server_name,
                        format!("{} {}", body.server.name, body.server.version),
                    );
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixVersion {
    pub(crate) server: MatrixVersionServer,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatrixVersionServer {
    pub(crate) name: String,
    pub(crate) version: String,
}
