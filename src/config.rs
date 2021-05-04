use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Config<'input> {
    pub postgres: Postgres<'input>,
    pub influxdb: InfluxDb<'input>,
    pub bot: Option<Bot<'input>>,
    pub api: Api<'input>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Postgres<'input> {
    pub url: Cow<'input, str>,
    pub query: Cow<'input, str>,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct InfluxDb<'input> {
    pub host: Cow<'input, str>,
    pub token: Cow<'input, str>,
    pub org: Cow<'input, str>,
    pub bucket: Cow<'input, str>,
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Bot<'input> {
    pub homeserver_url: Cow<'input, str>,
    pub mxid: Cow<'input, str>,
    pub password: Cow<'input, str>,
    pub force_cleanup: bool,
    pub force_reindex_of_joined_rooms: bool,
    pub admin_access_token: Cow<'input, str>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Api<'input> {
    pub ip: Cow<'input, str>,
    pub port: u16,
}

impl Config<'_> {
    pub fn load<P: AsRef<std::path::Path> + std::fmt::Debug>(
        path: P,
    ) -> Result<Self, crate::errors::Errors> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&contents)?;
        Ok(config)
    }
}
