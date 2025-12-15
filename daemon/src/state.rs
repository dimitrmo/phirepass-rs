use crate::env::Env;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) env: Arc<Env>,
}
