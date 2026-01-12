use phirepass_common::node::Node;
use phirepass_common::protocol::node::NodeFrameData;
use phirepass_common::protocol::web::WebFrameData;
use serde::Serialize;
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
}

impl NodeConnection {
    pub(crate) fn new(ip: IpAddr, tx: Sender<NodeFrameData>) -> Self {
        let now = SystemTime::now();

        Self {
            node: Node {
                connected_at: now,
                last_heartbeat: now,
                ip,
                last_stats: None,
            },
            tx,
        }
    }
}
