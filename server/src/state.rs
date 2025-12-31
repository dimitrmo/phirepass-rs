use crate::connection::{NodeConnection, WebConnection};
use crate::env::Env;
use std::collections::HashMap;
use std::sync::Arc;
use ulid::Ulid;

type Nodes = Arc<tokio::sync::RwLock<HashMap<Ulid, NodeConnection>>>;

type Connections = Arc<tokio::sync::RwLock<HashMap<Ulid, WebConnection>>>;

type TunnelSessions = Arc<tokio::sync::RwLock<HashMap<String, (Ulid, Ulid)>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) env: Arc<Env>,
    pub(crate) nodes: Nodes,
    pub(crate) connections: Connections,
    pub(crate) tunnel_sessions: TunnelSessions,
}
