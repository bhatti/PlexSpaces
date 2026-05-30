// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Timers Example (TimerFacet) - SDK Annotations Demo
//
// Demonstrates in-memory timers using PlexSpaces TimerFacet:
// - register_once(): One-shot timer (idle timeout)
// - register_periodic(): Repeating timer (heartbeat)
// - cancel(): Cancel active timers
// - Timer fires message to actor.handle_message()
// - CoordinationComputeTracker metrics for timer overhead analysis
//
// ## SDK Annotations Used
// - `#[gen_server_actor(name = "session_actor", facets = ["timer"])]` - marks struct as actor with timer facet
// - `#[plexspaces_handlers]` - generates Actor impl for SDK handlers
// - `#[handler("timer_fired", cast)]` - fire-and-forget handler for timers
//
// Use Case: Session timeout, heartbeat, retry with backoff
//
// ## Architecture
//
// This example demonstrates TimerFacet for in-memory timer management:
// 1. **TimerFacet**: Facet that adds timer capability to actors
// 2. **One-shot timers**: register_once() for idle timeout, retry
// 3. **Periodic timers**: register_periodic() for heartbeat, polling
// 4. **Timer cancellation**: cancel() to stop active timers
// 5. **Metrics**: CoordinationComputeTracker tracks coordination overhead vs timer processing
//
// ## Design Principles
//
// - **Core Functionality**: TimerFacet lives in main crates
// - **No Hacks**: Proper trait usage, no cyclic dependencies
// - **Observability**: CoordinationComputeTracker for metrics
// - **Tenant Isolation**: Explicit RequestContext with tenant/namespace (no internal())

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message, RequestContext, spawn_with_facets, TimerFacet,
};
use plexspaces_actor::{ActorId, ServiceLocator, RequestContextExt};
use plexspaces_node::{CoordinationComputeTracker, NodeBuilder};
use serde_json::json;
use std::time::{Duration, Instant};
use tracing::info;
use anyhow::Result;

// Required for macro-generated code


// =============================================================================
// Session Actor - SDK annotations style (like Python @actor, @handler)
// =============================================================================

/// Session actor with timer facet for idle timeout and heartbeat.
/// 
/// ## Annotations
/// - `#[gen_server_actor(name = "session_actor", facets = ["timer"])]` - declares timer facet
/// - `#[plexspaces_handlers]` - generates Actor impl dispatching to handlers
/// - `#[handler("op", cast)]` - fire-and-forget handlers
#[gen_server_actor(name = "session_actor", facets = ["timer"])]
struct SessionActor {
    user_id: String,
    is_active: bool,
    activity_count: u32,
    timer_fire_count: u32,
}

impl SessionActor {
    fn new(user_id: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            is_active: true,
            activity_count: 0,
            timer_fire_count: 0,
        }
    }
}

/// Handler implementations - SDK generates Actor impl with dispatch.
/// 
/// Using `#[handler("op", cast)]` for fire-and-forget semantics (no reply).
#[plexspaces_handlers]
impl SessionActor {
    /// Handle timer_fired events (from TimerFacet)
    #[handler("timer_fired", cast)]
    async fn handle_timer_fired(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload).to_string();
        self.timer_fire_count += 1;
        
        match timer_name.as_str() {
            "idle_timeout" => {
                self.is_active = false;
            }
            "heartbeat" => {
                // Heartbeat received - session still active
            }
            "retry" => {
                // Retry timer fired - would retry operation
            }
            _ => {
                // Unknown timer
            }
        }
        Ok(())
    }

    /// Handle activity events (simulates user activity)
    #[handler("activity", cast)]
    async fn handle_activity(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.activity_count += 1;
        Ok(())
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("timers=info,plexspaces=warn"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║           Timers Example (TimerFacet)                         ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    info!("API: TimerFacet for in-memory timer management");
    info!("Pattern: Session timeout, heartbeat, retry with backoff");
    println!();

    // Configuration: Use non-trivial scenario (many sessions, various timer patterns)
    let num_sessions = 80;
    let idle_timeout_secs = 30;
    let heartbeat_interval_secs = 5;
    let retry_delay_secs = 2;
    
    info!("Configuration:");
    info!("  Sessions: {}", num_sessions);
    info!("  Idle timeout: {}s", idle_timeout_secs);
    info!("  Heartbeat interval: {}s", heartbeat_interval_secs);
    info!("  Retry delay: {}s", retry_delay_secs);
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("timers".to_string());
    let total_start = Instant::now();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    info!("Step 1: Create PlexSpaces Node");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let node_start = Instant::now();
    
    let node = NodeBuilder::new("timers-node").build_started().await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "sessions".to_string());
    
    let node_time = node_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Node: timers-node");
    info!("  Created in {:.2}ms", node_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 2: Create Session Actors with TimerFacet (coordination phase)
    // =========================================================================
    info!("Step 2: Create {} session actors with TimerFacet", num_sessions);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let spawn_start = Instant::now();
    
    let mut session_ids: Vec<ActorId> = Vec::new();
    
    for i in 0..num_sessions {
        let user_id = format!("user-{}", i);
        let actor_name = format!("session-{}", user_id);
        
        // Create TimerFacet for this session
        let timer_facet = Box::new(TimerFacet::new(json!({}), 50, service_locator.clone()));
        
        // Spawn actor with TimerFacet attached
        let actor_ref = spawn_with_facets(
            &ctx,
            service_locator.clone(),
            actor_name,
            "sessions",
            SessionActor::new(&user_id),
            vec![timer_facet],
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn session actor for {}: {}", user_id, e))?;
        
        session_ids.push(actor_ref.id().clone());
        
        if i < 3 || i == num_sessions - 1 {
            info!("  Spawned session actor: {}", session_ids[i]);
        }
        
        metrics_tracker.increment_message();
    }
    
    let spawn_time = spawn_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Created {} session actors in {:.2}ms", num_sessions, spawn_time.as_secs_f64() * 1000.0);
    info!("  Average spawn time: {:.2}ms per actor", spawn_time.as_secs_f64() * 1000.0 / num_sessions as f64);
    println!();

    // =========================================================================
    // Step 3: Register Timers for Each Session (coordination phase)
    // =========================================================================
    info!("Step 3: Register timers for all sessions");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let timer_registration_start = Instant::now();
    
    let mut total_timers_registered = 0u64;
    
    for (i, actor_id) in session_ids.iter().enumerate() {
        // Get TimerFacet from node (facets are configured with actor_ref and actor_service after spawn)
        let facets_arc = node
            .service_locator()
            .facet_container_for_actor(actor_id.as_str())
            .await
            .ok_or_else(|| anyhow::anyhow!("Facets not found for actor {}", actor_id))?;
        
        let facets_guard = facets_arc.read().await;
        let timer_facet_arc = facets_guard.get_facet("timer")
            .ok_or_else(|| anyhow::anyhow!("TimerFacet not found for actor {}", actor_id))?;
        
        let timer_facet_guard = timer_facet_arc.read().await;
        let timer_facet = timer_facet_guard.as_any().downcast_ref::<TimerFacet>()
            .ok_or_else(|| anyhow::anyhow!("Failed to downcast to TimerFacet for actor {}", actor_id))?;
        
        // Register idle timeout (one-shot)
        timer_facet.register_once("idle_timeout", Duration::from_secs(idle_timeout_secs)).await
            .map_err(|e| anyhow::anyhow!("Failed to register idle_timeout for {}: {}", actor_id, e))?;
        total_timers_registered += 1;
        
        // Register heartbeat (periodic)
        timer_facet.register_periodic("heartbeat", Duration::from_secs(heartbeat_interval_secs)).await
            .map_err(|e| anyhow::anyhow!("Failed to register heartbeat for {}: {}", actor_id, e))?;
        total_timers_registered += 1;
        
        // Register retry timer (one-shot) for some sessions
        if i % 3 == 0 {
            timer_facet.register_once("retry", Duration::from_secs(retry_delay_secs)).await
                .map_err(|e| anyhow::anyhow!("Failed to register retry for {}: {}", actor_id, e))?;
            total_timers_registered += 1;
        }
        
        if i < 3 || i == num_sessions - 1 {
            info!("  Registered timers for {}", actor_id);
        }
        
        metrics_tracker.increment_message();
    }
    
    let timer_registration_time = timer_registration_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Registered {} timers in {:.2}ms", total_timers_registered, timer_registration_time.as_secs_f64() * 1000.0);
    info!("  Average registration time: {:.2}ms per timer", timer_registration_time.as_secs_f64() * 1000.0 / total_timers_registered as f64);
    println!();

    // =========================================================================
    // Step 4: Simulate Timer Processing (computation phase)
    // =========================================================================
    info!("Step 4: Simulate timer processing");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Simulate timer processing time (computation)
    // In real scenario, timers fire asynchronously and actors process them
    metrics_tracker.start_compute();
    let processing_start = Instant::now();
    
    // Simulate processing timer events (heartbeat fires every 5s, retry fires after 2s)
    // Wait for a short duration to allow some timer events to fire
    let simulation_duration = Duration::from_millis(2500); // 2.5 seconds
    tokio::time::sleep(simulation_duration).await;
    
    let processing_time = processing_start.elapsed();
    metrics_tracker.end_compute();
    
    info!("  Simulated timer processing for {:.2}s", processing_time.as_secs_f64());
    println!();

    // =========================================================================
    // Step 5: Cancel Some Timers (coordination phase)
    // =========================================================================
    info!("Step 5: Cancel timers for some sessions (simulate activity)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let cancel_start = Instant::now();
    
    let mut timers_cancelled = 0u64;
    
    // Cancel idle_timeout for first 20 sessions (simulate user activity)
    for actor_id in session_ids.iter().take(20) {
        // Get TimerFacet from node
        let facets_arc = node
            .service_locator()
            .facet_container_for_actor(actor_id.as_str())
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to get facets for {}", actor_id))?;
        let facets_guard = facets_arc.read().await;
        let timer_facet_arc = facets_guard.get_facet("timer")
            .ok_or_else(|| anyhow::anyhow!("TimerFacet not found for {}", actor_id))?;
        let timer_facet_guard = timer_facet_arc.read().await;
        let timer_facet = timer_facet_guard.as_any()
            .downcast_ref::<TimerFacet>()
            .ok_or_else(|| anyhow::anyhow!("Failed to downcast TimerFacet for {}", actor_id))?;
        
        timer_facet.unregister_timer("idle_timeout").await
            .map_err(|e| anyhow::anyhow!("Failed to cancel idle_timeout for {}: {}", actor_id, e))?;
        timers_cancelled += 1;
        
        // Re-register idle_timeout (reset timer)
        timer_facet.register_once("idle_timeout", Duration::from_secs(idle_timeout_secs)).await
            .map_err(|e| anyhow::anyhow!("Failed to re-register idle_timeout for {}: {}", actor_id, e))?;
        
        drop(timer_facet_guard);
        drop(facets_guard);
        
        metrics_tracker.increment_message();
    }
    
    let cancel_time = cancel_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Cancelled and re-registered {} timers in {:.2}ms", timers_cancelled, cancel_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 6: Performance Metrics
    // =========================================================================
    info!("Step 6: Performance Metrics");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();

    let coordinate_time = Duration::from_millis(metrics.coordinate_duration_ms);
    let compute_time = Duration::from_millis(metrics.compute_duration_ms);
    
    // Calculate benchmark metrics
    let timer_ops_per_sec = total_timers_registered as f64 / total_time.as_secs_f64();
    let sessions_per_sec = num_sessions as f64 / total_time.as_secs_f64();

    info!("Execution Summary:");
    info!("  Total execution time: {:.2}ms ({:.2}s)", 
          total_time.as_secs_f64() * 1000.0,
          total_time.as_secs_f64());
    info!("  Sessions: {}", num_sessions);
    info!("  Timers registered: {}", total_timers_registered);
    info!("  Timers cancelled: {}", timers_cancelled);
    println!();

    info!("Coordination vs Computation Breakdown:");
    info!("  Coordination time: {:.2}ms ({:.1}%)", 
          coordinate_time.as_secs_f64() * 1000.0,
          (coordinate_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Computation time: {:.2}ms ({:.1}%)",
          compute_time.as_secs_f64() * 1000.0,
          (compute_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Efficiency (compute/total): {:.1}%", metrics.efficiency * 100.0);
    println!();

    info!("Timer Operations Metrics:");
    info!("  Total timer operations: {}", metrics.message_count);
    if metrics.message_count > 0 {
        let avg_latency_ms = coordinate_time.as_secs_f64() * 1000.0 / metrics.message_count as f64;
        info!("  Average latency per operation: {:.2}ms", avg_latency_ms);
        info!("  Timer operations per second: {:.1}", timer_ops_per_sec);
        info!("  Sessions per second: {:.1}", sessions_per_sec);
    }
    println!();

    info!("Benchmark Metrics:");
    info!("  Timer operations per second: {:.1}", timer_ops_per_sec);
    info!("  Sessions per second: {:.1}", sessions_per_sec);
    println!();

    info!("Granularity Analysis:");
    if coordinate_time.as_secs_f64() > 0.0 {
        info!("  Granularity ratio (compute/coordinate): {:.2}", metrics.granularity_ratio);
        if metrics.granularity_ratio >= 100.0 {
            info!("  ✅ Excellent granularity (coordination overhead is negligible)");
        } else if metrics.granularity_ratio >= 10.0 {
            info!("  ✅ Good granularity (coordination overhead is low)");
        } else if metrics.granularity_ratio >= 1.0 {
            info!("  ⚠️  Moderate granularity (coordination overhead is noticeable)");
        } else {
            info!("  ❌ Poor granularity (coordination overhead dominates)");
        }
    } else {
        info!("  Granularity ratio: N/A (no coordination overhead)");
    }
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Timers Example Complete");
    println!();
    info!("Key Concepts:");
    info!("  - TimerFacet: Adds timer capability to actors");
    info!("  - One-shot timers: register_once() for idle timeout, retry");
    info!("  - Periodic timers: register_periodic() for heartbeat, polling");
    info!("  - Timer cancellation: cancel() to stop active timers");
    info!("  - In-memory: Timers are NOT persisted (lost on crash)");
    println!();
    info!("PlexSpaces Integration:");
    info!("  - TimerFacet: register_once, register_periodic, cancel");
    info!("  - Timer fires message to actor.handle_message()");
    info!("  - RequestContext: Explicit tenant/namespace (no internal())");
    info!("  - SDK annotations: #[gen_server_actor], #[plexspaces_handlers], #[handler]");
    println!();
    info!("Use Cases:");
    info!("  - Session idle timeout (disconnect after inactivity)");
    info!("  - Heartbeat / keep-alive (periodic ping to client)");
    info!("  - Retry with exponential backoff (retry failed operations)");
    info!("  - Debounce / throttle (rate-limit rapid events)");
    info!("  - Scheduled tasks (periodic cleanup, maintenance)");
    println!();
    info!("Timers vs Reminders:");
    info!("  - TimerFacet: In-memory, high-frequency, short-lived");
    info!("  - ReminderFacet: Durable (DB), long-lived, critical");
    println!();

    Ok(())
}
