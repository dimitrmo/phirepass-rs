use envconfig::Envconfig;
use phirepass_common::env::Mode;

#[derive(Envconfig)]
pub(crate) struct Env {
    #[envconfig(from = "APP_MODE", default = "development")]
    pub mode: Mode,

    #[envconfig(from = "HTTP_HOST", default = "0.0.0.0")]
    pub host: String,

    #[envconfig(from = "HTTP_PORT", default = "8081")]
    pub port: u16,

    #[envconfig(from = "PAT_TOKEN", default = "")]
    pub token: String,

    #[envconfig(from = "STATS_REFRESH_INTERVAL", default = "15")]
    pub stats_refresh_interval: u16,

    #[envconfig(from = "SERVER_HOST", default = "0.0.0.0")]
    pub server_host: String,

    #[envconfig(from = "SERVER_PORT", default = "3000")]
    pub server_port: u16,

    #[envconfig(from = "SSH_HOST", default = "0.0.0.0")]
    pub ssh_host: String,

    #[envconfig(from = "SSH_PORT", default = "22")]
    pub ssh_port: u16,

    #[envconfig(from = "SSH_USER")]
    pub ssh_user: String,
}

pub(crate) fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}
