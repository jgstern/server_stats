use crate::errors::Errors;
use regex::Regex;
use reqwest::StatusCode;
use serde::Deserialize;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    TokioAsyncResolver,
};

#[derive(Debug)]
pub enum MatrixSsServername {
    Ip(SocketAddr),
    Host(String),
}

impl fmt::Display for MatrixSsServername {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatrixSsServername::Ip(ip) => write!(f, "{}", ip),
            MatrixSsServername::Host(host) => write!(f, "{}", host),
        }
    }
}

/// Resolves the server_name for usage with matrix S-S-Api according too https://matrix.org/docs/spec/server_server/latest#server-discovery
/// It is required that the HOST header always gets set to the server_name when it is returning a IP.
pub async fn resolve_server_name(server_name: &str) -> Result<MatrixSsServername, Errors> {
    // If ip literal with port
    if let Ok(addr) = SocketAddr::from_str(&server_name) {
        return Ok(MatrixSsServername::Ip(addr));
    } else if let Ok(ip) = IpAddr::from_str(&server_name) {
        // If ip without port
        return Ok(MatrixSsServername::Ip(SocketAddr::new(ip, 8448)));
    }

    let resolver = TokioAsyncResolver::tokio(ResolverConfig::quad9_tls(), ResolverOpts::default())
        .map_err(|_| Errors::MatrixFederationWronglyConfigured)?;

    // If has hostname and port
    let port_re = Regex::new(r":([0-9]+)$").unwrap();
    if port_re.is_match(&server_name) {
        let caps = port_re.captures(&server_name).unwrap();
        let hostname = port_re.replace(&server_name, "");
        let port = caps.get(1).unwrap().as_str();

        // Get AAAA/A record
        let results = resolver
            .lookup_ip(hostname.to_string())
            .await
            .map_err(|_| Errors::MatrixFederationWronglyConfigured)?;

        let ip: IpAddr = results
            .iter()
            .next()
            .ok_or(Errors::MatrixFederationWronglyConfigured)?;
        return Ok(MatrixSsServername::Ip(SocketAddr::new(
            ip,
            port.parse().unwrap(),
        )));
    }

    // Check well-known file
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(crate::APP_USER_AGENT)
        .build()?;

    let resp = client
        .get(&format!(
            "https://{}/.well-known/matrix/server",
            server_name
        ))
        .send()
        .await;

    if let Ok(resp) = resp {
        if resp.status() == StatusCode::OK || resp.status().is_redirection() {
            let body: MatrixSsWellKnown = resp
                .json::<MatrixSsWellKnown>()
                .await
                .map_err(|_| Errors::MatrixFederationWronglyConfigured)?;

            return if port_re.is_match(&body.server) {
                let caps = port_re.captures(&body.server).unwrap();
                let hostname = port_re.replace(&body.server, "");
                let port = caps.get(1).unwrap().as_str();
                Ok(MatrixSsServername::Host(format!("{}:{}", hostname, port)))
            } else {
                // FIXME if we have a hostname and no port we actually shall check SRV first
                Ok(MatrixSsServername::Host(format!("{}:8448", body.server)))
            };
        }
    }

    // Check SRV record
    let results = resolver
        .srv_lookup(format!("_matrix._tcp.{}", server_name))
        .await;
    if let Ok(results) = results {
        let first = results
            .iter()
            .next()
            .ok_or(Errors::MatrixFederationWronglyConfigured)?;
        let target = first.target().to_string();
        let host = target.trim_end_matches('.');
        let port = first.port();
        return Ok(MatrixSsServername::Ip(SocketAddr::new(
            IpAddr::from_str(host).map_err(|_| Errors::MatrixFederationWronglyConfigured)?,
            port,
        )));
    }
    Ok(MatrixSsServername::Host(format!("{}:8448", server_name)))
}

#[derive(Debug, Deserialize)]
pub struct MatrixSsWellKnown {
    #[serde(rename = "m.server")]
    pub(crate) server: String,
}
