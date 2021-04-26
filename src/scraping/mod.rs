use crate::errors::Errors;
use futures::stream::{self, StreamExt};
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
        let servers_map = crate::CACHE_DB.get_all_servers();
        let mut points: Vec<ServerVersion> = Vec::new();
        servers_map
            .map(|x| {
                let server_name_bytes = x.expect("unable to get bytes from server_keys");
                let server_name_untrimmed =
                    std::str::from_utf8(server_name_bytes.as_ref()).unwrap();
                server_name_untrimmed.replace("address/", "")
            })
            .for_each(|server_name| {
                match crate::CACHE_DB.get_server_version(&server_name) {
                    Ok(version) => {
                        if let Some(version) = version {
                            let now = SystemTime::now();
                            let since_the_epoch =
                                now.duration_since(UNIX_EPOCH).expect("Time went backwards");

                            let point = ServerVersion {
                                server_name: server_name.to_string(),
                                version: format!("{} {}", version.name, version.version),
                                timestamp: Timestamp::from(since_the_epoch.as_millis() as i64),
                            };
                            points.push(point);
                        } else {
                            //println!("Server ({}) has no version yet", server_name);
                        }
                    }
                    Err(e) => {
                        error!("Failed to find server_version: {}", e);
                    }
                }
            });
        if points.is_empty() {
            info!("No points!");
            return Ok(());
        }
        stream::iter(points)
            .chunks(40)
            .for_each(|chunk| async move {
                // Insert without timestamp - InfluxDB will automatically set the timestamp
                if let Err(e) = self
                    .client
                    .insert_points(&chunk, TimestampOptions::None)
                    .await
                    .map_err(|e| Errors::InfluxDbError(format!("{:?}", e)))
                {
                    error!("Got influxdb error: {}", e);
                };
            })
            .await;
        Ok(())
    }
}
impl Default for InfluxDb {
    fn default() -> Self {
        let config = crate::CONFIG.get().expect("unable to get config");
        let host = config.influxdb.host.as_ref();
        let token = config.influxdb.token.as_ref();
        let bucket = config.influxdb.bucket.as_ref();
        let org = config.influxdb.org.as_ref();
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
