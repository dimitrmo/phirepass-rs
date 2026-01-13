use std::fmt::Debug;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("russh error: {0}")]
    Russh(russh::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),

    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

pub type Result<T> = std::result::Result<T, DaemonError>;
