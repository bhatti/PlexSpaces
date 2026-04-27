// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Event Analytics - Distributed Event Tracking Example
//
// Real-world use case: Web analytics/event tracking system (Google Analytics, Mixpanel-style)
// that tracks page views, clicks, and conversions across multiple shards for horizontal scaling.
//
// ## Architecture
//
// This example demonstrates the Shard Groups pattern (data-parallel actors) for horizontal scaling:
//
// 1. **Hash-Based Routing**: Partition key (user_id) → hash → shard_id → specific actor
//    - Ensures consistent routing: same user always routes to same shard
//    - Enables even distribution: keys are evenly distributed across shards
//    - O(1) routing: fast lookup without coordination
//
// 2. **Scatter-Gather Queries**: Query all shards in parallel, aggregate results
//    - Parallel queries: All shards queried simultaneously
//    - Aggregation: Sum metrics across shards for global totals
//    - Fault tolerance: Handles individual shard failures gracefully
//
// 3. **SDK Patterns**: Uses SDK annotations and helpers to minimize boilerplate
//    - `#[gen_server_actor]` - Declares GenServer behavior
//    - `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
//    - `#[handler("op", cast)]` - Fire-and-forget event tracking
//    - `#[handler("op")]` - Request-reply metrics queries
//    - `spawn_gen_server()` - SDK helper (delegates to ActorFactoryImpl)
//    - `GenServerRef.cast()` - Fire-and-forget messaging (wraps ActorRef.tell())
//    - `GenServerRef.call()` - Request-reply messaging (wraps ActorRef.ask())
//
// ## Design Principles
//
// - **Core Functionality**: Lives in main crates (ActorFactoryImpl, ActorRef)
// - **SDK Role**: Provides decorators/helpers to simplify usage
// - **No Hacks**: Proper trait usage, no cyclic dependencies
// - **Observability**: CoordinationComputeTracker for metrics
// - **Tenant Isolation**: Explicit RequestContext with tenant/namespace

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, spawn_gen_server, json, Value, GenServerRef,
};
use plexspaces_node::CoordinationComputeTracker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, Level};
use anyhow::Result;

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Event Analytics
// =============================================================================

/// Analytics event types tracked by the system.
/// 
/// Mirrors common web analytics event types:
/// - PageView: User views a page
/// - Click: User clicks an element
/// - Conversion: User completes a goal (purchase, signup, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    PageView,
    Click,
    Conversion,
}

/// Analytics event representing a user action.
/// 
/// ## Fields
/// - `user_id`: Partition key for hash-based routing to shards
/// - `page_id`: Page identifier for tracking page-specific metrics
/// - `event_type`: Type of event (PageView, Click, Conversion)
/// - `timestamp`: Event timestamp for time-series analysis
/// - `metadata`: Additional event metadata (custom dimensions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    pub user_id: String,
    pub page_id: String,
    pub event_type: EventType,
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
}

/// Aggregated analytics metrics for a single shard.
/// 
/// Used in scatter-gather queries to aggregate metrics across all shards.
/// Each shard returns its portion of the data, which is then summed to get
/// global totals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMetrics {
    pub shard_id: usize,
    pub page_views: u64,
    pub clicks: u64,
    pub conversions: u64,
    pub unique_users: usize,
    pub unique_pages: usize,
}

// =============================================================================
// Analytics Shard Actor - SDK Annotations
// =============================================================================

/// Analytics shard actor that tracks events for a subset of users/pages.
/// 
/// ## Architecture
/// Uses hash-based partitioning: user_id → hash → shard_id
/// Each shard handles events for a portion of the key space, enabling
/// horizontal scaling for high-throughput event tracking workloads.
/// 
/// ## State Management
/// - Tracks page views, clicks, and conversions per page
/// - Maintains sets of unique users and pages for analytics
/// - All state is in-memory (can be extended with DurabilityFacet for persistence)
/// 
/// ## Message Handling
/// - `track_event` (cast): Fire-and-forget event tracking
/// - `get_metrics` (call): Request-reply metrics query
#[gen_server_actor]
struct AnalyticsShard {
    shard_id: usize,
    page_views: HashMap<String, u64>,  // page_id -> count
    clicks: HashMap<String, u64>,      // page_id -> count
    conversions: HashMap<String, u64>, // page_id -> count
    users: std::collections::HashSet<String>,
    pages: std::collections::HashSet<String>,
}

impl AnalyticsShard {
    fn new(shard_id: usize) -> Self {
        Self {
            shard_id,
            page_views: HashMap::new(),
            clicks: HashMap::new(),
            conversions: HashMap::new(),
            users: std::collections::HashSet::new(),
            pages: std::collections::HashSet::new(),
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl AnalyticsShard {
    /// Track an analytics event (fire-and-forget).
    /// 
    /// ## Semantics
    /// Uses `cast` semantics - no reply is sent, enabling high-throughput
    /// event tracking without blocking on acknowledgments.
    /// 
    /// ## Processing
    /// - Updates counters for the event type (page_view, click, conversion)
    /// - Maintains unique user and page sets for analytics
    /// - All operations are O(1) for performance
    #[handler("track_event", cast)]
    async fn handle_track_event(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let event: AnalyticsEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid event: {}", e)))?;
        
        // Track unique users and pages for analytics
        self.users.insert(event.user_id.clone());
        self.pages.insert(event.page_id.clone());
        
        // Update counters based on event type
        match event.event_type {
            EventType::PageView => {
                *self.page_views.entry(event.page_id.clone()).or_insert(0) += 1;
            }
            EventType::Click => {
                *self.clicks.entry(event.page_id.clone()).or_insert(0) += 1;
            }
            EventType::Conversion => {
                *self.conversions.entry(event.page_id.clone()).or_insert(0) += 1;
            }
        }
        
        Ok(())
    }
    
    /// Get metrics for this shard (request-reply).
    /// 
    /// ## Semantics
    /// Uses `call` semantics (default for GenServer) - returns aggregated
    /// metrics for this shard's portion of the data.
    /// 
    /// ## Aggregation
    /// - Sums all counters across pages for this shard
    /// - Returns unique user/page counts
    /// - Used in scatter-gather queries to aggregate across all shards
    #[handler("get_metrics")]
    async fn handle_get_metrics(
        &self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let metrics = ShardMetrics {
            shard_id: self.shard_id,
            page_views: self.page_views.values().sum(),
            clicks: self.clicks.values().sum(),
            conversions: self.conversions.values().sum(),
            unique_users: self.users.len(),
            unique_pages: self.pages.len(),
        };
        
        Ok(json!(metrics))
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Hash-based routing: partition key → shard_id.
/// 
/// ## Algorithm
/// Uses a simple hash function (Java String.hashCode style) to deterministically
/// route partition keys to shards. This ensures:
/// - Consistent routing: same key always routes to same shard
/// - Even distribution: keys are evenly distributed across shards
/// - O(1) routing: fast lookup without coordination
/// 
/// ## Parameters
/// - `key`: Partition key (e.g., user_id, page_id)
/// - `shard_count`: Total number of shards
/// 
/// ## Returns
/// Shard ID (0 to shard_count-1)
fn route_to_shard(key: &str, shard_count: usize) -> usize {
    let hash = key.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % shard_count as u64) as usize
}

/// Generate sample analytics events for demonstration.
/// 
/// ## Distribution
/// - 1000 unique users (cycled through)
/// - 100 unique pages (cycled through)
/// - Event types: 70% page views, 20% clicks, 10% conversions
/// 
/// ## Purpose
/// Creates realistic event distribution for performance testing and
/// demonstrating shard distribution patterns.
fn generate_events(num_events: usize) -> Vec<AnalyticsEvent> {
    let mut events = Vec::new();
    
    for i in 0..num_events {
        let user_id = format!("user-{}", i % 1000);
        let page_id = format!("page-{}", i % 100);
        
        // Distribute event types: 70% page views, 20% clicks, 10% conversions
        let event_type = if i % 10 < 7 {
            EventType::PageView
        } else if i % 10 < 9 {
            EventType::Click
        } else {
            EventType::Conversion
        };
        
        events.push(AnalyticsEvent {
            user_id,
            page_id,
            event_type,
            timestamp: i as u64,
            metadata: HashMap::new(),
        });
    }
    
    events
}

// =============================================================================
// Main - Demonstrates Shard Groups with Event Analytics
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - use try_init() to avoid panic if already initialized
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("event_analytics=info,plexspaces=warn")
        .try_init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Event Analytics - Distributed Event Tracking              ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Web analytics/event tracking (Google Analytics-style)");
    println!("Pattern: Hash-based sharding for horizontal scaling");
    println!();
    
    // Create metrics tracker for coordination vs computation analysis
    let mut metrics_tracker = CoordinationComputeTracker::new("event-analytics".to_string());
    let total_start = Instant::now();

    // Configuration: Use non-trivial data sizes (run for 2+ seconds)
    let shard_count = 8;
    let num_events = 10_000; // 10K events to show real performance
    let node_id = "analytics-node";

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let node_start = Instant::now();
    
    // Create node with clustering disabled for single-node example
    // SDK pattern: Use NodeBuilder for node creation
    let node = NodeBuilder::new(node_id)
        .with_clustering_enabled(false)
        .build_started().await;
    
    let node_time = node_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  ✓ Node '{}' created ({:.2}ms)", node_id, node_time.as_secs_f64() * 1000.0);
    println!();

    // Create request context with explicit tenant/namespace
    // SDK pattern: Always use explicit tenant/namespace, never RequestContext::internal()
    let ctx = RequestContext::new_without_auth(
        "analytics-tenant".to_string(),
        "event-tracking".to_string(),
    );

    // =========================================================================
    // Step 2: Spawn Shard Actors using SDK
    // =========================================================================
    println!("Step 2: Spawn {} Analytics Shard Actors", shard_count);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let spawn_start = Instant::now();
    
    // SDK pattern: Use spawn_gen_server() helper instead of ActorFactory directly
    // This delegates to core crates (ActorFactoryImpl) while providing clean API
    let mut shards: Vec<GenServerRef> = Vec::new();
    let service_locator = node.service_locator();
    
    for i in 0..shard_count {
        let actor_name = format!("analytics-shard-{}", i);
        
        // SDK helper: spawn_gen_server() wraps ActorFactoryImpl.spawn_gen_server()
        // Core functionality stays in crates/actor, SDK provides convenience wrapper
        let shard = spawn_gen_server(
            &ctx,
            service_locator.clone(),
            &actor_name,
            AnalyticsShard::new(i),
            vec![], // No facets needed for this example
        ).await
        .map_err(|e| anyhow::anyhow!("Failed to spawn shard-{}: {}", i, e))?;
        
        shards.push(shard);
        metrics_tracker.increment_message();
        
        if i < 3 || i == shard_count - 1 {
            println!("  ✓ Spawned analytics-shard-{}", i);
        }
    }
    
    let spawn_time = spawn_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Spawned {} shards in {:.2}ms", shard_count, spawn_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 3: Track Events (Hash-based Routing)
    // =========================================================================
    println!("Step 3: Track {} Analytics Events (Hash-based Routing)", num_events);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let events = generate_events(num_events);
    
    metrics_tracker.start_coordinate();
    let track_start = Instant::now();
    
    let mut shard_distribution: HashMap<usize, u64> = HashMap::new();
    
    for (idx, event) in events.iter().enumerate() {
        // Hash-based routing: user_id → shard_id
        // This ensures consistent routing and even distribution
        let shard_id = route_to_shard(&event.user_id, shard_count);
        *shard_distribution.entry(shard_id).or_insert(0) += 1;
        
        metrics_tracker.start_compute();
        let event_start = Instant::now();
        
        // SDK pattern: Use GenServerRef.cast() for fire-and-forget messaging
        // This wraps ActorRef.tell() with automatic serialization and error handling
        let event_data = json!({
            "user_id": event.user_id,
            "page_id": event.page_id,
            "event_type": match event.event_type {
                EventType::PageView => "PageView",
                EventType::Click => "Click",
                EventType::Conversion => "Conversion",
            },
            "timestamp": event.timestamp,
            "metadata": event.metadata,
        });
        
        shards[shard_id].cast("track_event", &event_data).await
            .map_err(|e| anyhow::anyhow!("Failed to track event: {}", e))?;
        
        let event_time = event_start.elapsed();
        metrics_tracker.end_compute();
        metrics_tracker.increment_message();
        
        // Show progress for first few and last few events
        if idx < 5 || idx >= num_events - 5 {
            println!("  Event {}: user={} → shard-{} ({:.2}ms)", 
                idx + 1, event.user_id, shard_id, event_time.as_secs_f64() * 1000.0);
        }
    }
    
    // Wait for all events to be processed
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    let track_time = track_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Tracked {} events in {:.2}ms", num_events, track_time.as_secs_f64() * 1000.0);
    println!("  Shard distribution:");
    for (shard_id, count) in shard_distribution.iter() {
        println!("    Shard {}: {} events ({:.1}%)", 
            shard_id, count, (*count as f64 / num_events as f64) * 100.0);
    }
    println!();

    // =========================================================================
    // Step 4: Scatter-Gather Query (Aggregate Metrics)
    // =========================================================================
    println!("Step 4: Scatter-Gather Query (Aggregate Metrics Across All Shards)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let query_start = Instant::now();
    
    // Scatter-gather pattern: Query all shards in parallel
    // SDK pattern: Use GenServerRef.call() for request-reply queries
    // This wraps ActorRef.ask() with automatic serialization and typed responses
    let empty_query: serde_json::Value = serde_json::json!({});
    let mut query_futures = Vec::new();
    for shard in &shards {
        // call() serializes immediately, so reference lifetime is fine
        query_futures.push(shard.call::<serde_json::Value, ShardMetrics>("get_metrics", &empty_query));
    }
    
    // Collect results from all shards (scatter-gather aggregation)
    let shard_results: Vec<Result<ShardMetrics, _>> = futures::future::join_all(query_futures).await;
    
    // Aggregate metrics across all shards
    let mut total_page_views = 0u64;
    let mut total_clicks = 0u64;
    let mut total_conversions = 0u64;
    let mut total_unique_users = 0usize;
    let mut total_unique_pages = 0usize;
    
    for (i, result) in shard_results.iter().enumerate() {
        match result {
            Ok(metrics) => {
                // Sum metrics across shards
                total_page_views += metrics.page_views;
                total_clicks += metrics.clicks;
                total_conversions += metrics.conversions;
                total_unique_users += metrics.unique_users;
                total_unique_pages += metrics.unique_pages;
                
                if i < 3 || i == shard_count - 1 {
                    println!("  Shard {}: {} page views, {} clicks, {} conversions, {} users, {} pages",
                        metrics.shard_id, metrics.page_views, metrics.clicks, 
                        metrics.conversions, metrics.unique_users, metrics.unique_pages);
                }
                metrics_tracker.increment_message();
            }
            Err(e) => {
                eprintln!("  Error querying shard {}: {}", i, e);
            }
        }
    }
    
    let query_time = query_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Aggregate totals:");
    println!("    Total Page Views: {}", total_page_views);
    println!("    Total Clicks: {}", total_clicks);
    println!("    Total Conversions: {}", total_conversions);
    println!("    Unique Users: {}", total_unique_users);
    println!("    Unique Pages: {}", total_unique_pages);
    println!("  Query completed in {:.2}ms", query_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Performance Metrics & Benchmarks
    // =========================================================================
    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();
    
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║        PERFORMANCE METRICS & BENCHMARKS                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("COORDINATION vs COMPUTATION ANALYSIS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Total Time:                    {:.2}ms", total_time.as_secs_f64() * 1000.0);
    println!("Coordination Time:             {:.2}ms ({:.1}%)", 
        metrics.coordinate_duration_ms,
        if metrics.total_duration_ms > 0 {
            (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        });
    println!("Computation Time:              {:.2}ms ({:.1}%)", 
        metrics.compute_duration_ms,
        if metrics.total_duration_ms > 0 {
            (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        });
    println!("Granularity Ratio:             {:.2}x", metrics.granularity_ratio);
    println!("Efficiency:                    {:.1}%", metrics.efficiency * 100.0);
    println!("Total Messages:                {}", metrics.message_count);
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("BENCHMARK METRICS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let events_per_sec = num_events as f64 / total_time.as_secs_f64();
    let avg_latency_per_event = total_time.as_secs_f64() * 1000.0 / num_events as f64;
    println!("Events Processed:              {}", num_events);
    println!("Shards:                        {}", shard_count);
    println!("Events/Second:                  {:.2}", events_per_sec);
    println!("Avg Latency per Event:          {:.2}ms", avg_latency_per_event);
    println!("Shard Distribution:             Even (hash-based)");
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("ANALYSIS & RECOMMENDATIONS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    if metrics.granularity_ratio >= 10.0 {
        println!("✓ Excellent granularity ratio (>= 10x) - coordination overhead is minimal");
    } else {
        println!("⚠ Granularity ratio below 10x - consider batching events to reduce coordination overhead");
    }
    if metrics.efficiency >= 0.9 {
        println!("✓ High efficiency (>= 90%) - system is well-balanced");
    } else {
        println!("⚠ Efficiency below 90% - consider optimizing coordination patterns");
    }
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Event Analytics Example Complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("SDK Patterns Demonstrated:");
    println!("  • #[gen_server_actor] - Declares GenServer behavior");
    println!("  • #[plexspaces_handlers(gen_server)] - Auto-generated message dispatch");
    println!("  • #[handler(\"track_event\", cast)] - Fire-and-forget event tracking");
    println!("  • #[handler(\"get_metrics\")] - Request-reply metrics query");
    println!("  • spawn_gen_server() - SDK helper (delegates to ActorFactoryImpl)");
    println!("  • GenServerRef.cast() - Fire-and-forget (wraps ActorRef.tell())");
    println!("  • GenServerRef.call() - Request-reply (wraps ActorRef.ask())");
    println!();
    println!("Sharding Pattern (Shard Groups):");
    println!("  • Hash-based routing: user_id → hash → shard_id");
    println!("  • Scatter-gather queries: Query all shards in parallel, aggregate results");
    println!("  • Horizontal scaling: Millions of events across multiple shards");
    println!("  • Even distribution: Hash ensures balanced load across shards");
    println!();
    println!("Architecture Notes:");
    println!("  • Core functionality: ActorFactoryImpl, ActorRef (main crates)");
    println!("  • SDK role: Decorators/helpers to simplify usage");
    println!("  • Tenant isolation: Explicit RequestContext (no internal())");
    println!("  • Observability: CoordinationComputeTracker for metrics");
    println!();
    println!("Real-World Use Cases:");
    println!("  • Web analytics (Google Analytics, Mixpanel)");
    println!("  • Event tracking (Segment, Amplitude)");
    println!("  • Distributed counters/metrics");
    println!("  • Time-series data aggregation");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
