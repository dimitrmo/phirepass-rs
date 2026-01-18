use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;

#[derive(Debug, Error)]
struct ServerMessageError(pub String);

impl Display for ServerMessageError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{0}", self.0)
    }
}

impl From<ServerMessageError> for ServerError {
    fn from(message: ServerMessageError) -> Self {
        ServerError::Other(Box::new(message))
    }
}

impl From<String> for ServerError {
    fn from(message: String) -> Self {
        ServerError::Other(Box::new(ServerMessageError(message)))
    }
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}
