use envconfig::Envconfig;
use phirepass_common::env::Mode;

#[derive(Envconfig)]
pub(crate) struct Env {
    #[envconfig(from = "APP_MODE", default = "development")]
    #[allow(dead_code)]
    pub mode: Mode,

    #[envconfig(from = "HTTP_HOST", default = "0.0.0.0")]
    pub host: String,

    #[envconfig(from = "HTTP_PORT", default = "8080")]
    pub port: u16,

    #[envconfig(from = "STATS_REFRESH_INTERVAL", default = "15")]
    pub stats_refresh_interval: u16,
}

pub fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}
