use crate::env::Env;
use log::{debug, warn};
use redis::{Commands, Connection, RedisResult};
use std::sync::{Arc, Mutex};

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

    fn scan_keys(&self, key: &str) -> anyhow::Result<Vec<String>> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;

        let keys = connection
            .scan_match(key)?
            .collect::<RedisResult<Vec<String>>>()?;

        Ok(keys)
    }

    fn get_server(&self, key: &str) -> anyhow::Result<Option<String>> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| anyhow::anyhow!("redis connection lock poisoned"))?;
        let server: Option<String> = connection.hget(key, "server")?;
        Ok(server)
    }

    pub fn get_user_server_by_node_id(&self, node_id: &str) -> anyhow::Result<String> {
        let key = format!("phirepass:users:*:nodes:{}", node_id);
        debug!("scan by key: {}", key);

        let keys = self.scan_keys(&key)?;
        if keys.is_empty() {
            warn!("no entries found for key {}", key);
            anyhow::bail!("server not found for key")
        }

        let id = &keys[0];
        let server = self.get_server(id)?;
        let Some(server) = server else {
            warn!("server not found for id {}", id);
            anyhow::bail!("server not found for node {}", node_id)
        };

        Ok(server)
    }
}
