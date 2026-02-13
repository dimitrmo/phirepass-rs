use envconfig::Envconfig;
use phirepass_common::env::Mode;

#[derive(Envconfig)]
pub(crate) struct Env {
    #[envconfig(from = "APP_MODE", default = "production")]
    pub mode: Mode,

    #[envconfig(from = "HOST", default = "0.0.0.0")]
    pub host: String,

    #[envconfig(from = "PORT", default = "8000")]
    pub port: u16,

    #[envconfig(from = "REDIS_DATABASE_URL", default = "redis://127.0.0.1")]
    pub redis_database_url: String,
}

pub(crate) fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
