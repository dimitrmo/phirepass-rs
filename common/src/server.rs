use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ServerIdentifier {
    pub private_ip: String,
    pub public_ip: String,
    pub port: u16,
    pub fqdn: String,
}

impl ServerIdentifier {
    pub fn get_encoded(&self) -> anyhow::Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| anyhow::anyhow!(e))
    }
}
