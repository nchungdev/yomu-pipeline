//! Endpoint validation for user-configured provider URLs.
//!
//! SSRF WARNING: in server (BE) mode an endpoint must come from server configuration, never
//! from a client request. This check only blocks malformed and credential-bearing URLs and
//! cleartext HTTP to public hosts; it does NOT make an attacker-chosen HTTPS endpoint safe.

use crate::contract::v1::ProviderError;
use reqwest::Url;
use std::net::{Ipv4Addr, Ipv6Addr};

fn invalid() -> ProviderError {
    ProviderError::BadRequest("invalid endpoint".to_owned())
}

/// Parses and validates an endpoint. Rules:
/// - scheme `https`, or `http` only for loopback / private-LAN hosts (Komga and self-hosted LLM
///   servers on a home network rarely have a certificate);
/// - no userinfo (`user:pass@`), no query and no fragment (both can carry keys);
/// - a host is required.
///
/// The error never echoes the URL.
pub fn validate_endpoint(raw: &str) -> Result<Url, ProviderError> {
    let url = Url::parse(raw.trim()).map_err(|_| invalid())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    match url.scheme() {
        "https" if url.host_str().is_some_and(|h| !h.is_empty()) => Ok(url),
        "http" if is_local_host(&url) => Ok(url),
        _ => Err(invalid()),
    }
}

/// Validated endpoint as a string without a trailing slash.
pub(super) fn base_url(raw: &str) -> Result<String, ProviderError> {
    let url = validate_endpoint(raw)?;
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn is_local_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return is_lan_v4(ip);
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<Ipv6Addr>() {
        return is_lan_v6(ip);
    }
    is_lan_name(host)
}

fn is_lan_name(d: &str) -> bool {
    d == "localhost"
        || d.ends_with(".localhost")
        || d.ends_with(".local")
        || d.ends_with(".lan")
        || d.ends_with(".home.arpa")
        || !d.contains('.')
}

fn is_lan_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local()
}

fn is_lan_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
}
