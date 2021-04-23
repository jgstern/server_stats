use thiserror::Error;
#[derive(Error, Debug)]
pub enum Errors {
    #[error("Unable to open file")]
    FileMissing(#[from] std::io::Error),
    #[error("Unable to request another server")]
    ReqwestError(#[from] reqwest::Error),
    #[error("Unable to understand yaml")]
    SerdeYamlError(#[from] serde_yaml::Error),
    #[error("Error with the database: '{0}'")]
    DatabaseError(String),
    #[error("Error when talking to influxdb: '{0}'")]
    InfluxDbError(String),
    #[error("You Matrix Server is not configured correctly")]
    MatrixFederationWronglyConfigured,
    #[error("You Matrix Server is not reporting a version")]
    MatrixFederationVersionWronglyConfigured,
}
