use crate::db::common::NodeRecord;
use crate::env::Env;
use anyhow::Context;
use log::{debug, warn};
use phirepass_common::server::ServerIdentifier;
use redis::{Commands, Connection, RedisResult};
use serde_json::json;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct MemoryDB {
    client: redis::Client,
    connection: Arc<Mutex<Connection>>,
}

impl MemoryDB {
    pub fn create(config: &Env) -> anyhow::Result<Self> {
        let client = redis::Client::open(config.redis_database_url.clone())
            .context("failed to create a client")?;

        let connection = client
            .get_connection()
            .context("failed to get a connection")?;

        Ok(Self {
            client,
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn with_connection<T, F>(&self, mut op: F) -> anyhow::Result<T>
    where
        F: FnMut(&mut Connection) -> RedisResult<T>,
    {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        match op(&mut connection) {
            Ok(value) => return Ok(value),
            Err(err) if err.is_io_error() => {
                warn!("redis connection dropped, reconnecting");
            }
            Err(err) => return Err(err.into()),
        }

        drop(connection);

        let new_connection = self.client.get_connection()?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;
        *connection = new_connection;

        Ok(op(&mut connection)?)
    }

    pub fn set_node_connected(
        &self,
        node: &NodeRecord,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
        self.update_node_stats(node, server, String::from(""))
            .context("failed to set node connected by updating node stats")
    }

    pub fn save_server(&self, node_id: &Uuid, server_payload: &str) -> anyhow::Result<()> {
        let server_key = format!("phirepass:servers:{}", node_id);
        let fields_values = [("server", server_payload)];

        let _: () = self.with_connection(|connection| {
            let _: () = connection.hset_multiple(&server_key, &fields_values)?;
            connection.expire(&server_key, 120)
        })?;

        Ok(())
    }

    pub fn update_node_stats(
        &self,
        node: &NodeRecord,
        server: &Arc<ServerIdentifier>,
        stats_payload: String,
    ) -> anyhow::Result<()> {
        let node_payload = node.to_json()?;
        let server_payload = server.get_encoded()?;

        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);
        let fields_values = [
            ("node", node_payload),
            ("stats", stats_payload),
            ("server", server_payload),
        ];

        let _: () = self.with_connection(|connection| {
            let _: () = connection.hset_multiple(&node_key, &fields_values)?;
            connection.expire(&node_key, 120)
        })?;

        Ok(())
    }

    pub fn set_node_disconnected(&self, node: &NodeRecord) -> anyhow::Result<()> {
        let node_key = format!("phirepass:users:{}:nodes:{}", node.user_id, node.id);
        debug!("setting node disconnected by key {}", node_key);

        let _: () = self.with_connection(|connection| connection.del(&node_key))?;

        Ok(())
    }

    pub fn set_connection_connected(
        &self,
        cid: &Uuid,
        ip: IpAddr,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
        self.refresh_connection(cid, ip, server)
            .context("failed to set connection connected by refreshing the connection")
    }

    pub fn set_connection_disconnected(&self, cid: &Uuid) -> anyhow::Result<()> {
        let connection_key = format!("phirepass:connections:{}", cid);
        debug!("setting connection disconnected by key {}", connection_key);

        let _: () = self.with_connection(|connection| connection.del(&connection_key))?;

        Ok(())
    }

    pub fn refresh_connection(
        &self,
        cid: &Uuid,
        ip: IpAddr,
        server: &Arc<ServerIdentifier>,
    ) -> anyhow::Result<()> {
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

        let _: () = self.with_connection(|connection| {
            let _: () = connection.hset_multiple(&connection_key, &fields_values)?;
            connection.expire(&connection_key, 120)
        })?;

        Ok(())
    }
}
