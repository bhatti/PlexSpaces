// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! gRPC Connection Manager with Connection Pooling
//!
//! ## Purpose
//! Manages gRPC client connections to remote nodes with connection pooling.
//! Each service type has its own pool per node, bounded by `pool_size`.
//!
//! ## Design
//! - Channels are lazy-connected (no TCP until first RPC call).
//! - Tonic `Channel` is cheaply cloneable; all clones share the same HTTP/2 connection,
//!   so concurrent scatter-gather bursts never open extra file descriptors.
//! - The pool grows up to `pool_size` distinct channels per (ServiceType, node_id).
//!   Round-robin checkout spreads HTTP/2 stream load across the pool.
//! - File-descriptor usage is strictly bounded: at most
//!   `pool_size × num_service_types × num_nodes` TCP connections system-wide.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Service type identifier for gRPC connection pooling.
/// Re-exported from proto so the canonical definition lives in one place.
pub use plexspaces_proto::services::prv::ServiceName as ServiceType;

/// Connection pool for a specific service type and node.
///
/// Each entry is a lazy-connected tonic `Channel`. Cloning a `Channel` is free and
/// multiplexes over the same HTTP/2 connection — no new TCP socket or FD is opened.
/// The pool grows to `pool_size` on demand; callers always receive a clone.
struct ServiceConnectionPool {
    channels: Vec<Channel>,
    pool_size: usize,
    node_address: String,
}

impl ServiceConnectionPool {
    fn new(node_address: String, pool_size: usize) -> Self {
        Self {
            channels: Vec::with_capacity(pool_size.max(1)),
            pool_size: pool_size.max(1),
            node_address,
        }
    }

    /// Return a clone of a pooled channel, adding a new lazy channel if pool has room.
    fn get(&mut self) -> Result<Channel, tonic::transport::Error> {
        self.get_with_tls(None)
    }

    /// Return a channel with optional mTLS. When `tls` is provided the address
    /// must use https:// scheme; otherwise http:// is used automatically.
    fn get_with_tls(
        &mut self,
        tls: Option<&ClientTlsConfig>,
    ) -> Result<Channel, tonic::transport::Error> {
        if self.channels.len() < self.pool_size {
            // Only use https:// if the address already uses it or if TLS is configured
            // AND the address doesn't explicitly use http://. This prevents connecting
            // to plain-HTTP nodes with TLS (which causes InvalidContentType errors).
            let addr = match tls {
                Some(_) if !self.node_address.starts_with("http://") => {
                    self.node_address
                        .replace("grpc://", "grpcs://")
                        .replace("https://", "https://") // keep as-is
                }
                _ => self.node_address.clone(),
            };
            let endpoint = Endpoint::from_shared(addr)?;
            // Only apply TLS config if the server is expected to support it (https:// scheme).
            // For http:// nodes (plain gRPC), always connect without TLS regardless of config.
            let channel = if tls.is_some() && !self.node_address.starts_with("http://") {
                endpoint.tls_config(tls.unwrap().clone())?.connect_lazy()
            } else {
                endpoint.connect_lazy()
            };
            self.channels.push(channel);
        }
        self.channels.rotate_left(1);
        Ok(self.channels[self.channels.len() - 1].clone())
    }
}

/// gRPC Connection Manager with bounded lazy connection pooling.
pub struct GrpcConnectionManager {
    pools: Arc<RwLock<HashMap<(ServiceType, String), ServiceConnectionPool>>>,
    pool_size: usize,
    /// Optional mTLS config for node-to-node connections.
    tls_config: Option<ClientTlsConfig>,
}

impl GrpcConnectionManager {
    /// Create a new connection manager without mTLS.
    pub fn new(pool_size: Option<u32>) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            pool_size: pool_size.unwrap_or(2).max(1) as usize,
            tls_config: None,
        }
    }

    /// Configure mTLS for node-to-node connections using cert/key/CA files.
    /// Returns error if any file cannot be read.
    pub fn with_mtls(
        mut self,
        ca_cert_path: &str,
        client_cert_path: &str,
        client_key_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Install the ring crypto provider for rustls if not already installed.
        // rustls requires a CryptoProvider before any TLS config is built.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let ca_pem = std::fs::read(ca_cert_path)
            .map_err(|e| format!("Cannot read CA cert {}: {}", ca_cert_path, e))?;
        let client_cert_pem = std::fs::read(client_cert_path)
            .map_err(|e| format!("Cannot read client cert {}: {}", client_cert_path, e))?;
        let client_key_pem = std::fs::read(client_key_path)
            .map_err(|e| format!("Cannot read client key {}: {}", client_key_path, e))?;

        let ca = Certificate::from_pem(ca_pem);
        let identity = Identity::from_pem(client_cert_pem, client_key_pem);
        self.tls_config = Some(
            ClientTlsConfig::new()
                .ca_certificate(ca)
                .identity(identity)
                .domain_name("plexspaces.local"),
        );
        Ok(self)
    }

    /// Check whether mTLS is configured.
    pub fn has_mtls(&self) -> bool {
        self.tls_config.is_some()
    }

    /// Get a channel for the given service type and node.
    pub async fn get_connection(
        &self,
        service_type: ServiceType,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        let key = (service_type, node_id.to_string());
        let mut pools = self.pools.write().await;
        let pool = pools.entry(key).or_insert_with(|| {
            ServiceConnectionPool::new(node_address.to_string(), self.pool_size)
        });
        if pool.node_address != node_address {
            *pool = ServiceConnectionPool::new(node_address.to_string(), self.pool_size);
        }
        pool.get_with_tls(self.tls_config.as_ref())
    }

    /// Get a channel for the ActorService on the given node.
    pub async fn get_actor_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ServiceNameActorService, node_id, node_address)
            .await
    }

    /// Get a channel for the ApplicationService on the given node.
    pub async fn get_application_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameApplicationService,
            node_id,
            node_address,
        )
        .await
    }

    /// Get a channel for the TupleSpaceService on the given node.
    pub async fn get_tuplespace_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameTuplespaceService,
            node_id,
            node_address,
        )
        .await
    }

    /// Get a channel for the ProcessGroupService on the given node.
    pub async fn get_process_group_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameProcessGroupService,
            node_id,
            node_address,
        )
        .await
    }

    /// Get a channel for the NodeService on the given node.
    pub async fn get_node_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ServiceNameNodeService, node_id, node_address)
            .await
    }

    /// Get a channel for the KeyValueService on the given node.
    pub async fn get_key_value_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ServiceNameKeyValueService, node_id, node_address)
            .await
    }

    /// Get a channel for the BlobService on the given node.
    pub async fn get_blob_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ServiceNameBlobService, node_id, node_address)
            .await
    }

    /// Get a channel for the ServiceLinkService on the given node.
    pub async fn get_service_link_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameServiceLinkService,
            node_id,
            node_address,
        )
        .await
    }

    /// Get a channel for the MetricsService on the given node.
    pub async fn get_metrics_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameMetricsService,
            node_id,
            node_address,
        )
        .await
    }

    /// Get a channel for the ObjectRegistry service on the given node.
    pub async fn get_object_registry_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(
            ServiceType::ServiceNameObjectRegistry,
            node_id,
            node_address,
        )
        .await
    }

    /// Drop all pooled channels and close all connections.
    pub async fn shutdown(&self) {
        self.pools.write().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn updates_pool_address_when_node_endpoint_changes() {
        let manager = GrpcConnectionManager::new(Some(2));

        {
            let mut pools = manager.pools.write().await;
            pools.insert(
                (ServiceType::ServiceNameActorService, "node-a".to_string()),
                ServiceConnectionPool::new("http://0.0.0.0:8093".to_string(), 2),
            );
        }

        let _ = manager
            .get_connection(
                ServiceType::ServiceNameActorService,
                "node-a",
                "http://localhost:8093",
            )
            .await;

        let pools = manager.pools.read().await;
        let pool = pools
            .get(&(ServiceType::ServiceNameActorService, "node-a".to_string()))
            .expect("pool should exist");
        assert_eq!(pool.node_address, "http://localhost:8093");
    }

    #[tokio::test]
    async fn pool_grows_to_pool_size_and_rotates() {
        let manager = GrpcConnectionManager::new(Some(2));
        for _ in 0..4 {
            let _ = manager
                .get_connection(
                    ServiceType::ServiceNameActorService,
                    "node-b",
                    "http://localhost:8093",
                )
                .await;
        }
        let pools = manager.pools.read().await;
        let pool = pools
            .get(&(ServiceType::ServiceNameActorService, "node-b".to_string()))
            .expect("pool should exist");
        assert_eq!(pool.channels.len(), 2, "pool must not exceed pool_size");
    }

    #[tokio::test]
    async fn new_service_type_helpers_use_distinct_pools() {
        // Each service type gets its own pool entry per node — verified by counting
        // distinct (ServiceType, node_id) keys after calling each typed helper.
        let manager = GrpcConnectionManager::new(Some(1));
        let addr = "http://localhost:9999";
        let node = "node-c";
        let _ = manager.get_key_value_service_connection(node, addr).await;
        let _ = manager.get_blob_service_connection(node, addr).await;
        let _ = manager.get_service_link_service_connection(node, addr).await;
        let _ = manager.get_metrics_service_connection(node, addr).await;
        let _ = manager.get_object_registry_service_connection(node, addr).await;
        let pools = manager.pools.read().await;
        // Each typed helper must have created exactly one pool entry
        assert!(
            pools.contains_key(&(ServiceType::ServiceNameKeyValueService, node.to_string())),
            "KeyValue pool missing"
        );
        assert!(
            pools.contains_key(&(ServiceType::ServiceNameBlobService, node.to_string())),
            "Blob pool missing"
        );
        assert!(
            pools.contains_key(&(ServiceType::ServiceNameServiceLinkService, node.to_string())),
            "ServiceLink pool missing"
        );
        assert!(
            pools.contains_key(&(ServiceType::ServiceNameMetricsService, node.to_string())),
            "Metrics pool missing"
        );
        assert!(
            pools.contains_key(&(ServiceType::ServiceNameObjectRegistry, node.to_string())),
            "ObjectRegistry pool missing"
        );
    }
}
