use crate::db::common::NodeRecord;
use crate::env::Env;
use phirepass_common::server::ServerIdentifier;
use redis::{Commands, Connection};
use serde_json::json;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MemoryDB {
    connection: Arc<Mutex<Connection>>,
}

impl MemoryDB {
    pub fn create(config: &Env) -> anyhow::Result<Self> {
        let client = redis::Client::open(config.redis_database_url.clone())?;
        let connection = client.get_connection()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn set_node_connected(
        &self,
        node: &NodeRecord,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
        self.update_node_stats(node, server, String::from(""))
    }

    pub fn save_server(&self, node_id: &Uuid, server_payload: &str) -> anyhow::Result<()> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let server_key = format!("phirepass:servers:{}", node_id);
        let fields_values = [("server", server_payload)];

        let _: () = connection.hset_multiple(&server_key, &fields_values)?;
        let _: () = connection.expire(&server_key, 120)?;

        Ok(())
    }

    pub fn update_node_stats(
        &self,
        node: &NodeRecord,
        server: &Arc<ServerIdentifier>,
        stats: String,
    ) -> anyhow::Result<()> {
        let node_payload = node.to_json()?;
        let server_payload = server.get_encoded()?;

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);
        let fields_values = [
            ("node", node_payload),
            ("stats", stats),
            ("server", server_payload),
        ];

        let _: () = connection.hset_multiple(&node_key, &fields_values)?;
        let _: () = connection.expire(&node_key, 120)?;

        Ok(())
    }

    pub fn set_node_disconnected(&self, node: &NodeRecord) -> anyhow::Result<()> {
        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let _: () = connection.del(&node_key)?;

        Ok(())
    }

    pub fn set_connection_connected(
        &self,
        cid: &Uuid,
        ip: IpAddr,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
        self.refresh_connection(cid, ip, server)
    }

    pub fn set_connection_disconnected(&self, cid: &Uuid) -> anyhow::Result<()> {
        let connection_key = format!("phirepass:connections:{}", cid);

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let _: () = connection.del(&connection_key)?;

        Ok(())
    }

    pub fn refresh_connection(
        &self,
        cid: &Uuid,
        ip: IpAddr,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let server_payload = server.get_encoded()?;

        let connection_key = format!("phirepass:connections:{}", cid);
        let connection_data = json!({
            "id": cid.to_string(), // cid
            "ip": ip.to_string(),
        })
        .to_string();

        let fields_values = [
            ("connection", connection_data.as_str()),
            ("server", server_payload.as_str()),
        ];

        let _: () = connection.hset_multiple(&connection_key, &fields_values)?;
        let _: () = connection.expire(&connection_key, 120)?;

        Ok(())
    }
}
