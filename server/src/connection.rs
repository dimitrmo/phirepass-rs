use crate::db::common::NodeRecord;
use phirepass_common::node::Node;
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use serde::Serialize;
use serde_json::json;
use std::net::IpAddr;
use std::time::SystemTime;
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub(crate) struct WebConnection {
    pub(crate) connected_at: SystemTime,
    pub(crate) last_heartbeat: SystemTime,
    pub(crate) ip: IpAddr,
    pub(crate) tx: Sender<WebFrameData>,
}

impl WebConnection {
    pub(crate) fn new(ip: IpAddr, tx: Sender<WebFrameData>) -> Self {
        let now = SystemTime::now();

        Self {
            connected_at: now,
            last_heartbeat: now,
            ip,
            tx,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct NodeConnection {
    pub(crate) node: Node,
    #[serde(skip_serializing)]
    pub(crate) tx: Sender<NodeFrameData>,
    #[serde(skip_serializing)]
    pub(crate) node_record: NodeRecord,
}

impl NodeConnection {
    pub(crate) fn new(ip: IpAddr, tx: Sender<NodeFrameData>, node_record: NodeRecord) -> Self {
        let now = SystemTime::now();

        Self {
            node: Node {
                connected_at: now,
                last_heartbeat: now,
                ip,
                last_stats: None,
            },
            tx,
            node_record,
        }
    }

    pub fn get_extended_stats(&self) -> serde_json::Value {
        let now = SystemTime::now();

        let payload = json!({
            "id": self.node_record.id,
            "name": self.node_record.name,
            "ip": self.node.ip,
            "connected_for_secs": now
                .duration_since(self.node.connected_at)
                .unwrap()
                .as_secs(),
            "since_last_heartbeat_secs": now
                .duration_since(self.node.last_heartbeat)
                .unwrap()
                .as_secs(),
            "stats": &self.node.last_stats,
        });

        payload
    }
}
