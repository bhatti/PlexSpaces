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

//! SWIM Protocol Implementation
//!
//! ## Overview
//! Implementation of the SWIM (Scalable Weakly-consistent
//! Infection-style Process Group Membership) protocol for robust node discovery.
//!
//! ## Key Features
//! - **Direct Ping**: Primary failure detection via direct probing
//! - **Indirect Ping (Ping-Req)**: Fallback probing through intermediary nodes
//! - **Suspicion Mechanism**: Reduces false positives with suspicion timeout
//! - **Incarnation Numbers**: Handles network partitions and rejoins correctly
//! - **Piggybacking**: Efficient membership update dissemination
//! - **Anti-Entropy**: Periodic full state synchronization
//!
//! ## Performance Characteristics
//! - O(1) membership lookups via HashMap
//! - O(log n) failure detection convergence
//! - Bounded memory usage with configurable limits
//! - Lock-free metrics using atomics
//!
//! ## Observability
//! - Prometheus-compatible metrics via `metrics` crate
//! - Structured logging via `tracing`
//! - Health status reporting
//!
//! ## References
//! - SWIM: Scalable Weakly-consistent Infection-style Process Group Membership Protocol
//!   (Das, Gupta, Motivala - 2002)
//! - Lifeguard: Local Health Awareness for More Accurate Failure Detection
//!   (Hashimoto - 2017, used in HashiCorp Serf/Consul)

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, trace, warn};

// ============================================================================
// Metrics Keys - Centralized for consistency
// ============================================================================

/// Metric: Total members in cluster
const METRIC_MEMBERS_TOTAL: &str = "plexspaces_swim_members_total";
/// Metric: Direct pings sent
const METRIC_DIRECT_PINGS: &str = "plexspaces_swim_direct_pings_total";
/// Metric: Direct ping successes
const METRIC_DIRECT_PING_SUCCESS: &str = "plexspaces_swim_direct_ping_success_total";
/// Metric: Direct ping failures
const METRIC_DIRECT_PING_FAILED: &str = "plexspaces_swim_direct_ping_failed_total";
/// Metric: Indirect pings sent
const METRIC_INDIRECT_PINGS: &str = "plexspaces_swim_indirect_pings_total";
/// Metric: Indirect ping successes
const METRIC_INDIRECT_PING_SUCCESS: &str = "plexspaces_swim_indirect_ping_success_total";
/// Metric: Indirect ping failures
const METRIC_INDIRECT_PING_FAILED: &str = "plexspaces_swim_indirect_ping_failed_total";
/// Metric: Suspicions raised
const METRIC_SUSPICIONS: &str = "plexspaces_swim_suspicions_total";
/// Metric: Nodes declared dead
const METRIC_DEATHS: &str = "plexspaces_swim_deaths_total";
/// Metric: Nodes that joined
const METRIC_JOINS: &str = "plexspaces_swim_joins_total";
/// Metric: Nodes that left
const METRIC_LEAVES: &str = "plexspaces_swim_leaves_total";
/// Metric: Nodes reaped (removed after dead timeout)
const METRIC_REAPS: &str = "plexspaces_swim_reaps_total";
/// Metric: Incarnation refutations
const METRIC_REFUTATIONS: &str = "plexspaces_swim_refutations_total";
/// Metric: Membership updates broadcast
const METRIC_UPDATES_BROADCAST: &str = "plexspaces_swim_updates_broadcast_total";
/// Metric: Membership updates received
const METRIC_UPDATES_RECEIVED: &str = "plexspaces_swim_updates_received_total";
/// Metric: Anti-entropy syncs performed
const METRIC_ANTI_ENTROPY_SYNCS: &str = "plexspaces_swim_anti_entropy_syncs_total";
/// Metric: Protocol round duration
const METRIC_ROUND_DURATION: &str = "plexspaces_swim_round_duration_seconds";
/// Metric: Probe latency histogram
const METRIC_PROBE_LATENCY: &str = "plexspaces_swim_probe_latency_seconds";

// ============================================================================
// Node State
// ============================================================================

/// Node membership state in SWIM protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeState {
    /// Node is healthy and responding
    Alive,
    /// Node failed direct ping, being verified via indirect ping
    Suspect,
    /// Node confirmed dead after suspicion timeout
    Dead,
    /// Node gracefully left the cluster
    Left,
}

impl NodeState {
    /// Check if node is considered active (can receive messages)
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self, NodeState::Alive | NodeState::Suspect)
    }

    /// Convert to string for metrics/logging
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeState::Alive => "alive",
            NodeState::Suspect => "suspect",
            NodeState::Dead => "dead",
            NodeState::Left => "left",
        }
    }

    /// Convert to numeric value for ordering (used in state resolution)
    #[inline]
    fn priority(&self) -> u8 {
        match self {
            NodeState::Alive => 0,
            NodeState::Suspect => 1,
            NodeState::Dead => 2,
            NodeState::Left => 3,
        }
    }
}

impl std::fmt::Display for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// SWIM Member
// ============================================================================

/// SWIM cluster member with incarnation number for consistency
///
/// ## Incarnation Numbers
/// Incarnation numbers provide consistency in distributed membership:
/// - Each node starts with incarnation 0
/// - When a node refutes suspicion about itself, it increments its incarnation
/// - Higher incarnation always wins for the same node
/// - This prevents stale information from overriding fresh information
#[derive(Debug, Clone)]
pub struct SwimMember {
    /// Unique node identifier
    pub node_id: String,
    /// gRPC address for communication
    pub address: String,
    /// Current membership state
    pub state: NodeState,
    /// Incarnation number - monotonically increasing, used to resolve conflicts
    pub incarnation: u64,
    /// When state last changed
    pub state_changed_at: Instant,
    /// Consecutive failed probes (for Lifeguard protocol)
    pub failed_probes: u32,
    /// Last successful probe time
    pub last_probe_success: Option<Instant>,
    /// Metadata (capabilities, cluster, etc.)
    pub metadata: HashMap<String, String>,
}

impl SwimMember {
    /// Create a new alive member
    pub fn new(node_id: String, address: String) -> Self {
        Self {
            node_id,
            address,
            state: NodeState::Alive,
            incarnation: 0,
            state_changed_at: Instant::now(),
            failed_probes: 0,
            last_probe_success: Some(Instant::now()),
            metadata: HashMap::new(),
        }
    }

    /// Create member with metadata
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Check if member should be removed (dead for too long)
    #[inline]
    pub fn should_reap(&self, reap_timeout: Duration) -> bool {
        matches!(self.state, NodeState::Dead | NodeState::Left)
            && self.state_changed_at.elapsed() > reap_timeout
    }

    /// Update state with proper incarnation handling
    ///
    /// ## SWIM Rules
    /// 1. Higher incarnation always wins
    /// 2. Same incarnation: more severe state wins (Alive < Suspect < Dead < Left)
    /// 3. Lower incarnation is ignored
    ///
    /// Returns true if state was updated
    #[instrument(skip(self), fields(node_id = %self.node_id))]
    pub fn update_state(&mut self, new_state: NodeState, new_incarnation: u64) -> bool {
        // Rule 1: Higher incarnation always wins
        if new_incarnation > self.incarnation {
            let old_state = self.state;
            self.incarnation = new_incarnation;
            if self.state != new_state {
                self.state = new_state;
                self.state_changed_at = Instant::now();
                trace!(
                    old_state = %old_state,
                    new_state = %new_state,
                    incarnation = new_incarnation,
                    "State updated (higher incarnation)"
                );
            }
            return true;
        }

        // Rule 2: Same incarnation - more severe state wins
        if new_incarnation == self.incarnation && new_state.priority() > self.state.priority() {
            let old_state = self.state;
            self.state = new_state;
            self.state_changed_at = Instant::now();
            trace!(
                old_state = %old_state,
                new_state = %new_state,
                incarnation = new_incarnation,
                "State updated (same incarnation, higher priority)"
            );
            return true;
        }

        // Rule 3: Lower incarnation or lower priority - ignore
        false
    }

    /// Refute suspicion by incrementing incarnation (node proving it's alive)
    #[instrument(skip(self), fields(node_id = %self.node_id))]
    pub fn refute(&mut self) {
        self.incarnation += 1;
        self.state = NodeState::Alive;
        self.state_changed_at = Instant::now();
        self.failed_probes = 0;
        debug!(incarnation = self.incarnation, "Refuted suspicion");
        metrics::counter!(METRIC_REFUTATIONS).increment(1);
    }

    /// Record successful probe
    #[inline]
    pub fn record_probe_success(&mut self) {
        self.failed_probes = 0;
        self.last_probe_success = Some(Instant::now());
        if self.state == NodeState::Suspect {
            self.state = NodeState::Alive;
            self.state_changed_at = Instant::now();
        }
    }

    /// Record failed probe
    #[inline]
    pub fn record_probe_failure(&mut self) {
        self.failed_probes = self.failed_probes.saturating_add(1);
    }

    /// Get time since last successful probe
    pub fn time_since_last_success(&self) -> Option<Duration> {
        self.last_probe_success.map(|t| t.elapsed())
    }
}

// ============================================================================
// Exponential Backoff with Jitter
// ============================================================================

/// Exponential backoff with decorrelated jitter for DB operations
///
/// ## Algorithm
/// Uses decorrelated jitter from AWS Architecture Blog:
/// `sleep = min(cap, random_between(base, sleep * 3))`
///
/// This provides better distribution than full jitter while maintaining
/// the exponential backoff property.
///
/// ## Performance
/// - O(1) time and space per operation
/// - No allocations
/// - Thread-safe (can be cloned)
#[derive(Debug, Clone)]
pub struct ExponentialBackoff {
    /// Base delay
    base: Duration,
    /// Maximum delay cap
    cap: Duration,
    /// Current sleep duration
    current: Duration,
    /// Current attempt number
    attempt: u32,
    /// Maximum attempts (0 = unlimited)
    max_attempts: u32,
}

impl ExponentialBackoff {
    /// Create new backoff with defaults suitable for DB operations
    ///
    /// Defaults:
    /// - Base: 100ms
    /// - Cap: 30s
    /// - Max attempts: 10
    pub fn new() -> Self {
        Self {
            base: Duration::from_millis(100),
            cap: Duration::from_secs(30),
            current: Duration::from_millis(100),
            attempt: 0,
            max_attempts: 10,
        }
    }

    /// Create with custom parameters
    pub fn with_params(base: Duration, cap: Duration, max_attempts: u32) -> Self {
        Self {
            base,
            cap,
            current: base,
            attempt: 0,
            max_attempts,
        }
    }

    /// Get next backoff duration with decorrelated jitter
    ///
    /// Returns None if max attempts exceeded
    pub fn next_backoff(&mut self) -> Option<Duration> {
        if self.max_attempts > 0 && self.attempt >= self.max_attempts {
            return None;
        }

        self.attempt += 1;

        // Decorrelated jitter: sleep = min(cap, random_between(base, sleep * 3))
        let mut rng = rand::thread_rng();
        let min_sleep = self.base.as_millis() as u64;
        let max_sleep = (self.current.as_millis() as u64).saturating_mul(3);

        let jittered = if max_sleep > min_sleep {
            rng.gen_range(min_sleep..=max_sleep)
        } else {
            min_sleep
        };

        self.current = Duration::from_millis(jittered).min(self.cap);
        Some(self.current)
    }

    /// Reset backoff state
    #[inline]
    pub fn reset(&mut self) {
        self.current = self.base;
        self.attempt = 0;
    }

    /// Get current attempt number
    #[inline]
    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// Check if max attempts reached
    #[inline]
    pub fn is_exhausted(&self) -> bool {
        self.max_attempts > 0 && self.attempt >= self.max_attempts
    }
}

impl Default for ExponentialBackoff {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Membership Update
// ============================================================================

/// Membership update for piggybacking on protocol messages
///
/// ## Piggybacking
/// SWIM piggybacks membership updates on protocol messages for efficient
/// dissemination. Each update is broadcast a limited number of times
/// (typically λ * log(n) where n = cluster size).
#[derive(Debug, Clone)]
pub struct MembershipUpdate {
    /// Node ID this update is about
    pub node_id: String,
    /// New state
    pub state: NodeState,
    /// Incarnation number
    pub incarnation: u64,
    /// Address (if known)
    pub address: Option<String>,
    /// When this update was created
    pub created_at: Instant,
    /// How many times this has been broadcast
    pub broadcast_count: u32,
}

impl MembershipUpdate {
    /// Create new update
    pub fn new(node_id: String, state: NodeState, incarnation: u64) -> Self {
        Self {
            node_id,
            state,
            incarnation,
            address: None,
            created_at: Instant::now(),
            broadcast_count: 0,
        }
    }

    /// Create update with address
    pub fn with_address(mut self, address: String) -> Self {
        self.address = Some(address);
        self
    }

    /// Check if update should be dropped (broadcast enough times)
    #[inline]
    pub fn should_drop(&self, max_broadcasts: u32) -> bool {
        self.broadcast_count >= max_broadcasts
    }

    /// Get age of this update
    #[inline]
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }
}

// ============================================================================
// SWIM Configuration
// ============================================================================

/// SWIM protocol configuration
///
/// ## Tuning Guidelines
/// - **Small clusters (<10 nodes)**: Use defaults
/// - **Medium clusters (10-100 nodes)**: Increase suspicion_mult to 5-6
/// - **Large clusters (100+ nodes)**: Increase indirect_ping_nodes to 4-5
/// - **Unstable networks**: Increase probe_timeout and suspicion times
#[derive(Debug, Clone)]
pub struct SwimConfig {
    /// Protocol period (how often to probe a random node)
    pub protocol_period: Duration,
    /// Direct probe timeout
    pub probe_timeout: Duration,
    /// Number of nodes to use for indirect ping
    pub indirect_ping_nodes: usize,
    /// Suspicion timeout multiplier (multiplied by log(n) * period)
    pub suspicion_mult: u32,
    /// Minimum suspicion timeout
    pub suspicion_min: Duration,
    /// Maximum suspicion timeout
    pub suspicion_max: Duration,
    /// How long to keep dead nodes before reaping
    pub dead_node_reap_timeout: Duration,
    /// Maximum number of membership updates to piggyback
    pub max_piggyback_updates: usize,
    /// How many times to broadcast an update before dropping
    pub broadcast_limit: u32,
    /// Anti-entropy sync interval
    pub anti_entropy_interval: Duration,
    /// DB fallback backoff base
    pub db_backoff_base: Duration,
    /// DB fallback backoff cap
    pub db_backoff_cap: Duration,
    /// DB fallback max attempts per operation
    pub db_max_attempts: u32,
}

impl Default for SwimConfig {
    fn default() -> Self {
        Self {
            protocol_period: Duration::from_secs(1),
            probe_timeout: Duration::from_millis(500),
            indirect_ping_nodes: 3,
            suspicion_mult: 4,
            suspicion_min: Duration::from_secs(3),
            suspicion_max: Duration::from_secs(30),
            dead_node_reap_timeout: Duration::from_secs(300), // 5 minutes
            max_piggyback_updates: 10,
            broadcast_limit: 5,
            anti_entropy_interval: Duration::from_secs(30),
            db_backoff_base: Duration::from_millis(100),
            db_backoff_cap: Duration::from_secs(30),
            db_max_attempts: 10,
        }
    }
}

impl SwimConfig {
    /// Calculate suspicion timeout based on cluster size
    ///
    /// Formula: suspicion_mult * log(n + 1) * probe_interval
    /// Bounded by [suspicion_min, suspicion_max]
    #[inline]
    pub fn suspicion_timeout(&self, cluster_size: usize) -> Duration {
        let log_n = ((cluster_size + 1) as f64).ln().max(1.0);
        let timeout_ms =
            self.suspicion_mult as f64 * log_n * self.protocol_period.as_millis() as f64;

        Duration::from_millis(timeout_ms as u64)
            .max(self.suspicion_min)
            .min(self.suspicion_max)
    }

    /// Calculate broadcast limit based on cluster size
    ///
    /// Formula: broadcast_limit * log(n + 1)
    #[inline]
    pub fn effective_broadcast_limit(&self, cluster_size: usize) -> u32 {
        let log_n = ((cluster_size + 1) as f64).ln().max(1.0);
        (self.broadcast_limit as f64 * log_n).ceil() as u32
    }
}

// ============================================================================
// SWIM Protocol Statistics
// ============================================================================

/// Statistics for SWIM protocol operation
#[derive(Debug, Default)]
pub struct SwimStats {
    /// Total direct pings sent
    pub direct_pings: AtomicU64,
    /// Successful direct pings
    pub direct_ping_success: AtomicU64,
    /// Failed direct pings
    pub direct_ping_failed: AtomicU64,
    /// Total indirect pings sent
    pub indirect_pings: AtomicU64,
    /// Successful indirect pings
    pub indirect_ping_success: AtomicU64,
    /// Failed indirect pings
    pub indirect_ping_failed: AtomicU64,
    /// Suspicions raised
    pub suspicions: AtomicU64,
    /// Nodes declared dead
    pub deaths: AtomicU64,
    /// Nodes that joined
    pub joins: AtomicU64,
    /// Nodes that left
    pub leaves: AtomicU64,
    /// Nodes reaped
    pub reaps: AtomicU64,
    /// Incarnation refutations
    pub refutations: AtomicU64,
    /// Updates broadcast
    pub updates_broadcast: AtomicU64,
    /// Updates received
    pub updates_received: AtomicU64,
    /// Anti-entropy syncs
    pub anti_entropy_syncs: AtomicU64,
    /// Protocol rounds completed
    pub rounds_completed: AtomicU64,
}

impl SwimStats {
    /// Create new stats instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Report all stats to metrics
    pub fn report_metrics(&self) {
        metrics::gauge!(METRIC_DIRECT_PINGS).set(self.direct_pings.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_DIRECT_PING_SUCCESS)
            .set(self.direct_ping_success.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_DIRECT_PING_FAILED)
            .set(self.direct_ping_failed.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_INDIRECT_PINGS)
            .set(self.indirect_pings.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_INDIRECT_PING_SUCCESS)
            .set(self.indirect_ping_success.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_INDIRECT_PING_FAILED)
            .set(self.indirect_ping_failed.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_SUSPICIONS).set(self.suspicions.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_DEATHS).set(self.deaths.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_JOINS).set(self.joins.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_LEAVES).set(self.leaves.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_REAPS).set(self.reaps.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_REFUTATIONS).set(self.refutations.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_UPDATES_BROADCAST)
            .set(self.updates_broadcast.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_UPDATES_RECEIVED)
            .set(self.updates_received.load(Ordering::Relaxed) as f64);
        metrics::gauge!(METRIC_ANTI_ENTROPY_SYNCS)
            .set(self.anti_entropy_syncs.load(Ordering::Relaxed) as f64);
    }

    /// Get snapshot of current stats
    pub fn snapshot(&self) -> SwimStatsSnapshot {
        SwimStatsSnapshot {
            direct_pings: self.direct_pings.load(Ordering::Relaxed),
            direct_ping_success: self.direct_ping_success.load(Ordering::Relaxed),
            direct_ping_failed: self.direct_ping_failed.load(Ordering::Relaxed),
            indirect_pings: self.indirect_pings.load(Ordering::Relaxed),
            indirect_ping_success: self.indirect_ping_success.load(Ordering::Relaxed),
            indirect_ping_failed: self.indirect_ping_failed.load(Ordering::Relaxed),
            suspicions: self.suspicions.load(Ordering::Relaxed),
            deaths: self.deaths.load(Ordering::Relaxed),
            joins: self.joins.load(Ordering::Relaxed),
            leaves: self.leaves.load(Ordering::Relaxed),
            reaps: self.reaps.load(Ordering::Relaxed),
            refutations: self.refutations.load(Ordering::Relaxed),
            updates_broadcast: self.updates_broadcast.load(Ordering::Relaxed),
            updates_received: self.updates_received.load(Ordering::Relaxed),
            anti_entropy_syncs: self.anti_entropy_syncs.load(Ordering::Relaxed),
            rounds_completed: self.rounds_completed.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of SWIM stats (non-atomic, for reporting)
#[derive(Debug, Clone, Default)]
pub struct SwimStatsSnapshot {
    pub direct_pings: u64,
    pub direct_ping_success: u64,
    pub direct_ping_failed: u64,
    pub indirect_pings: u64,
    pub indirect_ping_success: u64,
    pub indirect_ping_failed: u64,
    pub suspicions: u64,
    pub deaths: u64,
    pub joins: u64,
    pub leaves: u64,
    pub reaps: u64,
    pub refutations: u64,
    pub updates_broadcast: u64,
    pub updates_received: u64,
    pub anti_entropy_syncs: u64,
    pub rounds_completed: u64,
}

// ============================================================================
// SWIM Protocol
// ============================================================================

/// SWIM Protocol state machine
///
/// ## Thread Safety
/// All operations are thread-safe. Members are protected by RwLock for
/// concurrent reads. Statistics use atomic counters for lock-free updates.
///
/// ## Performance
/// - Membership lookup: O(1) average
/// - Member iteration: O(n)
/// - Update processing: O(1) per update
pub struct SwimProtocol {
    /// Local node ID
    local_node_id: String,
    /// Local node address
    local_address: String,
    /// Local incarnation number (atomic for lock-free updates)
    local_incarnation: AtomicU64,
    /// Known members (RwLock for concurrent access)
    members: Arc<RwLock<HashMap<String, SwimMember>>>,
    /// Pending membership updates to broadcast
    updates_queue: Arc<RwLock<Vec<MembershipUpdate>>>,
    /// Configuration
    config: SwimConfig,
    /// Probe sequence number (for round-robin probing)
    probe_sequence: AtomicU64,
    /// Protocol running flag
    running: AtomicBool,
    /// Statistics (lock-free atomics)
    stats: Arc<SwimStats>,
}

impl SwimProtocol {
    /// Create new SWIM protocol instance
    pub fn new(local_node_id: String, local_address: String, config: SwimConfig) -> Self {
        info!(
            node_id = %local_node_id,
            address = %local_address,
            protocol_period_ms = config.protocol_period.as_millis(),
            indirect_nodes = config.indirect_ping_nodes,
            "Creating SWIM protocol instance"
        );

        Self {
            local_node_id,
            local_address,
            local_incarnation: AtomicU64::new(0),
            members: Arc::new(RwLock::new(HashMap::new())),
            updates_queue: Arc::new(RwLock::new(Vec::new())),
            config,
            probe_sequence: AtomicU64::new(0),
            running: AtomicBool::new(false),
            stats: Arc::new(SwimStats::new()),
        }
    }

    /// Get local node ID
    #[inline]
    pub fn local_node_id(&self) -> &str {
        &self.local_node_id
    }

    /// Get local address
    #[inline]
    pub fn local_address(&self) -> &str {
        &self.local_address
    }

    /// Get local incarnation
    #[inline]
    pub fn local_incarnation(&self) -> u64 {
        self.local_incarnation.load(Ordering::SeqCst)
    }

    /// Increment and get local incarnation (used when refuting suspicion)
    pub fn increment_incarnation(&self) -> u64 {
        let new_inc = self.local_incarnation.fetch_add(1, Ordering::SeqCst) + 1;
        self.stats.refutations.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_REFUTATIONS).increment(1);
        new_inc
    }

    /// Get configuration
    #[inline]
    pub fn config(&self) -> &SwimConfig {
        &self.config
    }

    /// Get statistics
    #[inline]
    pub fn stats(&self) -> &Arc<SwimStats> {
        &self.stats
    }

    /// Add or update a member
    #[instrument(skip(self, member), fields(member_id = %member.node_id))]
    pub async fn upsert_member(&self, member: SwimMember) {
        let should_queue_update;
        let update_data;

        {
            let mut members = self.members.write().await;

            if let Some(existing) = members.get_mut(&member.node_id) {
                // Only update if new info has higher incarnation
                if member.incarnation > existing.incarnation {
                    debug!(
                        old_incarnation = existing.incarnation,
                        new_incarnation = member.incarnation,
                        "Updating member with higher incarnation"
                    );
                    *existing = member.clone();
                    should_queue_update = true;
                    update_data = Some((
                        member.node_id.clone(),
                        member.state,
                        member.incarnation,
                        member.address.clone(),
                    ));
                } else {
                    should_queue_update = false;
                    update_data = None;
                }
            } else {
                // New member
                info!(
                    node_id = %member.node_id,
                    address = %member.address,
                    "New member joined"
                );
                self.stats.joins.fetch_add(1, Ordering::Relaxed);
                metrics::counter!(METRIC_JOINS).increment(1);

                should_queue_update = true;
                update_data = Some((
                    member.node_id.clone(),
                    member.state,
                    member.incarnation,
                    member.address.clone(),
                ));
                members.insert(member.node_id.clone(), member);
            }

            // Update member count metric
            let member_count = members.len();
            metrics::gauge!(METRIC_MEMBERS_TOTAL).set(member_count as f64);
        }

        // Queue update outside the lock
        if should_queue_update {
            if let Some((node_id, state, incarnation, address)) = update_data {
                self.queue_update(
                    MembershipUpdate::new(node_id, state, incarnation).with_address(address),
                )
                .await;
            }
        }
    }

    /// Get member by ID
    pub async fn get_member(&self, node_id: &str) -> Option<SwimMember> {
        let members = self.members.read().await;
        members.get(node_id).cloned()
    }

    /// Get all alive members
    pub async fn alive_members(&self) -> Vec<SwimMember> {
        let members = self.members.read().await;
        members
            .values()
            .filter(|m| m.state == NodeState::Alive)
            .cloned()
            .collect()
    }

    /// Get all active members (alive or suspect)
    pub async fn active_members(&self) -> Vec<SwimMember> {
        let members = self.members.read().await;
        members
            .values()
            .filter(|m| m.state.is_active())
            .cloned()
            .collect()
    }

    /// Get cluster size (active members + self)
    pub async fn cluster_size(&self) -> usize {
        let members = self.members.read().await;
        members.values().filter(|m| m.state.is_active()).count() + 1
    }

    /// Get member counts by state
    pub async fn member_counts(&self) -> HashMap<NodeState, usize> {
        let members = self.members.read().await;
        let mut counts = HashMap::new();
        counts.insert(NodeState::Alive, 0);
        counts.insert(NodeState::Suspect, 0);
        counts.insert(NodeState::Dead, 0);
        counts.insert(NodeState::Left, 0);

        for member in members.values() {
            *counts.entry(member.state).or_insert(0) += 1;
        }
        counts
    }

    /// Mark member as suspect
    #[instrument(skip(self), fields(node_id = %node_id))]
    pub async fn suspect_member(&self, node_id: &str) {
        let mut members = self.members.write().await;
        if let Some(member) = members.get_mut(node_id) {
            if member.state == NodeState::Alive {
                member.state = NodeState::Suspect;
                member.state_changed_at = Instant::now();
                debug!("Node moved to suspect state");

                self.stats.suspicions.fetch_add(1, Ordering::Relaxed);
                metrics::counter!(METRIC_SUSPICIONS).increment(1);

                // Queue update for broadcast
                let incarnation = member.incarnation;
                drop(members);
                self.queue_update(MembershipUpdate::new(
                    node_id.to_string(),
                    NodeState::Suspect,
                    incarnation,
                ))
                .await;
            }
        }
    }

    /// Mark member as dead
    #[instrument(skip(self), fields(node_id = %node_id))]
    pub async fn declare_dead(&self, node_id: &str) {
        let mut members = self.members.write().await;
        if let Some(member) = members.get_mut(node_id) {
            if member.state != NodeState::Dead && member.state != NodeState::Left {
                info!("Node declared dead");
                member.state = NodeState::Dead;
                member.state_changed_at = Instant::now();

                self.stats.deaths.fetch_add(1, Ordering::Relaxed);
                metrics::counter!(METRIC_DEATHS).increment(1);

                // Queue update for broadcast
                let incarnation = member.incarnation;
                drop(members);
                self.queue_update(MembershipUpdate::new(
                    node_id.to_string(),
                    NodeState::Dead,
                    incarnation,
                ))
                .await;
            }
        }
    }

    /// Remove a member without publishing a SWIM transition.
    ///
    /// This is reserved for local cleanup of synthetic placeholders, such as seed aliases with
    /// temporary `_unknown_*` IDs, which are not real cluster members and should not emit dead-node
    /// signals or metrics when reconciled to a concrete node identity.
    #[instrument(skip(self), fields(node_id = %node_id))]
    pub async fn remove_member_silently(&self, node_id: &str) {
        let mut members = self.members.write().await;
        members.remove(node_id);
    }

    /// Process an incoming alive message (node proving it's alive)
    #[instrument(skip(self))]
    pub async fn process_alive(&self, node_id: &str, incarnation: u64, address: &str) {
        let mut members = self.members.write().await;

        if let Some(member) = members.get_mut(node_id) {
            if member.update_state(NodeState::Alive, incarnation) {
                member.address = address.to_string();
                member.record_probe_success();
                debug!("Node confirmed alive");
            }
        } else {
            // New member
            let mut member = SwimMember::new(node_id.to_string(), address.to_string());
            member.incarnation = incarnation;
            members.insert(node_id.to_string(), member);

            info!("New node joined cluster");
            self.stats.joins.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_JOINS).increment(1);
        }

        self.stats.updates_received.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_UPDATES_RECEIVED).increment(1);
    }

    /// Process suspicion message from another node
    #[instrument(skip(self))]
    pub async fn process_suspect(&self, node_id: &str, incarnation: u64) {
        // If it's about us, refute it
        if node_id == self.local_node_id {
            let new_incarnation = self.increment_incarnation();
            info!(new_incarnation, "Refuting suspicion about self");

            self.queue_update(
                MembershipUpdate::new(
                    self.local_node_id.clone(),
                    NodeState::Alive,
                    new_incarnation,
                )
                .with_address(self.local_address.clone()),
            )
            .await;

            return;
        }

        let mut members = self.members.write().await;
        if let Some(member) = members.get_mut(node_id) {
            member.update_state(NodeState::Suspect, incarnation);
        }

        self.stats.updates_received.fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_UPDATES_RECEIVED).increment(1);
    }

    /// Select next node to probe (round-robin with randomization)
    pub async fn select_probe_target(&self) -> Option<SwimMember> {
        let members = self.members.read().await;
        let active: Vec<_> = members
            .values()
            .filter(|m| m.state.is_active() && m.node_id != self.local_node_id)
            .collect();

        if active.is_empty() {
            return None;
        }

        // Round-robin with some randomization for fairness
        let seq = self.probe_sequence.fetch_add(1, Ordering::Relaxed);
        let idx = (seq as usize) % active.len();

        Some(active[idx].clone())
    }

    /// Select k random nodes for indirect ping (excluding target and self)
    pub async fn select_indirect_targets(&self, exclude_node_id: &str) -> Vec<SwimMember> {
        use rand::seq::SliceRandom;

        let members = self.members.read().await;
        let mut candidates: Vec<_> = members
            .values()
            .filter(|m| {
                m.state == NodeState::Alive
                    && m.node_id != self.local_node_id
                    && m.node_id != exclude_node_id
            })
            .cloned()
            .collect();

        let mut rng = rand::thread_rng();
        candidates.shuffle(&mut rng);

        candidates
            .into_iter()
            .take(self.config.indirect_ping_nodes)
            .collect()
    }

    /// Check and timeout suspects that have exceeded suspicion timeout
    pub async fn check_suspect_timeouts(&self) {
        let cluster_size = self.cluster_size().await;
        let suspicion_timeout = self.config.suspicion_timeout(cluster_size);

        let members = self.members.write().await;
        let mut to_declare_dead = Vec::new();

        for member in members.values() {
            if member.state == NodeState::Suspect
                && member.state_changed_at.elapsed() > suspicion_timeout
            {
                to_declare_dead.push(member.node_id.clone());
            }
        }

        drop(members);

        for node_id in to_declare_dead {
            self.declare_dead(&node_id).await;
        }
    }

    /// Reap dead nodes that have been dead for too long
    pub async fn reap_dead_nodes(&self) {
        let mut members = self.members.write().await;
        let before = members.len();

        members.retain(|_, member| !member.should_reap(self.config.dead_node_reap_timeout));

        let reaped = before - members.len();
        if reaped > 0 {
            info!(reaped, "Reaped dead nodes from membership");
            self.stats.reaps.fetch_add(reaped as u64, Ordering::Relaxed);
            metrics::counter!(METRIC_REAPS).increment(reaped as u64);
        }
    }

    /// Queue a membership update for piggybacking
    async fn queue_update(&self, update: MembershipUpdate) {
        let mut queue = self.updates_queue.write().await;

        // Check if we already have an update for this node
        if let Some(existing) = queue.iter_mut().find(|u| u.node_id == update.node_id) {
            // Only replace if newer incarnation or more recent
            if update.incarnation >= existing.incarnation {
                *existing = update;
            }
        } else {
            queue.push(update);
        }

        // Limit queue size to prevent unbounded growth
        let max_size = self.config.max_piggyback_updates * 2;
        while queue.len() > max_size {
            queue.remove(0);
        }
    }

    /// Get updates to piggyback on outgoing messages
    pub async fn get_piggyback_updates(&self) -> Vec<MembershipUpdate> {
        let cluster_size = self.cluster_size().await;
        let broadcast_limit = self.config.effective_broadcast_limit(cluster_size);

        let mut queue = self.updates_queue.write().await;

        let updates: Vec<_> = queue
            .iter()
            .filter(|u| !u.should_drop(broadcast_limit))
            .take(self.config.max_piggyback_updates)
            .cloned()
            .collect();

        // Increment broadcast count
        for update in queue.iter_mut() {
            if updates.iter().any(|u| u.node_id == update.node_id) {
                update.broadcast_count += 1;
                self.stats.updates_broadcast.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Remove dropped updates
        queue.retain(|u| !u.should_drop(broadcast_limit));

        metrics::counter!(METRIC_UPDATES_BROADCAST).increment(updates.len() as u64);
        updates
    }

    /// Apply received membership updates
    pub async fn apply_updates(&self, updates: Vec<MembershipUpdate>) {
        for update in updates {
            match update.state {
                NodeState::Alive => {
                    if let Some(addr) = update.address {
                        self.process_alive(&update.node_id, update.incarnation, &addr)
                            .await;
                    }
                }
                NodeState::Suspect => {
                    self.process_suspect(&update.node_id, update.incarnation)
                        .await;
                }
                NodeState::Dead => {
                    self.declare_dead(&update.node_id).await;
                }
                NodeState::Left => {
                    let mut members = self.members.write().await;
                    if let Some(member) = members.get_mut(&update.node_id) {
                        member.update_state(NodeState::Left, update.incarnation);
                        self.stats.leaves.fetch_add(1, Ordering::Relaxed);
                        metrics::counter!(METRIC_LEAVES).increment(1);
                    }
                }
            }
        }
    }

    /// Get full membership state for anti-entropy sync
    pub async fn get_full_state(&self) -> Vec<SwimMember> {
        let members = self.members.read().await;
        members.values().cloned().collect()
    }

    /// Merge full state from anti-entropy sync
    #[instrument(skip(self, remote_state))]
    pub async fn merge_full_state(&self, remote_state: Vec<SwimMember>) {
        let merged_count = remote_state.len();

        for remote_member in remote_state {
            if remote_member.node_id == self.local_node_id {
                continue; // Skip self
            }

            let mut members = self.members.write().await;

            if let Some(local_member) = members.get_mut(&remote_member.node_id) {
                // Higher incarnation wins
                if remote_member.incarnation > local_member.incarnation {
                    *local_member = remote_member;
                } else if remote_member.incarnation == local_member.incarnation {
                    // Same incarnation - prefer more severe state
                    if remote_member.state.priority() > local_member.state.priority() {
                        local_member.state = remote_member.state;
                        local_member.state_changed_at = Instant::now();
                    }
                }
            } else if remote_member.state.is_active() {
                // New member we don't know about
                members.insert(remote_member.node_id.clone(), remote_member);
            }
        }

        self.stats
            .anti_entropy_syncs
            .fetch_add(1, Ordering::Relaxed);
        metrics::counter!(METRIC_ANTI_ENTROPY_SYNCS).increment(1);
        debug!(merged_count, "Merged anti-entropy state");
    }

    /// Check if protocol is running
    #[inline]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Start the protocol
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Stop the protocol
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Record a completed protocol round
    pub fn record_round_completed(&self, duration: Duration) {
        self.stats.rounds_completed.fetch_add(1, Ordering::Relaxed);
        metrics::histogram!(METRIC_ROUND_DURATION).record(duration.as_secs_f64());
    }

    /// Record direct ping result
    pub fn record_direct_ping(&self, success: bool, latency: Duration) {
        self.stats.direct_pings.fetch_add(1, Ordering::Relaxed);
        if success {
            self.stats
                .direct_ping_success
                .fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_DIRECT_PING_SUCCESS).increment(1);
        } else {
            self.stats
                .direct_ping_failed
                .fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_DIRECT_PING_FAILED).increment(1);
        }
        metrics::histogram!(METRIC_PROBE_LATENCY).record(latency.as_secs_f64());
    }

    /// Record indirect ping result
    pub fn record_indirect_ping(&self, success: bool) {
        self.stats.indirect_pings.fetch_add(1, Ordering::Relaxed);
        if success {
            self.stats
                .indirect_ping_success
                .fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_INDIRECT_PING_SUCCESS).increment(1);
        } else {
            self.stats
                .indirect_ping_failed
                .fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_INDIRECT_PING_FAILED).increment(1);
        }
    }

    /// Get health status of the protocol
    pub async fn health_status(&self) -> SwimHealthStatus {
        let members = self.members.read().await;
        let counts = {
            let mut counts = HashMap::new();
            for member in members.values() {
                *counts.entry(member.state).or_insert(0usize) += 1;
            }
            counts
        };
        drop(members);

        let alive = *counts.get(&NodeState::Alive).unwrap_or(&0);
        let suspect = *counts.get(&NodeState::Suspect).unwrap_or(&0);
        let dead = *counts.get(&NodeState::Dead).unwrap_or(&0);
        let total = alive + suspect + dead;

        SwimHealthStatus {
            is_running: self.is_running(),
            local_incarnation: self.local_incarnation(),
            total_members: total,
            alive_members: alive,
            suspect_members: suspect,
            dead_members: dead,
            stats: self.stats.snapshot(),
        }
    }
}

/// Health status of the SWIM protocol
#[derive(Debug, Clone)]
pub struct SwimHealthStatus {
    pub is_running: bool,
    pub local_incarnation: u64,
    pub total_members: usize,
    pub alive_members: usize,
    pub suspect_members: usize,
    pub dead_members: usize,
    pub stats: SwimStatsSnapshot,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // ExponentialBackoff Tests
    // ============================================================

    #[test]
    fn test_backoff_initial_delay() {
        let mut backoff = ExponentialBackoff::new();
        let delay = backoff.next_backoff().unwrap();

        // Should be between base and base*3 due to jitter
        assert!(delay >= Duration::from_millis(100));
        assert!(delay <= Duration::from_millis(300));
    }

    #[test]
    fn test_backoff_increases() {
        let mut backoff = ExponentialBackoff::with_params(
            Duration::from_millis(100),
            Duration::from_secs(10),
            20,
        );

        let mut delays = Vec::new();
        for _ in 0..5 {
            delays.push(backoff.next_backoff().unwrap());
        }

        // Delays should be capped
        assert!(delays.iter().all(|d| *d <= Duration::from_secs(10)));
    }

    #[test]
    fn test_backoff_respects_cap() {
        let mut backoff = ExponentialBackoff::with_params(
            Duration::from_millis(100),
            Duration::from_millis(500),
            100,
        );

        for _ in 0..50 {
            let delay = backoff.next_backoff().unwrap();
            assert!(delay <= Duration::from_millis(500));
        }
    }

    #[test]
    fn test_backoff_max_attempts() {
        let mut backoff = ExponentialBackoff::with_params(
            Duration::from_millis(10),
            Duration::from_millis(100),
            3,
        );

        assert!(backoff.next_backoff().is_some());
        assert_eq!(backoff.attempts(), 1);
        assert!(backoff.next_backoff().is_some());
        assert_eq!(backoff.attempts(), 2);
        assert!(backoff.next_backoff().is_some());
        assert_eq!(backoff.attempts(), 3);
        assert!(backoff.next_backoff().is_none());
        assert!(backoff.is_exhausted());
    }

    #[test]
    fn test_backoff_reset() {
        let mut backoff =
            ExponentialBackoff::with_params(Duration::from_millis(100), Duration::from_secs(1), 5);

        backoff.next_backoff();
        backoff.next_backoff();
        backoff.next_backoff();

        assert_eq!(backoff.attempts(), 3);

        backoff.reset();
        assert_eq!(backoff.attempts(), 0);
        assert!(!backoff.is_exhausted());
    }

    #[test]
    fn test_backoff_jitter_variance() {
        let mut backoff = ExponentialBackoff::with_params(
            Duration::from_millis(100),
            Duration::from_secs(10),
            1000,
        );

        let mut delays = Vec::new();
        for _ in 0..10 {
            backoff.reset();
            delays.push(backoff.next_backoff().unwrap());
        }

        let first = delays[0];
        let has_variance = delays.iter().any(|d| *d != first);
        assert!(has_variance, "Jitter should provide variance in delays");
    }

    #[test]
    fn test_backoff_unlimited_attempts() {
        let mut backoff = ExponentialBackoff::with_params(
            Duration::from_millis(10),
            Duration::from_millis(100),
            0, // unlimited
        );

        for _ in 0..100 {
            assert!(backoff.next_backoff().is_some());
        }
        assert!(!backoff.is_exhausted());
    }

    // ============================================================
    // NodeState Tests
    // ============================================================

    #[test]
    fn test_node_state_is_active() {
        assert!(NodeState::Alive.is_active());
        assert!(NodeState::Suspect.is_active());
        assert!(!NodeState::Dead.is_active());
        assert!(!NodeState::Left.is_active());
    }

    #[test]
    fn test_node_state_as_str() {
        assert_eq!(NodeState::Alive.as_str(), "alive");
        assert_eq!(NodeState::Suspect.as_str(), "suspect");
        assert_eq!(NodeState::Dead.as_str(), "dead");
        assert_eq!(NodeState::Left.as_str(), "left");
    }

    #[test]
    fn test_node_state_priority() {
        assert!(NodeState::Alive.priority() < NodeState::Suspect.priority());
        assert!(NodeState::Suspect.priority() < NodeState::Dead.priority());
        assert!(NodeState::Dead.priority() < NodeState::Left.priority());
    }

    #[test]
    fn test_node_state_display() {
        assert_eq!(format!("{}", NodeState::Alive), "alive");
        assert_eq!(format!("{}", NodeState::Dead), "dead");
    }

    // ============================================================
    // SwimMember Tests
    // ============================================================

    #[test]
    fn test_member_creation() {
        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());

        assert_eq!(member.node_id, "node-1");
        assert_eq!(member.address, "localhost:8001");
        assert_eq!(member.state, NodeState::Alive);
        assert_eq!(member.incarnation, 0);
        assert_eq!(member.failed_probes, 0);
    }

    #[test]
    fn test_member_with_metadata() {
        let mut metadata = HashMap::new();
        metadata.insert("cluster".to_string(), "test-cluster".to_string());

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string())
            .with_metadata(metadata);

        assert_eq!(
            member.metadata.get("cluster"),
            Some(&"test-cluster".to_string())
        );
    }

    #[test]
    fn test_member_state_update_higher_incarnation() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.incarnation = 5;
        member.state = NodeState::Alive;

        let updated = member.update_state(NodeState::Suspect, 10);
        assert!(updated);
        assert_eq!(member.state, NodeState::Suspect);
        assert_eq!(member.incarnation, 10);
    }

    #[test]
    fn test_member_state_update_lower_incarnation_rejected() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.incarnation = 10;
        member.state = NodeState::Alive;

        let updated = member.update_state(NodeState::Dead, 5);
        assert!(!updated);
        assert_eq!(member.state, NodeState::Alive);
        assert_eq!(member.incarnation, 10);
    }

    #[test]
    fn test_member_state_update_same_incarnation_priority() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.incarnation = 5;
        member.state = NodeState::Suspect;

        // Same incarnation - Dead beats Suspect
        let updated = member.update_state(NodeState::Dead, 5);
        assert!(updated);
        assert_eq!(member.state, NodeState::Dead);

        // Same incarnation - Alive cannot override Dead (lower priority)
        let updated = member.update_state(NodeState::Alive, 5);
        assert!(!updated);
        assert_eq!(member.state, NodeState::Dead);
    }

    #[test]
    fn test_member_refute() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.incarnation = 5;
        member.state = NodeState::Suspect;
        member.failed_probes = 3;

        member.refute();

        assert_eq!(member.state, NodeState::Alive);
        assert_eq!(member.incarnation, 6);
        assert_eq!(member.failed_probes, 0);
    }

    #[test]
    fn test_member_should_reap() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());

        // Alive member should not be reaped
        assert!(!member.should_reap(Duration::from_secs(1)));

        // Dead member that just died should not be reaped yet
        member.state = NodeState::Dead;
        member.state_changed_at = Instant::now();
        assert!(!member.should_reap(Duration::from_secs(300)));
    }

    #[test]
    fn test_member_probe_success_clears_suspect() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.state = NodeState::Suspect;
        member.failed_probes = 5;

        member.record_probe_success();

        assert_eq!(member.state, NodeState::Alive);
        assert_eq!(member.failed_probes, 0);
        assert!(member.last_probe_success.is_some());
    }

    #[test]
    fn test_member_probe_failure_count() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());

        for _ in 0..5 {
            member.record_probe_failure();
        }

        assert_eq!(member.failed_probes, 5);
    }

    #[test]
    fn test_member_probe_failure_saturates() {
        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.failed_probes = u32::MAX;
        member.record_probe_failure();
        assert_eq!(member.failed_probes, u32::MAX);
    }

    #[test]
    fn test_member_time_since_last_success() {
        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        let elapsed = member.time_since_last_success();
        assert!(elapsed.is_some());
        assert!(elapsed.unwrap() < Duration::from_secs(1));
    }

    // ============================================================
    // MembershipUpdate Tests
    // ============================================================

    #[test]
    fn test_membership_update_creation() {
        let update = MembershipUpdate::new("node-1".to_string(), NodeState::Alive, 5);

        assert_eq!(update.node_id, "node-1");
        assert_eq!(update.state, NodeState::Alive);
        assert_eq!(update.incarnation, 5);
        assert!(update.address.is_none());
        assert_eq!(update.broadcast_count, 0);
    }

    #[test]
    fn test_membership_update_with_address() {
        let update = MembershipUpdate::new("node-1".to_string(), NodeState::Alive, 5)
            .with_address("localhost:8001".to_string());

        assert_eq!(update.address, Some("localhost:8001".to_string()));
    }

    #[test]
    fn test_membership_update_should_drop() {
        let mut update = MembershipUpdate::new("node-1".to_string(), NodeState::Alive, 0);

        assert!(!update.should_drop(5));

        update.broadcast_count = 5;
        assert!(update.should_drop(5));

        update.broadcast_count = 4;
        assert!(!update.should_drop(5));
    }

    #[test]
    fn test_membership_update_age() {
        let update = MembershipUpdate::new("node-1".to_string(), NodeState::Alive, 0);
        let age = update.age();
        assert!(age < Duration::from_secs(1));
    }

    // ============================================================
    // SwimConfig Tests
    // ============================================================

    #[test]
    fn test_config_suspicion_timeout_scales_with_cluster() {
        let config = SwimConfig::default();

        let timeout_10 = config.suspicion_timeout(10);
        let timeout_100 = config.suspicion_timeout(100);
        let timeout_1000 = config.suspicion_timeout(1000);

        // Larger clusters should have longer suspicion timeouts
        assert!(timeout_100 > timeout_10);
        assert!(timeout_1000 > timeout_100);
    }

    #[test]
    fn test_config_suspicion_timeout_bounded() {
        let config = SwimConfig {
            suspicion_min: Duration::from_secs(1),
            suspicion_max: Duration::from_secs(10),
            ..Default::default()
        };

        // Very small cluster
        let timeout_small = config.suspicion_timeout(1);
        assert!(timeout_small >= Duration::from_secs(1));

        // Very large cluster
        let timeout_large = config.suspicion_timeout(1_000_000);
        assert!(timeout_large <= Duration::from_secs(10));
    }

    #[test]
    fn test_config_effective_broadcast_limit() {
        let config = SwimConfig::default();

        let limit_10 = config.effective_broadcast_limit(10);
        let limit_100 = config.effective_broadcast_limit(100);

        // Larger clusters should have higher broadcast limits
        assert!(limit_100 > limit_10);
    }

    #[test]
    fn test_config_default_values() {
        let config = SwimConfig::default();

        assert_eq!(config.protocol_period, Duration::from_secs(1));
        assert_eq!(config.probe_timeout, Duration::from_millis(500));
        assert_eq!(config.indirect_ping_nodes, 3);
        assert_eq!(config.suspicion_mult, 4);
    }

    // ============================================================
    // SwimStats Tests
    // ============================================================

    #[test]
    fn test_stats_creation() {
        let stats = SwimStats::new();
        assert_eq!(stats.direct_pings.load(Ordering::Relaxed), 0);
        assert_eq!(stats.joins.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_stats_snapshot() {
        let stats = SwimStats::new();
        stats.direct_pings.store(100, Ordering::Relaxed);
        stats.joins.store(10, Ordering::Relaxed);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.direct_pings, 100);
        assert_eq!(snapshot.joins, 10);
    }

    #[test]
    fn test_stats_atomic_increments() {
        let stats = SwimStats::new();

        for _ in 0..100 {
            stats.direct_pings.fetch_add(1, Ordering::Relaxed);
        }

        assert_eq!(stats.direct_pings.load(Ordering::Relaxed), 100);
    }

    // ============================================================
    // SwimProtocol Tests
    // ============================================================

    #[tokio::test]
    async fn test_protocol_creation() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        assert_eq!(protocol.local_node_id(), "local-node");
        assert_eq!(protocol.local_address(), "localhost:8000");
        assert_eq!(protocol.local_incarnation(), 0);
        assert_eq!(protocol.cluster_size().await, 1);
        assert!(!protocol.is_running());
    }

    #[tokio::test]
    async fn test_protocol_start_stop() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        assert!(!protocol.is_running());
        protocol.start();
        assert!(protocol.is_running());
        protocol.stop();
        assert!(!protocol.is_running());
    }

    #[tokio::test]
    async fn test_protocol_upsert_member() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        let retrieved = protocol.get_member("node-1").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().node_id, "node-1");
    }

    #[tokio::test]
    async fn test_protocol_upsert_member_updates_incarnation() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let mut member1 = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member1.incarnation = 5;
        protocol.upsert_member(member1).await;

        let mut member2 = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member2.incarnation = 10;
        protocol.upsert_member(member2).await;

        let retrieved = protocol.get_member("node-1").await.unwrap();
        assert_eq!(retrieved.incarnation, 10);
    }

    #[tokio::test]
    async fn test_protocol_alive_members() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member1 = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member1).await;

        let mut member2 = SwimMember::new("node-2".to_string(), "localhost:8002".to_string());
        member2.state = NodeState::Suspect;
        protocol.upsert_member(member2).await;

        let mut member3 = SwimMember::new("node-3".to_string(), "localhost:8003".to_string());
        member3.state = NodeState::Dead;
        protocol.upsert_member(member3).await;

        let alive = protocol.alive_members().await;
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].node_id, "node-1");

        let active = protocol.active_members().await;
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_protocol_member_counts() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member1 = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member1).await;

        let mut member2 = SwimMember::new("node-2".to_string(), "localhost:8002".to_string());
        member2.state = NodeState::Suspect;
        protocol.upsert_member(member2).await;

        let counts = protocol.member_counts().await;
        assert_eq!(*counts.get(&NodeState::Alive).unwrap(), 1);
        assert_eq!(*counts.get(&NodeState::Suspect).unwrap(), 1);
        assert_eq!(*counts.get(&NodeState::Dead).unwrap(), 0);
    }

    #[tokio::test]
    async fn test_protocol_suspect_member() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        protocol.suspect_member("node-1").await;

        let retrieved = protocol.get_member("node-1").await.unwrap();
        assert_eq!(retrieved.state, NodeState::Suspect);
    }

    #[tokio::test]
    async fn test_protocol_declare_dead() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        protocol.declare_dead("node-1").await;

        let retrieved = protocol.get_member("node-1").await.unwrap();
        assert_eq!(retrieved.state, NodeState::Dead);
    }

    #[tokio::test]
    async fn test_protocol_remove_member_silently() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("_unknown_seed".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        protocol.remove_member_silently("_unknown_seed").await;

        assert!(protocol.get_member("_unknown_seed").await.is_none());
    }

    #[tokio::test]
    async fn test_protocol_refute_self_suspicion() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let initial_incarnation = protocol.local_incarnation();

        protocol.process_suspect("local-node", 0).await;

        let new_incarnation = protocol.local_incarnation();
        assert!(new_incarnation > initial_incarnation);
    }

    #[tokio::test]
    async fn test_protocol_process_alive() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        protocol.process_alive("node-1", 5, "localhost:8001").await;

        let member = protocol.get_member("node-1").await.unwrap();
        assert_eq!(member.state, NodeState::Alive);
        assert_eq!(member.incarnation, 5);
    }

    #[tokio::test]
    async fn test_protocol_select_probe_target() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        // No members - should return None
        assert!(protocol.select_probe_target().await.is_none());

        // Add a member
        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        // Should return the member
        let target = protocol.select_probe_target().await;
        assert!(target.is_some());
        assert_eq!(target.unwrap().node_id, "node-1");
    }

    #[tokio::test]
    async fn test_protocol_select_indirect_targets() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        // Add multiple members
        for i in 1..=5 {
            let member = SwimMember::new(format!("node-{}", i), format!("localhost:800{}", i));
            protocol.upsert_member(member).await;
        }

        // Select indirect targets excluding node-1
        let targets = protocol.select_indirect_targets("node-1").await;

        // Should get up to indirect_ping_nodes (default 3)
        assert!(targets.len() <= 3);
        // Should not include node-1 or local-node
        assert!(targets.iter().all(|t| t.node_id != "node-1"));
        assert!(targets.iter().all(|t| t.node_id != "local-node"));
    }

    #[tokio::test]
    async fn test_protocol_piggyback_updates() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        let updates = protocol.get_piggyback_updates().await;
        assert!(!updates.is_empty());
        assert_eq!(updates[0].node_id, "node-1");
    }

    #[tokio::test]
    async fn test_protocol_apply_updates() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let updates = vec![
            MembershipUpdate::new("node-1".to_string(), NodeState::Alive, 5)
                .with_address("localhost:8001".to_string()),
        ];

        protocol.apply_updates(updates).await;

        let member = protocol.get_member("node-1").await;
        assert!(member.is_some());
        assert_eq!(member.unwrap().incarnation, 5);
    }

    #[tokio::test]
    async fn test_protocol_full_state_sync() {
        let protocol1 = SwimProtocol::new(
            "node-1".to_string(),
            "localhost:8001".to_string(),
            SwimConfig::default(),
        );

        let protocol2 = SwimProtocol::new(
            "node-2".to_string(),
            "localhost:8002".to_string(),
            SwimConfig::default(),
        );

        // Protocol1 knows about node-3
        let member3 = SwimMember::new("node-3".to_string(), "localhost:8003".to_string());
        protocol1.upsert_member(member3).await;

        // Get full state from protocol1
        let state = protocol1.get_full_state().await;

        // Merge into protocol2
        protocol2.merge_full_state(state).await;

        // Protocol2 should now know about node-3
        let member = protocol2.get_member("node-3").await;
        assert!(member.is_some());
    }

    #[tokio::test]
    async fn test_protocol_cluster_size() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        // Initially just self
        assert_eq!(protocol.cluster_size().await, 1);

        // Add alive member
        let member1 = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member1).await;
        assert_eq!(protocol.cluster_size().await, 2);

        // Add dead member (shouldn't count)
        let mut member2 = SwimMember::new("node-2".to_string(), "localhost:8002".to_string());
        member2.state = NodeState::Dead;
        protocol.upsert_member(member2).await;
        assert_eq!(protocol.cluster_size().await, 2);
    }

    #[tokio::test]
    async fn test_protocol_health_status() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        let health = protocol.health_status().await;
        assert!(!health.is_running);
        assert_eq!(health.total_members, 1);
        assert_eq!(health.alive_members, 1);
    }

    #[tokio::test]
    async fn test_protocol_record_ping() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        protocol.record_direct_ping(true, Duration::from_millis(10));
        protocol.record_direct_ping(false, Duration::from_millis(500));

        let stats = protocol.stats().snapshot();
        assert_eq!(stats.direct_pings, 2);
        assert_eq!(stats.direct_ping_success, 1);
        assert_eq!(stats.direct_ping_failed, 1);
    }

    // ============================================================
    // Edge Case Tests
    // ============================================================

    #[tokio::test]
    async fn test_edge_case_empty_cluster() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        // Operations on empty cluster should not panic
        assert!(protocol.select_probe_target().await.is_none());
        assert!(protocol.select_indirect_targets("any").await.is_empty());
        protocol.check_suspect_timeouts().await;
        protocol.reap_dead_nodes().await;
    }

    #[tokio::test]
    async fn test_edge_case_single_node_cluster() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        // With only one other node, indirect targets should be empty
        let targets = protocol.select_indirect_targets("node-1").await;
        assert!(targets.is_empty());
    }

    #[tokio::test]
    async fn test_edge_case_incarnation_overflow() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let mut member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        member.incarnation = u64::MAX - 1;
        protocol.upsert_member(member).await;

        protocol
            .process_alive("node-1", u64::MAX, "localhost:8001")
            .await;

        let updated = protocol.get_member("node-1").await.unwrap();
        assert_eq!(updated.incarnation, u64::MAX);
    }

    #[tokio::test]
    async fn test_edge_case_rapid_state_changes() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;

        // Rapid state changes
        for i in 0..100 {
            if i % 2 == 0 {
                protocol.suspect_member("node-1").await;
            } else {
                protocol
                    .process_alive("node-1", i as u64, "localhost:8001")
                    .await;
            }
        }

        // Should end up alive with high incarnation
        let final_member = protocol.get_member("node-1").await.unwrap();
        assert!(final_member.incarnation > 0);
    }

    #[tokio::test]
    async fn test_edge_case_duplicate_updates() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member.clone()).await;
        protocol.upsert_member(member.clone()).await;
        protocol.upsert_member(member).await;

        let members = protocol.alive_members().await;
        assert_eq!(members.len(), 1);
    }

    #[tokio::test]
    async fn test_edge_case_self_operations() {
        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        );

        let self_member = SwimMember::new("local-node".to_string(), "localhost:8000".to_string());
        protocol.upsert_member(self_member).await;

        // Self should not appear in probe targets
        let target = protocol.select_probe_target().await;
        assert!(target.is_none() || target.as_ref().unwrap().node_id != "local-node");
    }

    #[tokio::test]
    async fn test_edge_case_concurrent_upserts() {
        let protocol = Arc::new(SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            SwimConfig::default(),
        ));

        let mut handles = Vec::new();

        for i in 0..10 {
            let p = protocol.clone();
            handles.push(tokio::spawn(async move {
                for j in 0..100 {
                    let member = SwimMember::new(
                        format!("node-{}-{}", i, j),
                        format!("localhost:{}", 8000 + i * 100 + j),
                    );
                    p.upsert_member(member).await;
                }
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        let count = protocol.cluster_size().await;
        assert!(count > 1); // At least some members added
    }

    #[tokio::test]
    async fn test_reap_timing() {
        let config = SwimConfig {
            dead_node_reap_timeout: Duration::from_millis(50),
            ..Default::default()
        };

        let protocol = SwimProtocol::new(
            "local-node".to_string(),
            "localhost:8000".to_string(),
            config,
        );

        let member = SwimMember::new("node-1".to_string(), "localhost:8001".to_string());
        protocol.upsert_member(member).await;
        protocol.declare_dead("node-1").await;

        // Should not be reaped immediately
        protocol.reap_dead_nodes().await;
        assert!(protocol.get_member("node-1").await.is_some());

        // Wait for reap timeout
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be reaped now
        protocol.reap_dead_nodes().await;
        assert!(protocol.get_member("node-1").await.is_none());
    }
}
