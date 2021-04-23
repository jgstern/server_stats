use crate::errors::Errors;
use influxdb_client::derives::PointSerialize;
use influxdb_client::{Client, PointSerialize, Precision, Timestamp, TimestampOptions};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};
pub struct InfluxDb {
    client: Client,
}

impl InfluxDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn push_versions(&self) -> color_eyre::Result<()> {
        let servers_map = crate::SERVERS_CACHE.read().await.clone();
        let versions_map = crate::VERSIONS_CACHE.read().await.clone();
        let servers = servers_map.keys();
        let mut points: Vec<ServerVersion> = Vec::new();
        info!("Server amount: {}", servers.len());
        for server_name in servers {
            if let Some(version) = versions_map.get(server_name) {
                let now = SystemTime::now();
                let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

                let point = ServerVersion {
                    server_name: server_name.clone(),
                    version: version.clone(),
                    timestamp: Timestamp::from(since_the_epoch.as_millis() as i64),
                };
                points.push(point);
            } else {
                //println!("Server ({}) has no version yet", server_name);
            }
        }
        if points.is_empty() {
            info!("No points!");
            return Ok(());
        }
        let points_chunks: Vec<&[ServerVersion]> = points.chunks(40).collect();
        for chunk in points_chunks {
            // Insert without timestamp - InfluxDB will automatically set the timestamp
            if let Err(e) = self
                .client
                .insert_points(chunk, TimestampOptions::None)
                .await
                .map_err(|e| Errors::InfluxDbError(format!("{:?}", e)))
            {
                error!("Got influxdb error: {}", e);
            };
        }
        Ok(())
    }
}
impl Default for InfluxDb {
    fn default() -> Self {
        let config = crate::CONFIG.get().expect("unable to get config");
        let host = config.influxdb.host.clone();
        let token = config.influxdb.token.clone();
        let bucket = config.influxdb.bucket.clone();
        let org = config.influxdb.org.clone();
        let client = Client::new(host, token)
            .with_bucket(bucket)
            .with_org(org)
            .with_precision(Precision::MS);
        Self { client }
    }
}

#[derive(PointSerialize)]
#[point(measurement = "server_version")]
struct ServerVersion {
    #[point(tag)]
    server_name: String,
    #[point(field = "version")]
    version: String,
    #[point(timestamp)]
    timestamp: Timestamp,
}
