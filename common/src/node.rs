use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::SystemTime;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Node {
    pub connected_at: SystemTime,
    pub last_heartbeat: SystemTime,
    pub ip: IpAddr,
    pub last_stats: Option<Stats>,
}
