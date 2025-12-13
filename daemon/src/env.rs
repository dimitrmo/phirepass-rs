use envconfig::Envconfig;
use phirepass_common::env::Mode;

#[derive(Clone, Debug)]
pub enum SSHAuthMethod {
    CredentialsPrompt,
}

impl std::str::FromStr for SSHAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "credentials_prompt" => Ok(SSHAuthMethod::CredentialsPrompt),
            _ => Err(format!("invalid authentication method: {}", s)),
        }
    }
}

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

    #[envconfig(from = "PAT_TOKEN", default = "")]
    pub token: String,

    #[envconfig(from = "STATS_REFRESH_INTERVAL", default = "15")]
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

    #[envconfig(from = "SSH_AUTH_METHOD", default = "credentials_prompt")]
    pub ssh_auth_mode: SSHAuthMethod,
}

pub(crate) fn init() -> anyhow::Result<Env> {
    let config = Env::init_from_env()?;
    Ok(config)
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
