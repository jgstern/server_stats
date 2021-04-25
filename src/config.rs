use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Config<'a> {
    pub postgres: Postgres<'a>,
    pub influxdb: InfluxDb<'a>,
    pub bot: Option<Bot<'a>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Postgres<'a> {
    pub url: Cow<'a, str>,
    pub query: Cow<'a, str>,
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct InfluxDb<'a> {
    pub host: Cow<'a, str>,
    pub token: Cow<'a, str>,
    pub org: Cow<'a, str>,
    pub bucket: Cow<'a, str>,
}
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Bot<'a> {
    pub homeserver_url: Cow<'a, str>,
    pub mxid: Cow<'a, str>,
    pub password: Cow<'a, str>,
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
