use crate::ssh::auth::SSHAuthMethod;
use envconfig::Envconfig;
use phirepass_common::env::Mode;
use std::time::Duration;

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

    #[envconfig(from = "HOST", default = "0.0.0.0")]
    pub host: String,

    #[envconfig(from = "PORT", default = "8081")]
    pub port: u16,

    #[envconfig(from = "PAT_TOKEN", default = "")]
    pub token: String,

    #[envconfig(from = "STATS_REFRESH_INTERVAL", default = "60")]
    pub stats_refresh_interval: u16,

    #[envconfig(from = "PING_INTERVAL", default = "30")]
    pub ping_interval: u16,

    #[envconfig(from = "SERVER_HOST", default = "0.0.0.0")]
    pub server_host: String,

    #[envconfig(from = "SERVER_PORT", default = "8080")]
    pub server_port: u16,

    #[envconfig(from = "SSH_HOST", default = "0.0.0.0")]
    pub ssh_host: String,

    #[envconfig(from = "SSH_PORT", default = "22")]
    pub ssh_port: u16,

    #[envconfig(from = "SSH_AUTH_METHOD", default = "password")]
    pub ssh_auth_mode: SSHAuthMethod,

    #[envconfig(from = "SSH_INACTIVITY_PERIOD", default = "3600")] // 1 hour
    pub ssh_inactivity_secs: u64,
}

impl Env {
    pub fn get_ssh_inactivity_duration(&self) -> Option<Duration> {
        match self.ssh_inactivity_secs {
            0 => None,
            o => Some(Duration::from_secs(o)),
        }
    }
}

pub(crate) fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
