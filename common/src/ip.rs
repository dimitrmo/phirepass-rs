use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

static X_ENVOY_EXTERNAL_ADDRESS: &str = "x-envoy-external-address";

fn envoy_external_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let raw = headers.get(X_ENVOY_EXTERNAL_ADDRESS)?.to_str().ok()?.trim();
    let first = raw.split(',').next()?.trim();
    IpAddr::from_str(first)
        .or_else(|_| SocketAddr::from_str(first).map(|sa| sa.ip()))
        .ok()
}

pub fn extract_ip_from_headers(headers: &HeaderMap) -> Option<IpAddr> {
    envoy_external_ip(headers)
}
