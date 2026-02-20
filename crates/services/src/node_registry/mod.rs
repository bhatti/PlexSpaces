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
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use plexspaces_core::{NodeRegistryTrait, ObjectRegistry, RequestContext, ServiceLocator};
use plexspaces_proto::common::v1::Metadata as CommonMetadata;
use plexspaces_proto::node::v1::{NodeCapacity, NodeRegistration};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost_types::Timestamp;

pub use swim::{ExponentialBackoff, NodeState, SwimConfig, SwimMember, SwimProtocol};

/// Default cache TTL in seconds
const DEFAULT_CACHE_TTL_SECONDS: u64 = 60;

/// Default gossip interval in milliseconds
const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 1000;

/// Default gossip fanout (number of nodes to probe per round)
const DEFAULT_GOSSIP_FANOUT: usize = 3;

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
        config.cache_ttl = Duration::from_secs(cache_ttl_seconds.unwrap_or(DEFAULT_CACHE_TTL_SECONDS));
        config.gossip_enabled = gossip_enabled.unwrap_or(true);
        config.swim_config.protocol_period = Duration::from_millis(
            gossip_interval_ms.unwrap_or(DEFAULT_GOSSIP_INTERVAL_MS)
        );
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
                config.db_sync_interval = Duration::from_secs(proto.db_sync_interval_seconds as u64);
            }

            // DB backoff configuration
            if let Some(backoff_proto) = &proto.db_backoff {
                if backoff_proto.base_delay_ms > 0 {
                    config.db_backoff_base = Duration::from_millis(backoff_proto.base_delay_ms as u64);
                }
                if backoff_proto.max_delay_ms > 0 {
                    config.db_backoff_cap = Duration::from_millis(backoff_proto.max_delay_ms as u64);
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
            config.dead_node_reap_timeout = Duration::from_secs(proto.dead_node_reap_seconds as u64);
        }
        if proto.max_piggyback_updates > 0 {
            config.max_piggyback_updates = proto.max_piggyback_updates as usize;
        }
        if proto.broadcast_limit > 0 {
            config.broadcast_limit = proto.broadcast_limit;
        }
        if proto.anti_entropy_interval_seconds > 0 {
            config.anti_entropy_interval = Duration::from_secs(proto.anti_entropy_interval_seconds as u64);
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
        NodeRegistration {
            node_id: obj_reg.object_id.clone(),
            node_address: obj_reg.grpc_address.clone(),
            capabilities: obj_reg.capabilities.iter()
                .map(|c| (c.clone(), "true".to_string()))
                .collect(),
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
    fn to_object_registration(node_reg: &NodeRegistration, ctx: &RequestContext) -> ObjectRegistration {
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
            NodeState::Dead | NodeState::Left => plexspaces_proto::node::v1::NodeStatus::NodeStatusStopped as i32,
        };

        NodeRegistration {
            node_id: member.node_id.clone(),
            node_address: member.address.clone(),
            capabilities: member.metadata.clone(),
            status,
            last_heartbeat: member.last_probe_success.map(|t| {
                let elapsed = t.elapsed();
                let probe_time = std::time::SystemTime::now() - elapsed;
                let duration = probe_time.duration_since(std::time::SystemTime::UNIX_EPOCH)
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
    async fn sync_with_db(&self, ctx: &RequestContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.config.use_shared_db {
            return Ok(());
        }

        let object_registry = self.object_registry.clone();
        let ctx_clone = ctx.clone();

        let registrations = self.with_db_backoff("sync_discover", || {
            let registry = object_registry.clone();
            let ctx = ctx_clone.clone();
            async move {
                registry.discover(&ctx, Some(ObjectType::ObjectTypeNode), None, None, None, None, 0, 1000)
                    .await
                    .map_err(|e| format!("{}", e).into())
            }
        }).await?;

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

            let member = SwimMember::new(obj_reg.object_id.clone(), obj_reg.grpc_address.clone());
            // Note: ObjectRegistration.metadata is common::v1::Metadata (timestamps), not a HashMap
            // If we need to store custom metadata, use labels or metrics fields instead
            self.swim.upsert_member(member).await;
        }

        Ok(())
    }

    /// Run the SWIM protocol loop
    async fn run_swim_loop(
        swim: Arc<SwimProtocol>,
        cache: Arc<RwLock<HashMap<String, CachedNodeRegistration>>>,
        cache_ttl: Duration,
        service_locator: Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        running: Arc<AtomicBool>,
        config: SwimConfig,
    ) {
        let mut protocol_interval = tokio::time::interval(config.protocol_period);
        let mut anti_entropy_interval = tokio::time::interval(config.anti_entropy_interval);

        info!(
            "Starting SWIM protocol loop: period={:?}, indirect_nodes={}",
            config.protocol_period,
            config.indirect_ping_nodes
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
                        let probe_result = Self::probe_node(
                            &swim,
                            &target,
                            &service_locator,
                            &config,
                        ).await;

                        match probe_result {
                            ProbeResult::Alive => {
                                // Update cache with fresh data
                                let node_reg = Self::swim_member_to_node_registration(&target);
                                let mut cache_guard = cache.write().await;
                                cache_guard.insert(
                                    target.node_id.clone(),
                                    CachedNodeRegistration::new(node_reg, cache_ttl),
                                );
                            }
                            ProbeResult::Suspect => {
                                swim.suspect_member(&target.node_id).await;
                            }
                            ProbeResult::Failed => {
                                // Already handled in probe_node
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
        target: &SwimMember,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        config: &SwimConfig,
    ) -> ProbeResult {
        // Try direct ping first
        match Self::direct_ping(target, service_locator, config.probe_timeout).await {
            Ok(()) => {
                swim.process_alive(&target.node_id, target.incarnation, &target.address).await;
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
            swim.process_alive(&target.node_id, target.incarnation, &target.address).await;
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
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        use plexspaces_proto::node::v1::PingRequest;
        use plexspaces_core::grpc_connection_manager::ServiceType;

        let sl_guard = service_locator.read().await;
        let sl = sl_guard.as_ref()
            .ok_or_else(|| "ServiceLocator not available".to_string())?;

        let conn_manager = sl.get_grpc_connection_manager().await
            .ok_or_else(|| "GrpcConnectionManager not available".to_string())?;

        let channel = conn_manager.get_connection(
            ServiceType::NodeService,
            &target.node_id,
            &target.address,
        ).await
            .map_err(|e| format!("Failed to get channel: {}", e))?;

        let mut client = NodeServiceClient::new(channel);
        
        let request = tonic::Request::new(PingRequest {
            source_node_id: String::new(), // Will be set by sender
            sequence_number: 0,
            updates: Vec::new(),
        });

        tokio::time::timeout(timeout, client.ping(request))
            .await
            .map_err(|_| "Ping timeout")?
            .map_err(|e| format!("Ping failed: {}", e))?;

        Ok(())
    }

    /// Indirect ping (ask intermediary to ping target)
    async fn indirect_ping(
        intermediary: &SwimMember,
        target: &SwimMember,
        service_locator: &Arc<RwLock<Option<Arc<dyn ServiceLocator>>>>,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
        use plexspaces_proto::node::v1::PingReqRequest;
        use plexspaces_core::grpc_connection_manager::ServiceType;

        let sl_guard = service_locator.read().await;
        let sl = sl_guard.as_ref()
            .ok_or_else(|| "ServiceLocator not available".to_string())?;

        let conn_manager = sl.get_grpc_connection_manager().await
            .ok_or_else(|| "GrpcConnectionManager not available".to_string())?;

        let channel = conn_manager.get_connection(
            ServiceType::NodeService,
            &intermediary.node_id,
            &intermediary.address,
        ).await
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
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<Option<NodeRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        // Check SWIM first (most up-to-date)
        if let Some(member) = self.swim.get_member(node_id).await {
            if member.state.is_active() {
                let node_reg = Self::swim_member_to_node_registration(&member);
                return Ok(Some(node_reg));
            }
        }

        // Check cache
        if let Some(registration) = self.get_from_cache(node_id).await {
            return Ok(Some(registration));
        }

        // Cache miss - lookup in ObjectRegistry with backoff
        if self.config.use_shared_db {
            let object_registry = self.object_registry.clone();
            let node_id_owned = node_id.to_string();
            let ctx_clone = ctx.clone();

            let result = self.with_db_backoff("lookup", || {
                let registry = object_registry.clone();
                let ctx = ctx_clone.clone();
                let nid = node_id_owned.clone();
                async move {
                    registry.lookup_full(&ctx, ObjectType::ObjectTypeNode, &nid)
                        .await
                        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{}", e).into() })
                }
            }).await;

            match result {
                Ok(Some(obj_reg)) => {
                    let node_reg = Self::to_node_registration(&obj_reg);
                    self.update_cache(node_id, node_reg.clone()).await;
                    
                    // Also update SWIM
                    let member = SwimMember::new(
                        obj_reg.object_id.clone(),
                        obj_reg.grpc_address.clone(),
                    );
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
        ctx: &RequestContext,
        registration: NodeRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let node_id = registration.node_id.clone();

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

        // Persist to DB with backoff (if enabled)
        if self.config.use_shared_db {
            let obj_reg = Self::to_object_registration(&registration, ctx);
            let object_registry = self.object_registry.clone();
            let ctx_clone = ctx.clone();

            if let Err(e) = self.with_db_backoff("register", || {
                let registry = object_registry.clone();
                let ctx = ctx_clone.clone();
                let reg = obj_reg.clone();
                async move {
                    registry.register(&ctx, reg)
                        .await
                        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{}", e).into() })
                }
            }).await {
                warn!("Failed to persist node registration to DB: {}", e);
                // Continue anyway - SWIM will propagate
            }
        }

        info!("Registered node: {}", node_id);
        metrics::counter!("plexspaces_node_registry_registrations_total").increment(1);

        Ok(())
    }

    async fn unregister_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Update SWIM - mark as left
        self.swim.declare_dead(node_id).await;

        // Remove from cache
        self.remove_from_cache(node_id).await;

        // Remove from DB with backoff (if enabled)
        if self.config.use_shared_db {
            let object_registry = self.object_registry.clone();
            let node_id_owned = node_id.to_string();
            let ctx_clone = ctx.clone();

            if let Err(e) = self.with_db_backoff("unregister", || {
                let registry = object_registry.clone();
                let ctx = ctx_clone.clone();
                let nid = node_id_owned.clone();
                async move {
                    registry.unregister(&ctx, ObjectType::ObjectTypeNode, &nid)
                        .await
                        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{}", e).into() })
                }
            }).await {
                warn!("Failed to remove node from DB: {}", e);
            }
        }

        info!("Unregistered node: {}", node_id);
        metrics::counter!("plexspaces_node_registry_unregistrations_total").increment(1);

        Ok(())
    }

    async fn list_nodes(
        &self,
        ctx: &RequestContext,
        cluster: Option<&str>,
        page_size: u32,
        _page_token: &str,
    ) -> Result<(Vec<NodeRegistration>, String), Box<dyn std::error::Error + Send + Sync>> {
        // Get active members from SWIM (most authoritative)
        let mut nodes: Vec<NodeRegistration> = self.swim.active_members().await
            .into_iter()
            .filter(|m| {
                if let Some(cluster_name) = cluster {
                    m.metadata.get("cluster") == Some(&cluster_name.to_string())
                } else {
                    true
                }
            })
            .map(|m| Self::swim_member_to_node_registration(&m))
            .collect();

        // If SWIM is empty and we have DB, sync from DB
        if nodes.is_empty() && self.config.use_shared_db {
            self.sync_with_db(ctx).await?;
            
            // Try again from cache
            self.evict_expired().await;
            let cache = self.cache.read().await;
            nodes = cache.values()
                .filter(|e| !e.is_expired())
                .filter(|e| {
                    if let Some(cluster_name) = cluster {
                        e.registration.capabilities.get("cluster") == Some(&cluster_name.to_string())
                    } else {
                        true
                    }
                })
                .map(|e| e.registration.clone())
                .collect();
        }

        let limit = if page_size > 0 { page_size as usize } else { nodes.len() };
        let result: Vec<NodeRegistration> = nodes.into_iter().take(limit).collect();

        Ok((result, String::new()))
    }

    async fn send_heartbeat(
        &self,
        ctx: &RequestContext,
        node_id: &str,
        capacity: Option<NodeCapacity>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Update SWIM member status
        if let Some(mut member) = self.swim.get_member(node_id).await {
            member.record_probe_success();
            self.swim.upsert_member(member).await;
        }

        // Update DB if enabled
        if self.config.use_shared_db {
            let object_registry = self.object_registry.clone();
            let node_id_owned = node_id.to_string();
            let ctx_clone = ctx.clone();
            let capacity_clone = capacity.clone();

            if let Err(e) = self.with_db_backoff("heartbeat", || {
                let registry = object_registry.clone();
                let ctx = ctx_clone.clone();
                let nid = node_id_owned.clone();
                let cap = capacity_clone.clone();
                async move {
                    let obj_reg = registry.lookup_full(&ctx, ObjectType::ObjectTypeNode, &nid)
                        .await
                        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{}", e).into() })?;

                    if let Some(mut obj_reg) = obj_reg {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default();
                        let timestamp = Timestamp {
                            seconds: now.as_secs() as i64,
                            nanos: now.subsec_nanos() as i32,
                        };

                        obj_reg.last_heartbeat = Some(timestamp.clone());
                        obj_reg.updated_at = Some(timestamp);
                        obj_reg.health_status = HealthStatus::HealthStatusHealthy as i32;

                        if let Some(ref cap) = cap {
                            if let Some(ref total) = cap.total {
                                // Store capacity info in metrics (which is a HashMap<String, f64>)
                                obj_reg.metrics.insert("total_cpu_cores".to_string(), total.cpu_cores as f64);
                                obj_reg.metrics.insert("total_memory_bytes".to_string(), total.memory_bytes as f64);
                            }
                        }

                        registry.register(&ctx, obj_reg).await
                            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { format!("{}", e).into() })?;
                    }
                    Ok(())
                }
            }).await {
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
        let service_locator = self.service_locator.clone();
        let running = self.running.clone();
        let config = self.config.swim_config.clone();

        tokio::spawn(async move {
            Self::run_swim_loop(
                swim,
                cache,
                cache_ttl,
                service_locator,
                running,
                config,
            ).await;
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

    async fn create_test_node_registry() -> NodeRegistry {
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
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
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "node-1".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };

        registry.register_node(&ctx, node_reg.clone()).await.unwrap();

        let result = registry.lookup_node(&ctx, "node-1").await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().node_id, "node-1");
    }

    #[tokio::test]
    async fn test_unregister_node() {
        let registry = create_test_node_registry().await;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "node-heartbeat".to_string(),
            node_address: "http://localhost:8099".to_string(),
            ..Default::default()
        };

        registry.register_node(&ctx, node_reg).await.unwrap();
        registry.send_heartbeat(&ctx, "node-heartbeat", None).await.unwrap();

        let result = registry.lookup_node(&ctx, "node-heartbeat").await.unwrap();
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_swim_integration() {
        let registry = create_test_node_registry().await;
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        let node_reg = NodeRegistration {
            node_id: "db-node-1".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };

        // Register should persist to DB
        registry.register_node(&ctx, node_reg.clone()).await.unwrap();

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        // Add nodes to different clusters
        let mut node1 = NodeRegistration {
            node_id: "cluster-a-node".to_string(),
            node_address: "http://localhost:8001".to_string(),
            ..Default::default()
        };
        node1.capabilities.insert("cluster".to_string(), "cluster-a".to_string());
        registry.register_node(&ctx, node1).await.unwrap();

        let mut node2 = NodeRegistration {
            node_id: "cluster-b-node".to_string(),
            node_address: "http://localhost:8002".to_string(),
            ..Default::default()
        };
        node2.capabilities.insert("cluster".to_string(), "cluster-b".to_string());
        registry.register_node(&ctx, node2).await.unwrap();

        // List all
        let (all_nodes, _) = registry.list_nodes(&ctx, None, 10, "").await.unwrap();
        assert_eq!(all_nodes.len(), 2);

        // List cluster-a only
        let (cluster_a_nodes, _) = registry.list_nodes(&ctx, Some("cluster-a"), 10, "").await.unwrap();
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
        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

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
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
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
        
        assert_eq!(config.cache_ttl, Duration::from_secs(DEFAULT_CACHE_TTL_SECONDS));
        assert!(config.gossip_enabled); // Default is true
        assert!(!config.use_shared_db); // Default is false
        assert_eq!(config.db_max_attempts, 10);
    }

    #[test]
    fn test_config_from_proto_with_values() {
        use plexspaces_proto::node::v1::{
            NodeRegistryConfig as ProtoNodeRegistryConfig,
            SwimConfig as ProtoSwimConfig,
            DbBackoffConfig as ProtoDbBackoffConfig,
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
        assert_eq!(config.swim_config.protocol_period, Duration::from_millis(2000));
        assert_eq!(config.swim_config.probe_timeout, Duration::from_millis(1000));
        assert_eq!(config.swim_config.indirect_ping_nodes, 5);
        assert_eq!(config.swim_config.suspicion_mult, 6);
        assert_eq!(config.swim_config.suspicion_min, Duration::from_millis(5000));
        assert_eq!(config.swim_config.suspicion_max, Duration::from_millis(60000));
        assert_eq!(config.swim_config.dead_node_reap_timeout, Duration::from_secs(600));
        assert_eq!(config.swim_config.max_piggyback_updates, 20);
        assert_eq!(config.swim_config.broadcast_limit, 10);
        assert_eq!(config.swim_config.anti_entropy_interval, Duration::from_secs(60));
        
        // DB config
        assert!(config.use_shared_db);
        assert_eq!(config.db_sync_interval, Duration::from_secs(60));
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

    #[tokio::test]
    async fn test_from_node_config() {
        use plexspaces_proto::node::v1::{
            NodeConfig,
            NodeRegistryConfig as ProtoNodeRegistryConfig,
            SwimConfig as ProtoSwimConfig,
        };

        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
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
        assert_eq!(registry.config.swim_config.protocol_period, Duration::from_millis(1500));
        assert_eq!(registry.config.swim_config.indirect_ping_nodes, 4);
    }
}
