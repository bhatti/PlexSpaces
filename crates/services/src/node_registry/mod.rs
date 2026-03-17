// SPDX-License-Identifier: LGPL-2.1-or-later
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

//! NodeRegistry - Robust Node Discovery with SWIM Protocol
//!
//! ## Overview
//! NodeRegistry provides reliable node discovery using a combination of:
//! - **SWIM Protocol**: Scalable failure detection without shared database
//! - **TTL Cache**: Efficient local caching with configurable expiration
//! - **DB Fallback**: Optional persistence with exponential backoff + jitter
//!
//! ## Design Principles
//! - **Composition**: Wraps ObjectRegistry, uses SWIM protocol
//! - **Resilient**: Exponential backoff with decorrelated jitter for DB operations
//! - **Scalable**: SWIM provides O(log n) convergence for membership changes
//! - **Robust**: Works reliably with or without shared database

pub mod swim;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use plexspaces_core::{NodeRegistryTrait, ObjectRegistry, RequestContext, ServiceLocator};
use plexspaces_proto::common::v1::Metadata as CommonMetadata;
use plexspaces_proto::node::v1::{NodeCapacity, NodeRegistration, PingResponse};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost_types::Timestamp;

pub use swim::{ExponentialBackoff, NodeState, SwimConfig, SwimMember, SwimProtocol};

use crate::node_address::canonical_node_address_key;

/// Default cache TTL in seconds
const DEFAULT_CACHE_TTL_SECONDS: u64 = 60;

/// Default gossip interval in milliseconds
const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 1000;

/// Default gossip fanout (number of nodes to probe per round)
const DEFAULT_GOSSIP_FANOUT: usize = 3;
const UNKNOWN_NODE_ID_PREFIX: &str = "_unknown_";
const MAX_PROBE_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_ACTIVE_NODE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);

/// Cached node registration with expiry
struct CachedNodeRegistration {
    registration: NodeRegistration,
    cached_at: Instant,
    expires_at: Instant,
}

impl CachedNodeRegistration {
    fn new(registration: NodeRegistration, ttl: Duration) -> Self {
        let now = Instant::now();
        Self {
            registration,
            cached_at: now,
            expires_at: now + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

/// NodeRegistry configuration
#[derive(Debug, Clone)]
pub struct NodeRegistryConfig {
    /// Cache TTL for node entries
    pub cache_ttl: Duration,
    /// Enable SWIM gossip protocol
    pub gossip_enabled: bool,
    /// SWIM protocol configuration
    pub swim_config: SwimConfig,
    /// Use shared database as source of truth
    pub use_shared_db: bool,
    /// DB operation backoff base delay
    pub db_backoff_base: Duration,
    /// DB operation backoff max delay
    pub db_backoff_cap: Duration,
    /// DB operation max retry attempts
    pub db_max_attempts: u32,
    /// Anti-entropy sync with DB interval
    pub db_sync_interval: Duration,
    /// Maximum age for registrations considered active
    pub active_node_window: Duration,
}

impl Default for NodeRegistryConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(DEFAULT_CACHE_TTL_SECONDS),
            gossip_enabled: true,
            swim_config: SwimConfig::default(),
            use_shared_db: false,
            db_backoff_base: Duration::from_millis(100),
            db_backoff_cap: Duration::from_secs(30),
            db_max_attempts: 10,
            db_sync_interval: Duration::from_secs(30),
            active_node_window: DEFAULT_ACTIVE_NODE_WINDOW,
        }
    }
}

/// NodeRegistry implementation with SWIM protocol and DB fallback
pub struct NodeRegistry {
    /// Underlying ObjectRegistry for persistence (optional)
    object_registry: Arc<dyn ObjectRegistry>,
    /// TTL-based cache: node_id -> CachedNodeRegistration
    cache: Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
    /// Configuration
    config: NodeRegistryConfig,
    /// SWIM protocol instance
    swim: Arc<SwimProtocol>,
    /// Protocol running flag (Arc for sharing with spawned tasks)
    running: Arc<AtomicBool>,
    /// Local node ID
    local_node_id: String,
    /// Cache hits counter (for observability)
    cache_hits: AtomicU64,
    /// Cache misses counter (for observability)
    cache_misses: AtomicU64,
    /// DB operation failures counter
    db_failures: AtomicU64,
    /// ServiceLocator for accessing other services
    service_locator: Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
}

impl NodeRegistry {
    /// Create a new NodeRegistry
    pub fn new(
        object_registry: Arc<dyn ObjectRegistry>,
        local_node_id: String,
        local_address: String,
        config: NodeRegistryConfig,
    ) -> Self {
        let swim = Arc::new(SwimProtocol::new(
            local_node_id.clone(),
            local_address,
            config.swim_config.clone(),
        ));

        Self {
            object_registry,
            cache: Arc::new(RwLock::new(HashMap::new())),
            config,
            swim,
            running: Arc::new(AtomicBool::new(false)),
            local_node_id,
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            db_failures: AtomicU64::new(0),
            service_locator: Arc::new(RwLock::new(None)),
        }
    }

    /// Create with simple parameters (for backward compatibility)
    pub fn new_simple(
        object_registry: Arc<dyn ObjectRegistry>,
        local_node_id: String,
        cache_ttl_seconds: Option<u64>,
        gossip_enabled: Option<bool>,
        gossip_interval_ms: Option<u64>,
        gossip_fanout: Option<usize>,
    ) -> Self {
        let mut config = NodeRegistryConfig::default();
        config.cache_ttl =
            Duration::from_secs(cache_ttl_seconds.unwrap_or(DEFAULT_CACHE_TTL_SECONDS));
        config.gossip_enabled = gossip_enabled.unwrap_or(true);
        config.swim_config.protocol_period =
            Duration::from_millis(gossip_interval_ms.unwrap_or(DEFAULT_GOSSIP_INTERVAL_MS));
        config.swim_config.indirect_ping_nodes = gossip_fanout.unwrap_or(DEFAULT_GOSSIP_FANOUT);

        Self::new(
            object_registry,
            local_node_id.clone(),
            String::new(), // Address will be set later
            config,
        )
    }

    /// Create NodeRegistry from NodeConfig proto
    ///
    /// Reads configuration from the proper proto fields in NodeConfig.node_registry
    /// with sensible defaults if not specified.
    pub fn from_config(
        object_registry: Arc<dyn ObjectRegistry>,
        node_config: &plexspaces_proto::node::v1::NodeConfig,
    ) -> Self {
        let config = Self::config_from_proto(node_config.node_registry.as_ref());
        let grpc_address = if node_config.grpc_address.is_empty() {
            node_config.listen_addr.clone()
        } else {
            node_config.grpc_address.clone()
        };

        Self::new(
            object_registry,
            node_config.id.clone(),
            grpc_address,
            config,
        )
    }

    /// Create NodeRegistryConfig from proto config
    ///
    /// Maps proto NodeRegistryConfig to internal NodeRegistryConfig struct.
    /// Uses default values for any fields not specified.
    pub fn config_from_proto(
        proto_config: Option<&plexspaces_proto::node::v1::NodeRegistryConfig>,
    ) -> NodeRegistryConfig {
        let mut config = NodeRegistryConfig::default();

        if let Some(proto) = proto_config {
            // Cache configuration
            if proto.cache_ttl_seconds > 0 {
                config.cache_ttl = Duration::from_secs(proto.cache_ttl_seconds as u64);
            }

            // Gossip configuration
            config.gossip_enabled = proto.gossip_enabled;

            // SWIM configuration
            if let Some(swim_proto) = &proto.swim {
                config.swim_config = Self::swim_config_from_proto(swim_proto);
            }

            // DB fallback configuration
            config.use_shared_db = proto.use_shared_db;

            if proto.db_sync_interval_seconds > 0 {
                config.db_sync_interval =
                    Duration::from_secs(proto.db_sync_interval_seconds as u64);
            }

            if proto.active_node_window_seconds > 0 {
                config.active_node_window =
                    Duration::from_secs(proto.active_node_window_seconds as u64);
            }

            // DB backoff configuration
            if let Some(backoff_proto) = &proto.db_backoff {
                if backoff_proto.base_delay_ms > 0 {
                    config.db_backoff_base =
                        Duration::from_millis(backoff_proto.base_delay_ms as u64);
                }
                if backoff_proto.max_delay_ms > 0 {
                    config.db_backoff_cap =
                        Duration::from_millis(backoff_proto.max_delay_ms as u64);
                }
                if backoff_proto.max_attempts > 0 {
                    config.db_max_attempts = backoff_proto.max_attempts;
                }
            }
        }

        config
    }

    /// Create SwimConfig from proto config
    fn swim_config_from_proto(proto: &plexspaces_proto::node::v1::SwimConfig) -> SwimConfig {
        let mut config = SwimConfig::default();

        if proto.protocol_period_ms > 0 {
            config.protocol_period = Duration::from_millis(proto.protocol_period_ms as u64);
        }
        if proto.probe_timeout_ms > 0 {
            config.probe_timeout = Duration::from_millis(proto.probe_timeout_ms as u64);
        }
        if proto.indirect_ping_nodes > 0 {
            config.indirect_ping_nodes = proto.indirect_ping_nodes as usize;
        }
        if proto.suspicion_multiplier > 0 {
            config.suspicion_mult = proto.suspicion_multiplier;
        }
        if proto.suspicion_min_ms > 0 {
            config.suspicion_min = Duration::from_millis(proto.suspicion_min_ms as u64);
        }
        if proto.suspicion_max_ms > 0 {
            config.suspicion_max = Duration::from_millis(proto.suspicion_max_ms as u64);
        }
        if proto.dead_node_reap_seconds > 0 {
            config.dead_node_reap_timeout =
                Duration::from_secs(proto.dead_node_reap_seconds as u64);
        }
        if proto.max_piggyback_updates > 0 {
            config.max_piggyback_updates = proto.max_piggyback_updates as usize;
        }
        if proto.broadcast_limit > 0 {
            config.broadcast_limit = proto.broadcast_limit;
        }
        if proto.anti_entropy_interval_seconds > 0 {
            config.anti_entropy_interval =
                Duration::from_secs(proto.anti_entropy_interval_seconds as u64);
        }

        config
    }

    /// Set ServiceLocator (needed for gossip protocol gRPC communication)
    pub async fn set_service_locator(&self, service_locator: Arc<dyn ServiceLocator>) {
        let mut sl = self.service_locator.write().await;
        *sl = Some(service_locator);
    }

    /// Get SWIM protocol reference
    pub fn swim(&self) -> &Arc<SwimProtocol> {
        &self.swim
    }

    /// Convert ObjectRegistration to NodeRegistration
    fn to_node_registration(obj_reg: &ObjectRegistration) -> NodeRegistration {
        // Node placement depends on the original label values (for example cluster=heat),
        // so reconstruct capabilities from metadata labels first and only use the bare
        // capability list as a fallback for presence-only flags.
        let mut capabilities = obj_reg
            .metadata
            .as_ref()
            .map(|metadata| metadata.labels.clone())
            .unwrap_or_default();
        for capability in &obj_reg.capabilities {
            capabilities
                .entry(capability.clone())
                .or_insert_with(|| "true".to_string());
        }

        NodeRegistration {
            node_id: obj_reg.object_id.clone(),
            node_address: obj_reg.grpc_address.clone(),
            capabilities,
            status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
            last_heartbeat: obj_reg.last_heartbeat.clone(),
            actor_count: 0,
            message_count: 0,
            error_count: 0,
            registered_at: obj_reg.created_at.clone(),
        }
    }

    /// Convert NodeRegistration to ObjectRegistration.
    /// Capabilities (e.g. "cluster") are mirrored into metadata.labels so resource-based
    /// routing (NodeSelector, CapacityTracker) and list_nodes cluster filter stay aligned.
    fn to_object_registration(
        node_reg: &NodeRegistration,
        ctx: &RequestContext,
    ) -> ObjectRegistration {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        };

        let metadata = if node_reg.capabilities.is_empty() {
            None
        } else {
            Some(CommonMetadata {
                labels: node_reg.capabilities.clone(),
                ..Default::default()
            })
        };

        ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: node_reg.node_id.clone(),
            object_name: format!("Node {}", node_reg.node_id),
            node_id: node_reg.node_id.clone(),
            grpc_address: node_reg.node_address.clone(),
            object_category: "Node".to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
            health_status: HealthStatus::HealthStatusHealthy as i32,
            capabilities: node_reg.capabilities.keys().cloned().collect(),
            metadata,
            created_at: node_reg.registered_at.clone().or(Some(timestamp.clone())),
            updated_at: Some(timestamp.clone()),
            last_heartbeat: node_reg.last_heartbeat.clone().or(Some(timestamp)),
            ..Default::default()
        }
    }

    /// Convert SwimMember to NodeRegistration
    fn swim_member_to_node_registration(member: &SwimMember) -> NodeRegistration {
        let status = match member.state {
            NodeState::Alive => plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
            NodeState::Suspect => plexspaces_proto::node::v1::NodeStatus::NodeStatusBusy as i32,
            NodeState::Dead | NodeState::Left => {
                plexspaces_proto::node::v1::NodeStatus::NodeStatusStopped as i32
            }
        };

        NodeRegistration {
            node_id: member.node_id.clone(),
            node_address: member.address.clone(),
            capabilities: member.metadata.clone(),
            status,
            last_heartbeat: member.last_probe_success.map(|t| {
                let elapsed = t.elapsed();
                let probe_time = std::time::SystemTime::now() - elapsed;
                let duration = probe_time
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default();
                Timestamp {
                    seconds: duration.as_secs() as i64,
                    nanos: duration.subsec_nanos() as i32,
                }
            }),
            actor_count: 0,
            message_count: 0,
            error_count: member.failed_probes as u64,
            registered_at: None,
        }
    }

    fn is_unknown_node_id(node_id: &str) -> bool {
        node_id.starts_with(UNKNOWN_NODE_ID_PREFIX)
    }

    fn addresses_match(left: &str, right: &str) -> bool {
        canonical_node_address_key(left) == canonical_node_address_key(right)
    }

    fn looks_like_address(target: &str) -> bool {
        target.starts_with("http://") || target.starts_with("https://") || target.contains(':')
    }

    fn now_timestamp() -> Timestamp {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        }
    }

    fn timestamp_within_age(timestamp: Option<&Timestamp>, max_age: Duration) -> bool {
        let Some(timestamp) = timestamp else {
            return false;
        };
        let seconds = u64::try_from(timestamp.seconds.max(0)).unwrap_or_default();
        let nanos = u32::try_from(timestamp.nanos.max(0)).unwrap_or_default();
        let instant =
            UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_nanos(nanos as u64);
        SystemTime::now()
            .duration_since(instant)
            .map(|age| age <= max_age)
            .unwrap_or(true)
    }

    fn object_registration_is_recent(
        registration: &ObjectRegistration,
        active_node_window: Duration,
    ) -> bool {
        Self::timestamp_within_age(registration.last_heartbeat.as_ref(), active_node_window)
            || Self::timestamp_within_age(registration.updated_at.as_ref(), active_node_window)
    }

    async fn recent_node_registrations_from_registry(
        &self,
        cluster: Option<&str>,
    ) -> Result<Vec<NodeRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let ctx = self.system_registry_context(cluster).await;
        let registrations = self
            .object_registry
            .discover(
                &ctx,
                Some(ObjectType::ObjectTypeNode),
                None,
                None,
                None,
                None,
                0,
                1000,
            )
            .await?;

        let mut deduped_by_address: HashMap<String, ObjectRegistration> = HashMap::new();
        for registration in registrations.into_iter().filter(|registration| {
            Self::object_registration_is_recent(registration, self.config.active_node_window)
        }) {
            let key = canonical_node_address_key(&registration.grpc_address);
            match deduped_by_address.get(&key) {
                Some(existing)
                    if Self::is_unknown_node_id(&existing.object_id)
                        && !Self::is_unknown_node_id(&registration.object_id) =>
                {
                    deduped_by_address.insert(key, registration);
                }
                None => {
                    deduped_by_address.insert(key, registration);
                }
                _ => {}
            }
        }

        Ok(deduped_by_address
            .into_values()
            .map(|registration| Self::to_node_registration(&registration))
            .filter(|registration| {
                cluster.is_none_or(|cluster_name| {
                    registration
                        .capabilities
                        .get("cluster")
                        .map(|value| value == cluster_name)
                        .unwrap_or(false)
                })
            })
            .collect())
    }

    async fn persist_node_registration_with_registry(
        object_registry: &Arc<dyn ObjectRegistry>,
        registry_ctx: &RequestContext,
        registration: &NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let target_key = canonical_node_address_key(&registration.node_address);
        let existing = object_registry
            .discover(
                registry_ctx,
                Some(ObjectType::ObjectTypeNode),
                None,
                None,
                None,
                None,
                0,
                1000,
            )
            .await?;

        for candidate in existing {
            if candidate.object_id == registration.node_id {
                continue;
            }
            if canonical_node_address_key(&candidate.grpc_address) != target_key {
                continue;
            }

            let candidate_unknown = Self::is_unknown_node_id(&candidate.object_id);
            let registration_unknown = Self::is_unknown_node_id(&registration.node_id);
            match (candidate_unknown, registration_unknown) {
                (true, false) => {
                    object_registry
                        .unregister(
                            registry_ctx,
                            ObjectType::ObjectTypeNode,
                            &candidate.object_id,
                        )
                        .await?;
                }
                (false, true) => {
                    return Ok(());
                }
                _ => {
                    return Err(format!(
                        "node address '{}' is already registered to '{}'",
                        registration.node_address, candidate.object_id
                    )
                    .into());
                }
            }
        }

        let obj_reg = Self::to_object_registration(registration, registry_ctx);
        object_registry.register(registry_ctx, obj_reg).await?;
        Ok(())
    }

    fn should_probe_member(member: &SwimMember) -> bool {
        member
            .last_probe_success
            .map(|instant| instant.elapsed() <= MAX_PROBE_AGE)
            .unwrap_or(true)
    }

    async fn system_registry_context_for(
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        cluster: Option<&str>,
    ) -> RequestContext {
        let local_cluster = Self::local_cluster_name(service_locator).await;
        let effective_cluster = cluster
            .map(str::to_string)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                if local_cluster.is_empty() {
                    None
                } else {
                    Some(local_cluster)
                }
            });

        let sl_guard = service_locator.read().await;
        let service_locator = sl_guard.as_ref().cloned();
        drop(sl_guard);

        match service_locator {
            Some(service_locator) => {
                if let Some(cluster) = effective_cluster {
                    service_locator
                        .request_context_for_system_operations_with_namespace(cluster)
                        .await
                } else {
                    service_locator
                        .request_context_for_system_operations()
                        .await
                }
            }
            None => RequestContext::new_without_auth(
                String::new(),
                effective_cluster.unwrap_or_default(),
            )
            .with_admin(true),
        }
    }

    async fn unregister_discovered_node(
        cache: &Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
        swim: &Arc<SwimProtocol>,
        object_registry: &Arc<dyn ObjectRegistry>,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        node_id: &str,
        cluster_name: Option<&str>,
    ) {
        swim.declare_dead(node_id).await;
        let mut cache_guard = cache.write().await;
        cache_guard.remove(node_id);
        drop(cache_guard);

        let system_ctx = Self::system_registry_context_for(service_locator, cluster_name).await;
        if let Err(e) = object_registry
            .unregister(&system_ctx, ObjectType::ObjectTypeNode, node_id)
            .await
        {
            warn!(
                node_id = %node_id,
                error = %e,
                "Failed to unregister node during probe reconciliation"
            );
        }
    }

    async fn local_cluster_name(
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
    ) -> String {
        let sl_guard = service_locator.read().await;
        let service_locator = sl_guard.as_ref().cloned();
        drop(sl_guard);

        match service_locator {
            Some(sl) => sl
                .get_node_config()
                .await
                .map(|cfg| cfg.cluster_name)
                .unwrap_or_default(),
            None => String::new(),
        }
    }

    async fn system_registry_context(&self, cluster: Option<&str>) -> RequestContext {
        Self::system_registry_context_for(&self.service_locator, cluster).await
    }

    async fn reconcile_ping_response(
        cache: &Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
        swim: &Arc<SwimProtocol>,
        object_registry: &Arc<dyn ObjectRegistry>,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        cache_ttl: Duration,
        target: &SwimMember,
        response: &PingResponse,
        local_cluster: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let remote_cluster = response.cluster_name.clone();
        let cluster_mismatch = !local_cluster.is_empty()
            && !remote_cluster.is_empty()
            && local_cluster != remote_cluster;

        if cluster_mismatch {
            Self::unregister_discovered_node(
                cache,
                swim,
                object_registry,
                service_locator,
                &target.node_id,
                Some(local_cluster),
            )
            .await;
            return Err(format!(
                "cluster mismatch for {}: local={}, remote={}",
                target.address, local_cluster, remote_cluster
            )
            .into());
        }

        let resolved_node_id = if response.node_id.is_empty() {
            target.node_id.clone()
        } else {
            response.node_id.clone()
        };
        let resolved_address = if response.node_address.is_empty() {
            target.address.clone()
        } else {
            response.node_address.clone()
        };

        if resolved_node_id != target.node_id && Self::is_unknown_node_id(&target.node_id) {
            Self::unregister_discovered_node(
                cache,
                swim,
                object_registry,
                service_locator,
                &target.node_id,
                if remote_cluster.is_empty() {
                    Some(local_cluster)
                } else {
                    Some(remote_cluster.as_str())
                },
            )
            .await;
        }

        let mut capabilities = HashMap::new();
        if !remote_cluster.is_empty() {
            capabilities.insert("cluster".to_string(), remote_cluster.clone());
        }

        let heartbeat = response
            .last_heartbeat
            .clone()
            .unwrap_or_else(Self::now_timestamp);

        let registration = NodeRegistration {
            node_id: resolved_node_id.clone(),
            node_address: resolved_address.clone(),
            capabilities: capabilities.clone(),
            status: plexspaces_proto::node::v1::NodeStatus::NodeStatusReady as i32,
            last_heartbeat: Some(heartbeat.clone()),
            actor_count: 0,
            message_count: 0,
            error_count: 0,
            registered_at: Some(heartbeat),
        };

        let mut member = SwimMember::new(resolved_node_id.clone(), resolved_address);
        member.metadata = capabilities;
        swim.upsert_member(member).await;

        let mut cache_guard = cache.write().await;
        cache_guard.insert(
            resolved_node_id.clone(),
            CachedNodeRegistration::new(registration.clone(), cache_ttl),
        );
        drop(cache_guard);

        let system_ctx = Self::system_registry_context_for(
            service_locator,
            if remote_cluster.is_empty() {
                Some(local_cluster)
            } else {
                Some(remote_cluster.as_str())
            },
        )
        .await;
        Self::persist_node_registration_with_registry(object_registry, &system_ctx, &registration)
            .await?;

        Ok(())
    }

    /// Evict expired entries from cache
    async fn evict_expired(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, entry| !entry.is_expired());
    }

    /// Get node from cache (if not expired)
    async fn get_from_cache(&self, node_id: &str) -> Option<NodeRegistration> {
        let cache = self.cache.read().await;
        if let Some(entry) = cache.get(node_id) {
            if !entry.is_expired() {
                self.cache_hits.fetch_add(1, Ordering::Relaxed);
                return Some(entry.registration.clone());
            }
        }
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        None
    }

    /// Update cache with node registration
    async fn update_cache(&self, node_id: &str, registration: NodeRegistration) {
        let mut cache = self.cache.write().await;
        cache.insert(
            node_id.to_string(),
            CachedNodeRegistration::new(registration, self.config.cache_ttl),
        );
    }

    async fn lookup_cached_or_swim_by_address(&self, target: &str) -> Option<NodeRegistration> {
        let target_key = canonical_node_address_key(target);

        let mut best_match = self
            .swim
            .active_members()
            .await
            .into_iter()
            .filter(|member| Self::addresses_match(&member.address, &target_key))
            .map(|member| Self::swim_member_to_node_registration(&member))
            .find(|registration| !Self::is_unknown_node_id(&registration.node_id));

        if best_match.is_none() {
            best_match = self
                .swim
                .active_members()
                .await
                .into_iter()
                .find(|member| Self::addresses_match(&member.address, &target_key))
                .map(|member| Self::swim_member_to_node_registration(&member));
        }

        if best_match.is_some() {
            return best_match;
        }

        self.evict_expired().await;
        let cache = self.cache.read().await;
        let mut fallback = None;
        for entry in cache.values().filter(|entry| !entry.is_expired()) {
            if !Self::addresses_match(&entry.registration.node_address, &target_key) {
                continue;
            }
            if !Self::is_unknown_node_id(&entry.registration.node_id) {
                return Some(entry.registration.clone());
            }
            if fallback.is_none() {
                fallback = Some(entry.registration.clone());
            }
        }
        fallback
    }

    async fn reconcile_local_registration_aliases(
        &self,
        registration: &NodeRegistration,
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let target_key = canonical_node_address_key(&registration.node_address);

        let existing_swim = self.swim.active_members().await;
        for member in existing_swim {
            if member.node_id == registration.node_id
                || canonical_node_address_key(&member.address) != target_key
            {
                continue;
            }

            let existing_unknown = Self::is_unknown_node_id(&member.node_id);
            let registration_unknown = Self::is_unknown_node_id(&registration.node_id);
            match (existing_unknown, registration_unknown) {
                (true, false) => {
                    self.swim.declare_dead(&member.node_id).await;
                    self.remove_from_cache(&member.node_id).await;
                }
                (false, true) => return Ok(false),
                (false, false) => {
                    return Err(format!(
                        "node address '{}' is already registered to '{}'",
                        registration.node_address, member.node_id
                    )
                    .into());
                }
                (true, true) => return Ok(false),
            }
        }

        self.evict_expired().await;
        let cache_entries: Vec<NodeRegistration> = {
            let cache = self.cache.read().await;
            cache.values()
                .filter(|entry| !entry.is_expired())
                .map(|entry| entry.registration.clone())
                .collect()
        };
        for existing in cache_entries {
            if existing.node_id == registration.node_id
                || canonical_node_address_key(&existing.node_address) != target_key
            {
                continue;
            }

            let existing_unknown = Self::is_unknown_node_id(&existing.node_id);
            let registration_unknown = Self::is_unknown_node_id(&registration.node_id);
            match (existing_unknown, registration_unknown) {
                (true, false) => {
                    self.remove_from_cache(&existing.node_id).await;
                }
                (false, true) => return Ok(false),
                (false, false) => {
                    return Err(format!(
                        "node address '{}' is already registered to '{}'",
                        registration.node_address, existing.node_id
                    )
                    .into());
                }
                (true, true) => return Ok(false),
            }
        }

        Ok(true)
    }

    /// Remove node from cache
    async fn remove_from_cache(&self, node_id: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(node_id);
    }

    /// Perform DB operation with exponential backoff and jitter
    async fn with_db_backoff<T, F, Fut>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
    {
        let mut backoff = ExponentialBackoff::with_params(
            self.config.db_backoff_base,
            self.config.db_backoff_cap,
            self.config.db_max_attempts,
        );

        loop {
            match operation().await {
                Ok(result) => {
                    if backoff.attempts() > 0 {
                        debug!(
                            "DB operation '{}' succeeded after {} attempts",
                            operation_name,
                            backoff.attempts()
                        );
                    }
                    return Ok(result);
                }
                Err(e) => {
                    self.db_failures.fetch_add(1, Ordering::Relaxed);

                    match backoff.next_backoff() {
                        Some(delay) => {
                            warn!(
                                "DB operation '{}' failed (attempt {}): {}. Retrying in {:?}",
                                operation_name,
                                backoff.attempts(),
                                e,
                                delay
                            );
                            tokio::time::sleep(delay).await;
                        }
                        None => {
                            error!(
                                "DB operation '{}' failed after {} attempts: {}",
                                operation_name,
                                backoff.attempts(),
                                e
                            );
                            metrics::counter!("plexspaces_node_registry_db_exhausted").increment(1);
                            return Err(e);
                        }
                    }
                }
            }
        }
    }

    /// Sync local state with database (anti-entropy)
    async fn sync_with_db(
        &self,
        _ctx: &RequestContext,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.config.use_shared_db {
            return Ok(());
        }

        let object_registry = self.object_registry.clone();
        let system_ctx = self.system_registry_context(None).await;

        let registrations = self
            .with_db_backoff("sync_discover", || {
                let registry = object_registry.clone();
                let ctx = system_ctx.clone();
                async move {
                    registry
                        .discover(
                            &ctx,
                            Some(ObjectType::ObjectTypeNode),
                            None,
                            None,
                            None,
                            None,
                            0,
                            1000,
                        )
                        .await
                        .map_err(|e| format!("{}", e).into())
                }
            })
            .await?;

        // Merge DB state with SWIM state
        for obj_reg in registrations {
            let node_reg = Self::to_node_registration(&obj_reg);

            // Update cache
            self.update_cache(&node_reg.node_id, node_reg.clone()).await;

            // Update SWIM if node is not already known with higher incarnation
            if let Some(_existing) = self.swim.get_member(&obj_reg.object_id).await {
                // DB doesn't have incarnation, so only update if SWIM doesn't know this node
                continue;
            }

            let mut member =
                SwimMember::new(obj_reg.object_id.clone(), obj_reg.grpc_address.clone());
            member.metadata = node_reg.capabilities.clone();
            self.swim.upsert_member(member).await;
        }

        Ok(())
    }

    /// Run the SWIM protocol loop
    async fn run_swim_loop(
        swim: Arc<SwimProtocol>,
        cache: Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
        cache_ttl: Duration,
        object_registry: Arc<dyn ObjectRegistry>,
        service_locator: Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        running: Arc<AtomicBool>,
        config: SwimConfig,
    ) {
        let mut protocol_interval = tokio::time::interval(config.protocol_period);
        let mut anti_entropy_interval = tokio::time::interval(config.anti_entropy_interval);

        info!(
            "Starting SWIM protocol loop: period={:?}, indirect_nodes={}",
            config.protocol_period, config.indirect_ping_nodes
        );

        loop {
            tokio::select! {
                _ = protocol_interval.tick() => {
                    if !running.load(Ordering::Relaxed) {
                        info!("SWIM protocol loop stopping");
                        break;
                    }

                    // Select and probe a random node
                    if let Some(target) = swim.select_probe_target().await {
                        if Self::should_probe_member(&target) {
                            let local_cluster = Self::local_cluster_name(&service_locator).await;
                            let probe_result = Self::probe_node(
                                &swim,
                                &cache,
                                &object_registry,
                                cache_ttl,
                                &target,
                                &service_locator,
                                &local_cluster,
                                &config,
                            ).await;

                            match probe_result {
                                ProbeResult::Alive => {
                                }
                                ProbeResult::Suspect => {
                                    swim.suspect_member(&target.node_id).await;
                                }
                                ProbeResult::Failed => {
                                    // Already handled in probe_node
                                }
                            }
                        }
                    }

                    // Check suspect timeouts
                    swim.check_suspect_timeouts().await;

                    // Reap dead nodes
                    swim.reap_dead_nodes().await;
                }
                _ = anti_entropy_interval.tick() => {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }

                    // Perform anti-entropy sync with a random peer
                    if let Some(peer) = swim.select_probe_target().await {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Anti-entropy sync with peer: {}", peer.node_id);
                        }
                        // Full state sync would happen here via gRPC
                        // For now, just log
                    }
                }
            }
        }
    }

    /// Probe a node (direct ping, then indirect if needed)
    async fn probe_node(
        swim: &Arc<SwimProtocol>,
        cache: &Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
        object_registry: &Arc<dyn ObjectRegistry>,
        cache_ttl: Duration,
        target: &SwimMember,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        local_cluster: &str,
        config: &SwimConfig,
    ) -> ProbeResult {
        // Try direct ping first
        match Self::direct_ping(target, service_locator, config.probe_timeout).await {
            Ok(response) => {
                if let Err(e) = Self::reconcile_ping_response(
                    cache,
                    swim,
                    object_registry,
                    service_locator,
                    cache_ttl,
                    target,
                    &response,
                    local_cluster,
                )
                .await
                {
                    warn!(
                        node_id = %target.node_id,
                        address = %target.address,
                        error = %e,
                        "Failed to reconcile successful node ping"
                    );
                    return ProbeResult::Failed;
                }
                metrics::counter!("plexspaces_swim_direct_ping_success").increment(1);
                return ProbeResult::Alive;
            }
            Err(e) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("Direct ping to {} failed: {}", target.node_id, e);
                }
                metrics::counter!("plexspaces_swim_direct_ping_failed").increment(1);
            }
        }

        // Direct ping failed - try indirect ping through other nodes
        let indirect_targets = swim.select_indirect_targets(&target.node_id).await;

        if indirect_targets.is_empty() {
            // No indirect nodes available - mark as suspect immediately
            swim.suspect_member(&target.node_id).await;
            return ProbeResult::Suspect;
        }

        // Try indirect pings in parallel
        let mut indirect_futures = Vec::new();
        for intermediary in &indirect_targets {
            indirect_futures.push(Self::indirect_ping(
                intermediary,
                target,
                service_locator,
                config.probe_timeout,
            ));
        }

        let results = futures::future::join_all(indirect_futures).await;

        // If any indirect ping succeeded, node is alive
        if results.iter().any(|r| r.is_ok()) {
            swim.process_alive(&target.node_id, target.incarnation, &target.address)
                .await;
            metrics::counter!("plexspaces_swim_indirect_ping_success").increment(1);
            return ProbeResult::Alive;
        }

        // All probes failed - mark as suspect
        swim.suspect_member(&target.node_id).await;
        metrics::counter!("plexspaces_swim_indirect_ping_failed").increment(1);
        ProbeResult::Suspect
    }

    /// Direct ping to a node
    async fn direct_ping(
        target: &SwimMember,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        timeout: Duration,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::grpc_connection_manager::ServiceType;
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        use plexspaces_proto::node::v1::PingRequest;

        let sl_guard = service_locator.read().await;
        let sl = sl_guard
            .as_ref()
            .ok_or_else(|| "ServiceLocator not available".to_string())?;

        let conn_manager = sl
            .get_grpc_connection_manager()
            .await
            .ok_or_else(|| "GrpcConnectionManager not available".to_string())?;

        let source_node_id = sl
            .get_node_config()
            .await
            .map(|cfg| cfg.id)
            .unwrap_or_default();

        let channel = conn_manager
            .get_connection(ServiceType::NodeService, &target.node_id, &target.address)
            .await
            .map_err(|e| format!("Failed to get channel: {}", e))?;

        let mut client = NodeServiceClient::new(channel);

        let request = tonic::Request::new(PingRequest {
            source_node_id,
            sequence_number: 0,
            updates: Vec::new(),
        });

        let response = tokio::time::timeout(timeout, client.ping(request))
            .await
            .map_err(|_| "Ping timeout")?
            .map_err(|e| format!("Ping failed: {}", e))?;

        Ok(response.into_inner())
    }

    /// Indirect ping (ask intermediary to ping target)
    async fn indirect_ping(
        intermediary: &SwimMember,
        target: &SwimMember,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_core::grpc_connection_manager::ServiceType;
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        use plexspaces_proto::node::v1::PingReqRequest;

        let sl_guard = service_locator.read().await;
        let sl = sl_guard
            .as_ref()
            .ok_or_else(|| "ServiceLocator not available".to_string())?;

        let conn_manager = sl
            .get_grpc_connection_manager()
            .await
            .ok_or_else(|| "GrpcConnectionManager not available".to_string())?;

        let channel = conn_manager
            .get_connection(
                ServiceType::NodeService,
                &intermediary.node_id,
                &intermediary.address,
            )
            .await
            .map_err(|e| format!("Failed to get channel to intermediary: {}", e))?;

        let mut client = NodeServiceClient::new(channel);

        let request = tonic::Request::new(PingReqRequest {
            source_node_id: String::new(),
            target_node_id: target.node_id.clone(),
            target_address: target.address.clone(),
            sequence_number: 0,
        });

        tokio::time::timeout(timeout * 2, client.ping_req(request)) // Double timeout for indirect
            .await
            .map_err(|_| "Indirect ping timeout")?
            .map_err(|e| format!("Indirect ping failed: {}", e))?;

        Ok(())
    }
}

/// Result of a probe operation
enum ProbeResult {
    Alive,
    Suspect,
    Failed,
}

#[async_trait]
impl NodeRegistryTrait for NodeRegistry {
    async fn lookup_node(
        &self,
        _ctx: &RequestContext,
        target: &str,
    ) -> Result<Option<NodeRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        if Self::looks_like_address(target) {
            if let Some(registration) = self.lookup_cached_or_swim_by_address(target).await {
                return Ok(Some(registration));
            }
        }

        // Check SWIM first (most up-to-date)
        if let Some(member) = self.swim.get_member(target).await {
            if member.state.is_active() {
                let node_reg = Self::swim_member_to_node_registration(&member);
                return Ok(Some(node_reg));
            }
        }

        // Check cache
        if let Some(registration) = self.get_from_cache(target).await {
            return Ok(Some(registration));
        }

        // Cache miss - lookup in ObjectRegistry with backoff
        if self.config.use_shared_db {
            let object_registry = self.object_registry.clone();
            let target_owned = target.to_string();
            let system_ctx = self.system_registry_context(None).await;

            let result = self
                .with_db_backoff("lookup", || {
                    let registry = object_registry.clone();
                    let ctx = system_ctx.clone();
                    let lookup_target = target_owned.clone();
                    async move {
                        if let Some(found) = registry
                            .lookup_full(&ctx, ObjectType::ObjectTypeNode, &lookup_target)
                            .await
                            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                                format!("{}", e).into()
                            })?
                        {
                            return Ok(Some(found));
                        }

                        if !Self::looks_like_address(&lookup_target) {
                            return Ok(None);
                        }

                        let registrations = registry
                            .discover(
                                &ctx,
                                Some(ObjectType::ObjectTypeNode),
                                None,
                                None,
                                None,
                                None,
                                0,
                                1000,
                            )
                            .await
                            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                                format!("{}", e).into()
                            })?;
                        let mut fallback = None;
                        for registration in registrations {
                            if canonical_node_address_key(&registration.grpc_address)
                                != canonical_node_address_key(&lookup_target)
                            {
                                continue;
                            }
                            if !Self::is_unknown_node_id(&registration.object_id) {
                                return Ok(Some(registration));
                            }
                            if fallback.is_none() {
                                fallback = Some(registration);
                            }
                        }
                        Ok(fallback)
                    }
                })
                .await;

            match result {
                Ok(Some(obj_reg)) => {
                    let node_reg = Self::to_node_registration(&obj_reg);
                    self.update_cache(node_reg.node_id.as_str(), node_reg.clone())
                        .await;

                    // Also update SWIM
                    let mut member =
                        SwimMember::new(obj_reg.object_id.clone(), obj_reg.grpc_address.clone());
                    member.metadata = node_reg.capabilities.clone();
                    self.swim.upsert_member(member).await;

                    return Ok(Some(node_reg));
                }
                Ok(None) => return Ok(None),
                Err(e) => {
                    warn!("DB lookup failed, returning cache-only result: {}", e);
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    async fn register_node(
        &self,
        _ctx: &RequestContext,
        registration: NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let node_id = registration.node_id.clone();

        if !self
            .reconcile_local_registration_aliases(&registration)
            .await?
        {
            return Ok(());
        }

        // Update SWIM first (immediate local update)
        let mut member = SwimMember::new(
            registration.node_id.clone(),
            registration.node_address.clone(),
        );
        for (k, v) in &registration.capabilities {
            member.metadata.insert(k.clone(), v.clone());
        }
        self.swim.upsert_member(member).await;

        // Update cache
        self.update_cache(&node_id, registration.clone()).await;

        let registry_ctx = self
            .system_registry_context(
                registration
                    .capabilities
                    .get("cluster")
                    .map(|value| value.as_str()),
            )
            .await;
        if let Err(e) = Self::persist_node_registration_with_registry(
            &self.object_registry,
            &registry_ctx,
            &registration,
        )
        .await
        {
            warn!(
                "Failed to register node {} in ObjectRegistry: {}",
                node_id, e
            );
        }

        info!("Registered node: {}", node_id);
        metrics::counter!("plexspaces_node_registry_registrations_total").increment(1);

        Ok(())
    }

    async fn unregister_node(
        &self,
        _ctx: &RequestContext,
        node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Update SWIM - mark as left
        self.swim.declare_dead(node_id).await;

        // Remove from cache
        self.remove_from_cache(node_id).await;

        let registry_ctx = self.system_registry_context(None).await;
        if let Err(e) = self
            .object_registry
            .unregister(&registry_ctx, ObjectType::ObjectTypeNode, node_id)
            .await
        {
            warn!(
                "Failed to unregister node {} from ObjectRegistry: {}",
                node_id, e
            );
        }

        info!("Unregistered node: {}", node_id);
        metrics::counter!("plexspaces_node_registry_unregistrations_total").increment(1);

        Ok(())
    }

    async fn list_nodes(
        &self,
        _ctx: &RequestContext,
        cluster: Option<&str>,
        page_size: u32,
        _page_token: &str,
    ) -> Result<(Vec<NodeRegistration>, String), Box<dyn std::error::Error + Send + Sync>> {
        let local_cluster = Self::local_cluster_name(&self.service_locator).await;
        let cluster_filter = cluster.or_else(|| {
            if local_cluster.is_empty() {
                None
            } else {
                Some(local_cluster.as_str())
            }
        });

        let mut nodes_by_id: HashMap<String, NodeRegistration> = HashMap::new();
        if self.config.use_shared_db {
            self.sync_with_db(_ctx).await?;
            // In shared-DB mode the object registry is the authoritative membership view.
            // SWIM still drives liveness reconciliation, but list_nodes must not surface
            // nodes that are absent or stale in the backing registry.
            for registration in self
                .recent_node_registrations_from_registry(cluster_filter)
                .await?
            {
                nodes_by_id
                    .entry(registration.node_id.clone())
                    .or_insert(registration);
            }
        } else {
            nodes_by_id.extend(
                self.swim
                    .active_members()
                    .await
                    .into_iter()
                    .filter_map(|member| {
                        if let Some(cluster_name) = cluster_filter {
                            if member.metadata.get("cluster") != Some(&cluster_name.to_string()) {
                                return None;
                            }
                        }
                        let registration = Self::swim_member_to_node_registration(&member);
                        Some((registration.node_id.clone(), registration))
                    }),
            );

            self.evict_expired().await;
            let cache = self.cache.read().await;
            for entry in cache
                .values()
                .filter(|entry| !entry.is_expired())
                .filter(|entry| {
                    cluster_filter.is_none_or(|cluster_name| {
                        entry.registration.capabilities.get("cluster")
                            == Some(&cluster_name.to_string())
                    })
                })
            {
                nodes_by_id
                    .entry(entry.registration.node_id.clone())
                    .or_insert_with(|| entry.registration.clone());
            }
        }

        let mut nodes: Vec<NodeRegistration> = nodes_by_id.into_values().collect();
        nodes.sort_by(|left, right| left.node_id.cmp(&right.node_id));

        let limit = if page_size > 0 {
            page_size as usize
        } else {
            nodes.len()
        };
        let result: Vec<NodeRegistration> = nodes.into_iter().take(limit).collect();

        Ok((result, String::new()))
    }

    async fn send_heartbeat(
        &self,
        _ctx: &RequestContext,
        node_id: &str,
        capacity: Option<NodeCapacity>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let heartbeat_timestamp = Self::now_timestamp();

        // Update SWIM member status
        if let Some(mut member) = self.swim.get_member(node_id).await {
            member.record_probe_success();
            self.swim.upsert_member(member).await;
        }

        {
            let mut cache = self.cache.write().await;
            if let Some(entry) = cache.get_mut(node_id) {
                entry.registration.last_heartbeat = Some(heartbeat_timestamp.clone());
            }
        }

        // Update DB if enabled
        if self.config.use_shared_db {
            let object_registry = self.object_registry.clone();
            let node_id_owned = node_id.to_string();
            let capacity_clone = capacity.clone();
            let heartbeat_timestamp_clone = heartbeat_timestamp.clone();
            let system_ctx = self.system_registry_context(None).await;

            if let Err(e) = self
                .with_db_backoff("heartbeat", || {
                    let registry = object_registry.clone();
                    let ctx = system_ctx.clone();
                    let nid = node_id_owned.clone();
                    let cap = capacity_clone.clone();
                    let heartbeat_timestamp = heartbeat_timestamp_clone.clone();
                    async move {
                        let obj_reg = registry
                            .lookup_full(&ctx, ObjectType::ObjectTypeNode, &nid)
                            .await
                            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                                format!("{}", e).into()
                            })?;
                        if let Some(mut obj_reg) = obj_reg {
                            obj_reg.last_heartbeat = Some(heartbeat_timestamp.clone());
                            obj_reg.updated_at = Some(heartbeat_timestamp);
                            obj_reg.health_status = HealthStatus::HealthStatusHealthy as i32;

                            if let Some(ref cap) = cap {
                                if let Some(ref total) = cap.total {
                                    // Store capacity info in metrics (which is a HashMap<String, f64>)
                                    obj_reg.metrics.insert(
                                        "total_cpu_cores".to_string(),
                                        total.cpu_cores as f64,
                                    );
                                    obj_reg.metrics.insert(
                                        "total_memory_bytes".to_string(),
                                        total.memory_bytes as f64,
                                    );
                                }
                            }

                            registry.register(&ctx, obj_reg).await.map_err(
                                |e| -> Box<dyn std::error::Error + Send + Sync> {
                                    format!("{}", e).into()
                                },
                            )?;
                        }
                        Ok(())
                    }
                })
                .await
            {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    debug!("Heartbeat DB update failed (non-critical): {}", e);
                }
            }
        }

        metrics::counter!("plexspaces_node_registry_heartbeats_total").increment(1);
        Ok(())
    }

    fn start_gossip_protocol(&self) {
        if !self.config.gossip_enabled {
            info!("SWIM gossip protocol is disabled");
            return;
        }

        if self.running.swap(true, Ordering::SeqCst) {
            warn!("SWIM gossip protocol already running");
            return;
        }

        let swim = self.swim.clone();
        let cache = self.cache.clone();
        let cache_ttl = self.config.cache_ttl;
        let object_registry = self.object_registry.clone();
        let service_locator = self.service_locator.clone();
        let running = self.running.clone();
        let config = self.config.swim_config.clone();

        tokio::spawn(async move {
            Self::run_swim_loop(
                swim,
                cache,
                cache_ttl,
                object_registry,
                service_locator,
                running,
                config,
            )
            .await;
        });
    }

    fn stop_gossip_protocol(&self) {
        if self.running.swap(false, Ordering::SeqCst) {
            info!("Stopping SWIM gossip protocol");
            self.swim.stop();
        }
    }

    fn is_gossip_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    async fn cache_stats(&self) -> (usize, usize, Duration) {
        let cache = self.cache.read().await;
        let cache_size = cache.len();
        let hits = self.cache_hits.load(Ordering::Relaxed) as usize;
        (cache_size, hits, self.config.cache_ttl)
    }
}

// Re-export for backward compatibility
pub fn new(
    object_registry: Arc<dyn ObjectRegistry>,
    local_node_id: String,
    cache_ttl_seconds: Option<u64>,
    gossip_enabled: Option<bool>,
    gossip_interval_ms: Option<u64>,
    gossip_fanout: Option<usize>,
) -> NodeRegistry {
    NodeRegistry::new_simple(
        object_registry,
        local_node_id,
        cache_ttl_seconds,
        gossip_enabled,
        gossip_interval_ms,
        gossip_fanout,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::common::v1::Metadata as CommonMetadata;
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};

    async fn create_test_node_registry() -> NodeRegistry {
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));
        let mut config = NodeRegistryConfig::default();
        config.gossip_enabled = false; // Disable for unit tests
        config.use_shared_db = false;

        NodeRegistry::new(
            object_registry,
            "test-node".to_string(),
            "localhost:8000".to_string(),
            config,
        )
    }

    async fn create_test_node_registry_with_db() -> NodeRegistry {
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));
        let mut config = NodeRegistryConfig::default();
        config.gossip_enabled = false;
        config.use_shared_db = true;
        config.db_max_attempts = 3; // Fewer retries for tests
        config.db_backoff_base = Duration::from_millis(10);
        config.db_backoff_cap = Duration::from_millis(100);

        NodeRegistry::new(
            object_registry,
            "test-node".to_string(),
            "localhost:8000".to_string(),
            config,
        )
    }

    #[tokio::test]
    async fn test_register_and_lookup_node() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "node-1".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };

        registry
            .register_node(&ctx, node_reg.clone())
            .await
            .unwrap();

        let result = registry.lookup_node(&ctx, "node-1").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().node_id, "node-1");
    }

    #[tokio::test]
    async fn test_lookup_node_by_address_collapses_loopback_aliases() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "node-loopback".to_string(),
                    node_address: "http://0.0.0.0:8002".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let result = registry
            .lookup_node(&ctx, "http://localhost:8002")
            .await
            .unwrap()
            .expect("address lookup should resolve canonical loopback alias");
        assert_eq!(result.node_id, "node-loopback");
    }

    #[tokio::test]
    async fn test_register_node_upserts_unknown_seed_to_concrete_node_in_local_state() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "_unknown_seed".to_string(),
                    node_address: "http://localhost:8116".to_string(),
                    capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "node-concrete".to_string(),
                    node_address: "http://0.0.0.0:8116".to_string(),
                    capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let by_id = registry
            .lookup_node(&ctx, "node-concrete")
            .await
            .unwrap()
            .expect("concrete node should remain registered");
        assert_eq!(by_id.node_address, "http://0.0.0.0:8116");

        assert!(
            registry.lookup_node(&ctx, "_unknown_seed").await.unwrap().is_none(),
            "placeholder identity should be removed once a concrete node claims the address"
        );

        let by_address = registry
            .lookup_node(&ctx, "http://localhost:8116")
            .await
            .unwrap()
            .expect("address lookup should resolve to the concrete node");
        assert_eq!(by_address.node_id, "node-concrete");

        let (nodes, _) = registry.list_nodes(&ctx, Some("heat"), 100, "").await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "node-concrete");
    }

    #[test]
    fn test_to_node_registration_preserves_metadata_labels() {
        let registration = NodeRegistry::to_node_registration(&ObjectRegistration {
            object_id: "node-labeled".to_string(),
            grpc_address: "http://localhost:8119".to_string(),
            capabilities: vec!["cluster".to_string(), "gpu".to_string()],
            metadata: Some(CommonMetadata {
                labels: HashMap::from([
                    ("cluster".to_string(), "heat".to_string()),
                    ("rack".to_string(), "r1".to_string()),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        });

        assert_eq!(
            registration.capabilities.get("cluster"),
            Some(&"heat".to_string())
        );
        assert_eq!(
            registration.capabilities.get("rack"),
            Some(&"r1".to_string())
        );
        assert_eq!(
            registration.capabilities.get("gpu"),
            Some(&"true".to_string())
        );
    }

    #[tokio::test]
    async fn test_unregister_node() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "node-2".to_string(),
            node_address: "http://localhost:8002".to_string(),
            ..Default::default()
        };

        registry.register_node(&ctx, node_reg).await.unwrap();
        registry.unregister_node(&ctx, "node-2").await.unwrap();

        let result = registry.lookup_node(&ctx, "node-2").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_nodes() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        for i in 0..3 {
            let node_reg = NodeRegistration {
                node_id: format!("node-{}", i),
                node_address: format!("http://localhost:800{}", i),
                ..Default::default()
            };
            registry.register_node(&ctx, node_reg).await.unwrap();
        }

        let (nodes, _token) = registry.list_nodes(&ctx, None, 10, "").await.unwrap();
        assert_eq!(nodes.len(), 3);
    }

    #[tokio::test]
    async fn test_heartbeat() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "node-heartbeat".to_string(),
            node_address: "http://localhost:8099".to_string(),
            ..Default::default()
        };

        registry.register_node(&ctx, node_reg).await.unwrap();
        registry
            .send_heartbeat(&ctx, "node-heartbeat", None)
            .await
            .unwrap();

        let result = registry.lookup_node(&ctx, "node-heartbeat").await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_list_nodes_includes_recent_object_registry_entries() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);
        let timestamp = Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            nanos: 0,
        };

        registry
            .object_registry
            .register(
                &ctx,
                ObjectRegistration {
                    object_type: ObjectType::ObjectTypeNode as i32,
                    object_id: "db-node-visible".to_string(),
                    node_id: "db-node-visible".to_string(),
                    grpc_address: "http://localhost:8111".to_string(),
                    object_category: "Node".to_string(),
                    capabilities: vec!["cluster".to_string()],
                    metadata: Some(CommonMetadata {
                        labels: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                        ..Default::default()
                    }),
                    last_heartbeat: Some(timestamp.clone()),
                    updated_at: Some(timestamp),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let (nodes, _) = registry
            .list_nodes(&ctx, Some("heat"), 100, "")
            .await
            .unwrap();
        assert!(nodes.iter().any(|node| node.node_id == "db-node-visible"));
    }

    #[tokio::test]
    async fn test_list_nodes_excludes_stale_object_registry_entries() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);
        let stale_timestamp = Timestamp {
            seconds: (SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                - Duration::from_secs(2 * 24 * 60 * 60))
            .as_secs() as i64,
            nanos: 0,
        };

        registry
            .object_registry
            .register(
                &ctx,
                ObjectRegistration {
                    object_type: ObjectType::ObjectTypeNode as i32,
                    object_id: "db-node-stale".to_string(),
                    node_id: "db-node-stale".to_string(),
                    grpc_address: "http://localhost:8112".to_string(),
                    object_category: "Node".to_string(),
                    capabilities: vec!["cluster".to_string()],
                    metadata: Some(CommonMetadata {
                        labels: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                        ..Default::default()
                    }),
                    last_heartbeat: Some(stale_timestamp.clone()),
                    updated_at: Some(stale_timestamp),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let (nodes, _) = registry
            .list_nodes(&ctx, Some("heat"), 100, "")
            .await
            .unwrap();
        assert!(!nodes.iter().any(|node| node.node_id == "db-node-stale"));
    }

    #[tokio::test]
    async fn test_list_nodes_prefers_concrete_node_id_over_unknown_loopback_alias() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);
        let timestamp = Timestamp {
            seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            nanos: 0,
        };

        for (object_id, address) in [
            ("_unknown_loopback".to_string(), "http://localhost:8114".to_string()),
            ("db-node-concrete".to_string(), "http://0.0.0.0:8114".to_string()),
        ] {
            registry
                .object_registry
                .register(
                    &ctx,
                    ObjectRegistration {
                        object_type: ObjectType::ObjectTypeNode as i32,
                        object_id: object_id.clone(),
                        node_id: object_id,
                        grpc_address: address,
                        object_category: "Node".to_string(),
                        capabilities: vec!["cluster".to_string()],
                        metadata: Some(CommonMetadata {
                            labels: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                            ..Default::default()
                        }),
                        last_heartbeat: Some(timestamp.clone()),
                        updated_at: Some(timestamp.clone()),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
        }

        let (nodes, _) = registry
            .list_nodes(&ctx, Some("heat"), 100, "")
            .await
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "db-node-concrete");
    }

    #[tokio::test]
    async fn test_register_node_rejects_duplicate_concrete_address() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "node-a".to_string(),
                    node_address: "http://localhost:8115".to_string(),
                    capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "node-b".to_string(),
                    node_address: "http://0.0.0.0:8115".to_string(),
                    capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let (nodes, _) = registry
            .list_nodes(&ctx, Some("heat"), 100, "")
            .await
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "node-a");
    }

    #[tokio::test]
    async fn test_list_nodes_uses_object_registry_as_source_of_truth_in_shared_db_mode() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth(String::new(), "heat".to_string()).with_admin(true);

        let mut transient_member = SwimMember::new(
            "swim-only-node".to_string(),
            "http://localhost:8113".to_string(),
        );
        transient_member
            .metadata
            .insert("cluster".to_string(), "heat".to_string());
        registry.swim.upsert_member(transient_member).await;

        let (nodes, _) = registry
            .list_nodes(&ctx, Some("heat"), 100, "")
            .await
            .unwrap();
        assert!(!nodes.iter().any(|node| node.node_id == "swim-only-node"));
    }

    #[test]
    fn test_should_probe_member_skips_old_heartbeat() {
        let mut member = SwimMember::new("node-1".to_string(), "http://localhost:8001".to_string());
        member.last_probe_success = Some(Instant::now() - (MAX_PROBE_AGE + Duration::from_secs(1)));

        assert!(!NodeRegistry::should_probe_member(&member));
    }

    #[tokio::test]
    async fn test_reconcile_ping_response_replaces_unknown_node_id() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "_unknown_test".to_string(),
                    node_address: "http://localhost:8002".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let target = registry.swim.get_member("_unknown_test").await.unwrap();
        NodeRegistry::reconcile_ping_response(
            &registry.cache,
            &registry.swim,
            &registry.object_registry,
            &registry.service_locator,
            registry.config.cache_ttl,
            &target,
            &PingResponse {
                node_id: "node-2".to_string(),
                sequence_number: 0,
                incarnation: 0,
                updates: vec![],
                cluster_name: "cluster-a".to_string(),
                node_address: "http://localhost:8122".to_string(),
                last_heartbeat: None,
            },
            "cluster-a",
        )
        .await
        .unwrap();

        assert!(registry
            .lookup_node(&ctx, "_unknown_test")
            .await
            .unwrap()
            .is_none());
        let resolved = registry.lookup_node(&ctx, "node-2").await.unwrap().unwrap();
        assert_eq!(resolved.node_address, "http://localhost:8122");
        assert_eq!(
            resolved.capabilities.get("cluster"),
            Some(&"cluster-a".to_string())
        );
    }

    #[tokio::test]
    async fn test_reconcile_ping_response_updates_heartbeat_in_registry() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "_unknown_test".to_string(),
                    node_address: "http://localhost:8004".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let target = registry.swim.get_member("_unknown_test").await.unwrap();
        let heartbeat = Timestamp {
            seconds: 1_700_000_000,
            nanos: 123_000_000,
        };
        NodeRegistry::reconcile_ping_response(
            &registry.cache,
            &registry.swim,
            &registry.object_registry,
            &registry.service_locator,
            registry.config.cache_ttl,
            &target,
            &PingResponse {
                node_id: "node-4".to_string(),
                sequence_number: 0,
                incarnation: 0,
                updates: vec![],
                cluster_name: "cluster-a".to_string(),
                node_address: "http://localhost:8124".to_string(),
                last_heartbeat: Some(heartbeat.clone()),
            },
            "cluster-a",
        )
        .await
        .unwrap();

        let cluster_ctx = RequestContext::new_without_auth(String::new(), "cluster-a".to_string())
            .with_admin(true);
        let resolved = registry
            .lookup_node(&cluster_ctx, "node-4")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.node_address, "http://localhost:8124");
        assert_eq!(resolved.last_heartbeat, Some(heartbeat.clone()));

        let object_registration = registry
            .object_registry
            .lookup_full(&cluster_ctx, ObjectType::ObjectTypeNode, "node-4")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_registration.last_heartbeat, Some(heartbeat));
    }

    #[tokio::test]
    async fn test_reconcile_ping_response_removes_cluster_mismatch() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        registry
            .register_node(
                &ctx,
                NodeRegistration {
                    node_id: "_unknown_test".to_string(),
                    node_address: "http://localhost:8003".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let target = registry.swim.get_member("_unknown_test").await.unwrap();
        let result = NodeRegistry::reconcile_ping_response(
            &registry.cache,
            &registry.swim,
            &registry.object_registry,
            &registry.service_locator,
            registry.config.cache_ttl,
            &target,
            &PingResponse {
                node_id: "node-3".to_string(),
                sequence_number: 0,
                incarnation: 0,
                updates: vec![],
                cluster_name: "cluster-b".to_string(),
                node_address: "http://localhost:8123".to_string(),
                last_heartbeat: None,
            },
            "cluster-a",
        )
        .await;

        assert!(result.is_err());
        assert!(registry
            .lookup_node(&ctx, "_unknown_test")
            .await
            .unwrap()
            .is_none());
        assert!(registry
            .lookup_node(&ctx, "node-3")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_swim_integration() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        // Register nodes
        for i in 0..5 {
            let node_reg = NodeRegistration {
                node_id: format!("swim-node-{}", i),
                node_address: format!("http://localhost:900{}", i),
                ..Default::default()
            };
            registry.register_node(&ctx, node_reg).await.unwrap();
        }

        // Check SWIM has all members
        let members = registry.swim().active_members().await;
        assert_eq!(members.len(), 5);

        // Suspect a node
        registry.swim().suspect_member("swim-node-0").await;

        let member = registry.swim().get_member("swim-node-0").await.unwrap();
        assert_eq!(member.state, NodeState::Suspect);
    }

    #[tokio::test]
    async fn test_db_fallback_with_backoff() {
        let registry = create_test_node_registry_with_db().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "db-node-1".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };

        // Register should persist to DB
        registry
            .register_node(&ctx, node_reg.clone())
            .await
            .unwrap();

        // Lookup should work
        let result = registry.lookup_node(&ctx, "db-node-1").await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_gossip_protocol_disabled() {
        let registry = create_test_node_registry().await;

        assert!(!registry.is_gossip_running());
        registry.start_gossip_protocol();
        // Should still not be running since gossip is disabled in config
        assert!(!registry.is_gossip_running());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let (size, hits, ttl) = registry.cache_stats().await;
        assert_eq!(size, 0);
        assert_eq!(ttl, Duration::from_secs(60));

        let node_reg = NodeRegistration {
            node_id: "node-stats".to_string(),
            node_address: "http://localhost:7777".to_string(),
            ..Default::default()
        };
        registry.register_node(&ctx, node_reg).await.unwrap();

        let (size, _, _) = registry.cache_stats().await;
        assert_eq!(size, 1);
    }

    #[tokio::test]
    async fn test_pagination() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        for i in 0..10 {
            let node_reg = NodeRegistration {
                node_id: format!("paginated-node-{:02}", i),
                node_address: format!("http://localhost:{}", 9000 + i),
                ..Default::default()
            };
            registry.register_node(&ctx, node_reg).await.unwrap();
        }

        let (nodes, _token) = registry.list_nodes(&ctx, None, 5, "").await.unwrap();
        assert_eq!(nodes.len(), 5);
    }

    #[tokio::test]
    async fn test_swim_member_state_transitions() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "state-node".to_string(),
            node_address: "http://localhost:8888".to_string(),
            ..Default::default()
        };
        registry.register_node(&ctx, node_reg).await.unwrap();

        // Should be alive initially
        let member = registry.swim().get_member("state-node").await.unwrap();
        assert_eq!(member.state, NodeState::Alive);

        // Suspect
        registry.swim().suspect_member("state-node").await;
        let member = registry.swim().get_member("state-node").await.unwrap();
        assert_eq!(member.state, NodeState::Suspect);

        // Declare dead
        registry.swim().declare_dead("state-node").await;
        let member = registry.swim().get_member("state-node").await.unwrap();
        assert_eq!(member.state, NodeState::Dead);
    }

    #[tokio::test]
    async fn test_cluster_filter() {
        let registry = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        // Add nodes to different clusters
        let mut node1 = NodeRegistration {
            node_id: "cluster-a-node".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };
        node1
            .capabilities
            .insert("cluster".to_string(), "cluster-a".to_string());
        registry.register_node(&ctx, node1).await.unwrap();

        let mut node2 = NodeRegistration {
            node_id: "cluster-b-node".to_string(),
            node_address: "http://localhost:8002".to_string(),
            ..Default::default()
        };
        node2
            .capabilities
            .insert("cluster".to_string(), "cluster-b".to_string());
        registry.register_node(&ctx, node2).await.unwrap();

        // List all
        let (all_nodes, _) = registry.list_nodes(&ctx, None, 10, "").await.unwrap();
        assert_eq!(all_nodes.len(), 2);

        // List cluster-a only
        let (cluster_a_nodes, _) = registry
            .list_nodes(&ctx, Some("cluster-a"), 10, "")
            .await
            .unwrap();
        assert_eq!(cluster_a_nodes.len(), 1);
        assert_eq!(cluster_a_nodes[0].node_id, "cluster-a-node");
    }

    #[tokio::test]
    async fn test_exponential_backoff_integration() {
        let registry = create_test_node_registry_with_db().await;

        // Verify backoff config
        assert_eq!(registry.config.db_max_attempts, 3);
        assert_eq!(registry.config.db_backoff_base, Duration::from_millis(10));
    }

    #[tokio::test]
    async fn test_swim_refute_self_suspicion() {
        let registry = create_test_node_registry().await;

        let initial = registry.swim().local_incarnation();

        // Process suspicion about self
        registry.swim().process_suspect("test-node", 0).await;

        // Incarnation should have increased (refutation)
        let new_incarnation = registry.swim().local_incarnation();
        assert!(new_incarnation > initial);
    }

    #[tokio::test]
    async fn test_swim_anti_entropy_merge() {
        let registry1 = create_test_node_registry().await;
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        // Registry1 knows about node-x
        let node_x = NodeRegistration {
            node_id: "node-x".to_string(),
            node_address: "http://localhost:9001".to_string(),
            ..Default::default()
        };
        registry1.register_node(&ctx, node_x).await.unwrap();

        // Get full state
        let state = registry1.swim().get_full_state().await;
        assert!(!state.is_empty());

        // Create registry2 and merge state
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));
        let mut config = NodeRegistryConfig::default();
        config.gossip_enabled = false;

        let registry2 = NodeRegistry::new(
            object_registry,
            "test-node-2".to_string(),
            "localhost:8001".to_string(),
            config,
        );

        registry2.swim().merge_full_state(state).await;

        // Registry2 should now know about node-x
        let member = registry2.swim().get_member("node-x").await;
        assert!(member.is_some());
    }

    // ============================================================
    // Proto Config Conversion Tests
    // ============================================================

    #[test]
    fn test_config_from_proto_defaults() {
        // When no proto config is provided, should use defaults
        let config = NodeRegistry::config_from_proto(None);

        assert_eq!(
            config.cache_ttl,
            Duration::from_secs(DEFAULT_CACHE_TTL_SECONDS)
        );
        assert!(config.gossip_enabled); // Default is true
        assert!(!config.use_shared_db); // Default is false
        assert_eq!(config.db_max_attempts, 10);
        assert_eq!(config.active_node_window, DEFAULT_ACTIVE_NODE_WINDOW);
    }

    #[test]
    fn test_config_from_proto_with_values() {
        use plexspaces_proto::node::v1::{
            DbBackoffConfig as ProtoDbBackoffConfig, NodeRegistryConfig as ProtoNodeRegistryConfig,
            SwimConfig as ProtoSwimConfig,
        };

        let proto_config = ProtoNodeRegistryConfig {
            cache_ttl_seconds: 120,
            gossip_enabled: true,
            swim: Some(ProtoSwimConfig {
                protocol_period_ms: 2000,
                probe_timeout_ms: 1000,
                indirect_ping_nodes: 5,
                suspicion_multiplier: 6,
                suspicion_min_ms: 5000,
                suspicion_max_ms: 60000,
                dead_node_reap_seconds: 600,
                max_piggyback_updates: 20,
                broadcast_limit: 10,
                anti_entropy_interval_seconds: 60,
            }),
            use_shared_db: true,
            db_sync_interval_seconds: 60,
            active_node_window_seconds: 7200,
            db_backoff: Some(ProtoDbBackoffConfig {
                base_delay_ms: 200,
                max_delay_ms: 60000,
                max_attempts: 5,
            }),
        };

        let config = NodeRegistry::config_from_proto(Some(&proto_config));

        // Cache config
        assert_eq!(config.cache_ttl, Duration::from_secs(120));

        // Gossip config
        assert!(config.gossip_enabled);

        // SWIM config
        assert_eq!(
            config.swim_config.protocol_period,
            Duration::from_millis(2000)
        );
        assert_eq!(
            config.swim_config.probe_timeout,
            Duration::from_millis(1000)
        );
        assert_eq!(config.swim_config.indirect_ping_nodes, 5);
        assert_eq!(config.swim_config.suspicion_mult, 6);
        assert_eq!(
            config.swim_config.suspicion_min,
            Duration::from_millis(5000)
        );
        assert_eq!(
            config.swim_config.suspicion_max,
            Duration::from_millis(60000)
        );
        assert_eq!(
            config.swim_config.dead_node_reap_timeout,
            Duration::from_secs(600)
        );
        assert_eq!(config.swim_config.max_piggyback_updates, 20);
        assert_eq!(config.swim_config.broadcast_limit, 10);
        assert_eq!(
            config.swim_config.anti_entropy_interval,
            Duration::from_secs(60)
        );

        // DB config
        assert!(config.use_shared_db);
        assert_eq!(config.db_sync_interval, Duration::from_secs(60));
        assert_eq!(config.active_node_window, Duration::from_secs(7200));
        assert_eq!(config.db_backoff_base, Duration::from_millis(200));
        assert_eq!(config.db_backoff_cap, Duration::from_millis(60000));
        assert_eq!(config.db_max_attempts, 5);
    }

    #[test]
    fn test_config_from_proto_partial() {
        use plexspaces_proto::node::v1::NodeRegistryConfig as ProtoNodeRegistryConfig;

        // Only set some fields - others should use defaults
        let proto_config = ProtoNodeRegistryConfig {
            cache_ttl_seconds: 30,
            gossip_enabled: false,
            ..Default::default()
        };

        let config = NodeRegistry::config_from_proto(Some(&proto_config));

        // Explicitly set values
        assert_eq!(config.cache_ttl, Duration::from_secs(30));
        assert!(!config.gossip_enabled);

        // Default values for unset fields
        assert!(!config.use_shared_db);
        assert_eq!(config.db_max_attempts, 10);
        assert_eq!(config.active_node_window, DEFAULT_ACTIVE_NODE_WINDOW);
    }

    #[test]
    fn test_swim_config_from_proto() {
        use plexspaces_proto::node::v1::SwimConfig as ProtoSwimConfig;

        let proto = ProtoSwimConfig {
            protocol_period_ms: 500,
            probe_timeout_ms: 250,
            indirect_ping_nodes: 2,
            suspicion_multiplier: 3,
            suspicion_min_ms: 1000,
            suspicion_max_ms: 15000,
            dead_node_reap_seconds: 120,
            max_piggyback_updates: 5,
            broadcast_limit: 3,
            anti_entropy_interval_seconds: 15,
        };

        let config = NodeRegistry::swim_config_from_proto(&proto);

        assert_eq!(config.protocol_period, Duration::from_millis(500));
        assert_eq!(config.probe_timeout, Duration::from_millis(250));
        assert_eq!(config.indirect_ping_nodes, 2);
        assert_eq!(config.suspicion_mult, 3);
        assert_eq!(config.suspicion_min, Duration::from_millis(1000));
        assert_eq!(config.suspicion_max, Duration::from_millis(15000));
        assert_eq!(config.dead_node_reap_timeout, Duration::from_secs(120));
        assert_eq!(config.max_piggyback_updates, 5);
        assert_eq!(config.broadcast_limit, 3);
        assert_eq!(config.anti_entropy_interval, Duration::from_secs(15));
    }

    #[test]
    fn test_config_from_proto_active_node_window() {
        let proto_config = plexspaces_proto::node::v1::NodeRegistryConfig {
            active_node_window_seconds: 7200,
            ..Default::default()
        };

        let config = NodeRegistry::config_from_proto(Some(&proto_config));
        assert_eq!(config.active_node_window, Duration::from_secs(7200));
    }

    #[tokio::test]
    async fn test_from_node_config() {
        use plexspaces_proto::node::v1::{
            NodeConfig, NodeRegistryConfig as ProtoNodeRegistryConfig,
            SwimConfig as ProtoSwimConfig,
        };

        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));

        let node_config = NodeConfig {
            id: "test-node-proto".to_string(),
            listen_addr: "0.0.0.0:8000".to_string(),
            grpc_address: "http://localhost:8000".to_string(),
            node_registry: Some(ProtoNodeRegistryConfig {
                cache_ttl_seconds: 90,
                gossip_enabled: true,
                swim: Some(ProtoSwimConfig {
                    protocol_period_ms: 1500,
                    indirect_ping_nodes: 4,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let registry = NodeRegistry::from_config(object_registry, &node_config);

        assert_eq!(registry.local_node_id, "test-node-proto");
        assert_eq!(registry.config.cache_ttl, Duration::from_secs(90));
        assert!(registry.config.gossip_enabled);
        assert_eq!(
            registry.config.swim_config.protocol_period,
            Duration::from_millis(1500)
        );
        assert_eq!(registry.config.swim_config.indirect_ping_nodes, 4);
    }
}
