use crate::env::Env;
use log::{debug, warn};
use redis::{Commands, Connection, RedisResult};
use std::sync::{Arc, Mutex};

pub struct MemoryDB {
    client: redis::Client,
    connection: Arc<Mutex<Connection>>,
}

impl MemoryDB {
    pub fn create(config: &Env) -> anyhow::Result<Self> {
        let client = redis::Client::open(config.redis_database_url.clone())?;
        let connection = client.get_connection()?;
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

    fn scan_keys(&self, key: &str) -> anyhow::Result<Vec<String>> {
        let keys = self.with_connection(|connection| {
            connection
                .scan_match(key)?
                .collect::<RedisResult<Vec<String>>>()
        })?;

        Ok(keys)
    }

    fn get_server(&self, key: &str) -> anyhow::Result<Option<String>> {
        let server: Option<String> =
            self.with_connection(|connection| connection.hget(key, "server"))?;
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
