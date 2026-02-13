use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ServerIdentifier {
    pub id: Uuid,
    pub private_ip: String,
    pub public_ip: String,
    pub port: u16,
    pub fqdn: String,
}

impl ServerIdentifier {
    pub fn get_encoded(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| anyhow::anyhow!(e))
    }

    pub fn get_decoded(raw: String) -> anyhow::Result<Self> {
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!(e))
    }
}
