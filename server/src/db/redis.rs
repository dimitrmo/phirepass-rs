use crate::db::common::NodeRecord;
use crate::env::Env;
use redis::{Commands, Connection};
use serde_json::json;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MemoryDB {
    connection: Arc<Mutex<Connection>>,
}

impl MemoryDB {
    pub async fn create(config: &Env) -> anyhow::Result<Self> {
        let client = redis::Client::open(config.redis_database_url.clone())?;
        let connection = client.get_connection()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub async fn set_node_connected(&self, node: &NodeRecord) -> anyhow::Result<()> {
        let key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let payload = node.to_json()?;
        let ttl_seconds = 120u64;

        let _: () = connection.set_ex(key, payload, ttl_seconds)?;

        Ok(())
    }

    pub async fn save_server(&self, id: &Uuid, ip: &str, port: u16) -> anyhow::Result<()> {
        let key = format!("phirepass:servers:{}", id);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let payload = json!({ "ip": ip.to_string(), "port": port, });
        let ttl_seconds = 120u64;

        let _: () = connection.set_ex(key, payload.to_string(), ttl_seconds)?;

        Ok(())
    }

    pub async fn update_node_stats(&self, node: &NodeRecord, stats: String) -> anyhow::Result<()> {
        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);
        let stats_key = format!("phirepass:users:{}:nodes:{}:stats", node.user_id, node.id);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let nodes_payload = node.to_json()?;
        let ttl_seconds = 120u64;

        let _: () = connection.set_ex(node_key, nodes_payload, ttl_seconds)?;
        let _: () = connection.set_ex(stats_key, stats, ttl_seconds)?;

        Ok(())
    }

    pub async fn set_node_disconnected(&self, node: &NodeRecord) -> anyhow::Result<()> {
        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);
        let stats_key = format!("phirepass:users:{}:nodes:{}:stats", node.user_id, node.id);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let _: () = connection.del(&node_key)?;
        let _: () = connection.del(&stats_key)?;

        Ok(())
    }
}
