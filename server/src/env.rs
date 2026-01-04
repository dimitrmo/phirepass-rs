use axum_client_ip::ClientIpSource;
use envconfig::Envconfig;
use phirepass_common::env::Mode;

#[derive(Envconfig)]
pub(crate) struct Env {
    #[cfg_attr(
        debug_assertions,
        envconfig(from = "APP_MODE", default = "development")
    )]
    #[cfg_attr(
        not(debug_assertions),
        envconfig(from = "APP_MODE", default = "production")
    )]
    #[allow(dead_code)]
    pub mode: Mode,

    #[envconfig(from = "IP_SOURCE", default = "ConnectInfo")]
    pub(crate) ip_source: ClientIpSource,

    #[envconfig(from = "HOST", default = "0.0.0.0")]
    pub host: String,

    #[envconfig(from = "PORT", default = "8080")]
    pub port: u16,

    #[envconfig(from = "STATS_REFRESH_INTERVAL", default = "60")]
    pub stats_refresh_interval: u16,

    #[envconfig(from = "ACCESS_CONTROL_ALLOW_ORIGIN")]
    pub access_control_allowed_origin: Option<String>,
}

pub fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
