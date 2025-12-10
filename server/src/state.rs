use crate::connection::{NodeConnection, WebConnection};
use crate::env::Env;
use std::collections::HashMap;
use std::sync::Arc;
use ulid::Ulid;

type Nodes = Arc<tokio::sync::Mutex<HashMap<Ulid, NodeConnection>>>;

type Clients = Arc<tokio::sync::Mutex<HashMap<Ulid, WebConnection>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) env: Arc<Env>,
    pub(crate) nodes: Nodes,
    pub(crate) clients: Clients,
}
