use phirepass_common::node::Node;
use phirepass_common::protocol::{Frame, NodeControlMessage};
use phirepass_common::stats::Stats;
use serde::Serialize;
use std::net::IpAddr;
use std::time::SystemTime;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone)]
pub(crate) struct WebConnection {
    pub(crate) node: Node,
    pub(crate) tx: UnboundedSender<Frame>,
}

impl WebConnection {
    pub(crate) fn new(ip: IpAddr, tx: UnboundedSender<Frame>) -> Self {
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
    pub(crate) tx: UnboundedSender<NodeControlMessage>,
}

impl NodeConnection {
    pub(crate) fn new(ip: IpAddr, tx: UnboundedSender<NodeControlMessage>) -> Self {
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
