use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use stunclient::StunClient;

const DEFAULT_SERVERS: [&str; 5] = [
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun2.l.google.com:19302",
    "stun3.l.google.com:19302",
    "stun4.l.google.com:19302",
];

const DEFAULT_TIMEOUT_SECS: u64 = 3;

fn servers_from_env() -> Vec<String> {
    match std::env::var("STUN_SERVERS") {
        Ok(value) => value
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => DEFAULT_SERVERS.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn resolve_server(server: &str) -> Option<SocketAddr> {
    server
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
}

pub(crate) fn get_public_address() -> Result<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").context("bind UDP socket")?;
    socket
        .set_read_timeout(Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)))
        .context("set UDP read timeout")?;

    let mut last_error: Option<String> = None;

    for server in servers_from_env() {
        let addr = match resolve_server(&server) {
            Some(addr) => addr,
            None => {
                last_error = Some(format!("cannot resolve {}", server));
                continue;
            }
        };

        let client = StunClient::new(addr);
        match client.query_external_address(&socket) {
            Ok(mapped) => return Ok(mapped.ip().to_string()),
            Err(err) => last_error = Some(format!("{} failed: {}", server, err)),
        }

        thread::sleep(Duration::from_secs(1));
    }

    Err(anyhow!(last_error.unwrap_or_else(|| {
        "no STUN servers configured".to_string()
    })))
}
