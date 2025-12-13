use phirepass_common::node::Node;
use phirepass_common::protocol::{Frame, NodeControlMessage};
use phirepass_common::stats::Stats;
use serde::Serialize;
use std::net::IpAddr;
use std::time::SystemTime;
use tokio::sync::mpsc::Sender;

#[derive(Clone)]
pub(crate) struct WebConnection {
    pub(crate) node: Node,
    pub(crate) tx: Sender<Frame>,
}

impl WebConnection {
    pub(crate) fn new(ip: IpAddr, tx: Sender<Frame>) -> Self {
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

#[derive(Serialize)]
pub(crate) struct NodeConnection {
    pub(crate) node: Node,
    #[serde(skip_serializing)]
    pub(crate) tx: Sender<NodeControlMessage>,
}

impl NodeConnection {
    pub(crate) fn new(ip: IpAddr, tx: Sender<NodeControlMessage>) -> Self {
        let now = SystemTime::now();

        Self {
            node: Node {
                connected_at: now,
                last_heartbeat: now,
                ip,
                last_stats: Stats::gather(),
            },
            tx,
        }
    }
}
