use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;

#[derive(Debug, Error)]
struct AgentMessageError(pub &'static str);

impl Display for AgentMessageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{0}", self.0)
    }
}

impl From<AgentMessageError> for AgentError {
    fn from(message: AgentMessageError) -> Self {
        AgentError::Other(Box::new(message))
    }
}

pub fn message_error<T>(msg: &'static str) -> Result<T, AgentError> {
    Err(AgentMessageError(msg).into())
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("russh error: {0}")]
    Russh(#[from] russh::Error),

    #[error("russh sftp error: {0}")]
    RusshSFTP(#[from] russh_sftp::client::error::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
