use crate::db::redis::MemoryDB;
use crate::env::Env;
use async_trait::async_trait;
use dashmap::DashMap;
use log::{debug, info, warn};
use phirepass_common::server::ServerIdentifier;
use pingora::prelude::*;
use pingora::proxy::{ProxyHttp, Session, http_proxy_service};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct CacheEntry {
    server: ServerIdentifier,
    cached_at: Instant,
}

struct WsProxy {
    upstream_servers: DashMap<String, CacheEntry>,
    memory_db: Arc<MemoryDB>,
}

struct RequestCtx {
    node_id: Option<String>,
}

fn extract_node_id(req: &RequestHeader) -> Option<String> {
    req.headers
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .split(',')
                .map(|part| part.trim())
                .find(|part| !part.is_empty())
        })
        .map(str::to_string)
}

impl WsProxy {
    fn get_server_by_id(&self, node_id: &str) -> anyhow::Result<ServerIdentifier> {
        debug!("searching for server by user node {}", node_id);

        if let Some(entry) = self.upstream_servers.get(node_id) {
            if entry.cached_at.elapsed() < Duration::from_secs(30) {
                debug!("server found in upstream cache: {}", entry.server.id);
                return Ok(entry.server.clone());
            }
            self.upstream_servers.remove(node_id);
        }

        let server = self.memory_db.get_user_server_by_node_id(node_id)?;
        let server = ServerIdentifier::get_decoded(server)?;

        debug!("server found: {}", server.id);

        self.upstream_servers.insert(
            node_id.to_string(),
            CacheEntry {
                server: server.clone(),
                cached_at: Instant::now(),
            },
        );

        Ok(server)
    }
}

#[async_trait]
impl ProxyHttp for WsProxy {
    type CTX = RequestCtx;

    fn new_ctx(&self) -> Self::CTX {
        RequestCtx { node_id: None }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> Result<Box<HttpPeer>> {
        let Some(node_id) = ctx.node_id.as_deref() else {
            warn!("node_id missing before upstream selection");
            return Err(Error::new(HTTPStatus(400)));
        };

        info!("proxying request for node_id {}", node_id);

        let server_with_node = self.get_server_by_id(node_id);
        let server = match server_with_node {
            Ok(server) => server,
            Err(err) => {
                warn!("node could not be found: {err}");
                return Err(Error::new(HTTPStatus(400)));
            }
        };

        info!("proxying request to server {}", server.id);

        let peer = HttpPeer::new((server.private_ip, server.port), false, server.fqdn);
        debug!("proxying request for peer {}", peer);

        Ok(Box::new(peer))
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> Result<bool> {
        let req = session.req_header();
        let node_id = extract_node_id(req);

        if node_id.is_none() {
            warn!("sec-websocket-protocol missing or empty");
            session.respond_error(400).await?;
            return Ok(true);
        }

        ctx.node_id = node_id;
        debug!("node_id is {:?}", ctx.node_id);

        Ok(false)
    }
}

pub fn start(config: Env) -> anyhow::Result<()> {
    info!("running server on {} mode", config.mode);

    let memory_db = MemoryDB::create(&config)?;
    info!("connected to valkey");

    let bind_addr = format!("{}:{}", config.host, config.port);
    info!("running proxy on {}", bind_addr);

    let mut server = Server::new(None)?;
    server.bootstrap();

    let proxy = WsProxy {
        upstream_servers: DashMap::new(),
        memory_db: Arc::new(memory_db),
    };
    let mut service = http_proxy_service(&server.configuration, proxy);
    service.add_tcp(&bind_addr);
    info!("proxy prepared");

    server.add_service(service);
    info!("proxy running forever");

    server.run_forever();
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;
    use http::header::HeaderValue;

    #[test]
    fn extract_node_id_none_when_missing_header() {
        let req = RequestHeader::build(Method::GET, b"/", None).unwrap();
        assert!(extract_node_id(&req).is_none());
    }

    #[test]
    fn extract_node_id_none_when_empty_header() {
        let mut req = RequestHeader::build(Method::GET, b"/", None).unwrap();
        req.headers
            .insert("sec-websocket-protocol", HeaderValue::from_static(""));
        assert!(extract_node_id(&req).is_none());
    }

    #[test]
    fn extract_node_id_first_non_empty_token() {
        let mut req = RequestHeader::build(Method::GET, b"/", None).unwrap();
        req.headers.insert(
            "sec-websocket-protocol",
            HeaderValue::from_static(" , node-1 , node-2"),
        );
        assert_eq!(extract_node_id(&req), Some("node-1".to_string()));
    }
}
