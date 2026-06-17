// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Shared node-address normalization helpers.
//!
//! Node registration, seeded-node connectivity, and remote service dialing all need the
//! same understanding of node address identity. In local deployments the same endpoint
//! often appears as `localhost`, `127.0.0.1`, or `0.0.0.0`, with or without a scheme.
//! These helpers collapse those aliases for identity comparison and also provide a
//! dialable form for client connections.

fn canonicalize_host(host: &str) -> String {
    match host {
        "localhost" | "127.0.0.1" | "0.0.0.0" | "::1" | "[::1]" | "[::]" => "loopback".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn split_host_port(address: &str) -> (String, String) {
    let trimmed = address
        .strip_prefix("http://")
        .or_else(|| address.strip_prefix("https://"))
        .unwrap_or(address)
        .split('/')
        .next()
        .unwrap_or(address);

    if let Some(stripped) = trimmed.strip_prefix('[') {
        if let Some((host, remainder)) = stripped.split_once(']') {
            let port = remainder.strip_prefix(':').unwrap_or_default().to_string();
            return (canonicalize_host(&format!("[{}]", host)), port);
        }
    }

    if let Some((host, port)) = trimmed.rsplit_once(':') {
        return (canonicalize_host(host), port.to_string());
    }

    (canonicalize_host(trimmed), String::new())
}

/// Build a stable comparison key for a node address.
pub fn canonical_node_address_key(address: &str) -> String {
    let (host, port) = split_host_port(address);
    if port.is_empty() {
        host
    } else {
        format!("{}:{}", host, port)
    }
}

/// Returns true when two node addresses refer to the same effective endpoint.
pub fn node_addresses_equivalent(left: &str, right: &str) -> bool {
    canonical_node_address_key(left) == canonical_node_address_key(right)
}

/// Normalize a node address into a dialable HTTP endpoint.
///
/// Bind addresses like `0.0.0.0` or `[::]` are valid for listeners but not for clients.
/// For loopback aliases we canonicalize to `localhost` and always return an explicit
/// `http://` endpoint. The gRPC connection manager applies mTLS at the transport layer
/// (via `ClientTlsConfig`) independently of the URL scheme — the server listens on
/// plain TCP in single-port mode.
pub fn dialable_node_address(address: &str) -> String {
    let trimmed = address.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed);

    let rewritten = if let Some(rest) = without_scheme.strip_prefix("0.0.0.0") {
        format!("localhost{}", rest)
    } else if let Some(rest) = without_scheme.strip_prefix("[::]") {
        format!("localhost{}", rest)
    } else if let Some(rest) = without_scheme.strip_prefix("[::1]") {
        format!("localhost{}", rest)
    } else if let Some(rest) = without_scheme.strip_prefix("::1") {
        format!("localhost{}", rest)
    } else if let Some(rest) = without_scheme.strip_prefix("::") {
        format!("localhost{}", rest)
    } else if let Some(rest) = without_scheme.strip_prefix("127.0.0.1") {
        format!("localhost{}", rest)
    } else {
        without_scheme.to_string()
    };

    format!("http://{}", rewritten)
}

#[cfg(test)]
mod tests {
    use super::{canonical_node_address_key, dialable_node_address, node_addresses_equivalent};

    #[test]
    fn canonical_key_normalizes_loopback_aliases() {
        assert_eq!(
            canonical_node_address_key("localhost:8091"),
            canonical_node_address_key("http://127.0.0.1:8091")
        );
        assert_eq!(
            canonical_node_address_key("0.0.0.0:8091"),
            canonical_node_address_key("http://localhost:8091")
        );
    }

    #[test]
    fn address_equivalence_matches_loopback_variants() {
        assert!(node_addresses_equivalent(
            "localhost:8091",
            "http://0.0.0.0:8091"
        ));
        assert!(node_addresses_equivalent(
            "http://127.0.0.1:8093",
            "0.0.0.0:8093"
        ));
        assert!(!node_addresses_equivalent(
            "http://127.0.0.1:8093",
            "http://127.0.0.1:8094"
        ));
    }

    #[test]
    fn dialable_address_rewrites_loopback_bind_hosts() {
        assert_eq!(
            dialable_node_address("http://0.0.0.0:8093"),
            "http://localhost:8093"
        );
        assert_eq!(
            dialable_node_address("127.0.0.1:8094"),
            "http://localhost:8094"
        );
        assert_eq!(
            dialable_node_address("https://node.example:8095"),
            "http://node.example:8095"
        );
    }
}
