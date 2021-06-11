use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Config {
    pub postgres: Postgres,
    pub influxdb: InfluxDb,
    pub bot: Bot,
    pub api: Api,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Postgres {
    pub url: String,
    pub query: String,
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct InfluxDb {
    pub host: String,
    pub token: String,
    pub org: String,
    pub bucket: String,
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Bot {
    pub homeserver_url: String,
    pub server_name: String,
    pub mxid: String,
    pub password: String,
    pub force_cleanup: bool,
    pub force_reindex_of_joined_rooms: bool,
    pub admin_access_token: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Api {
    pub ip: String,
    pub webpage_path: String,
    pub port: u16,
}

impl Config {
    #[tracing::instrument]
    pub fn load<P: AsRef<std::path::Path> + std::fmt::Debug>(
        path: P,
    ) -> Result<Self, crate::errors::Errors> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}
