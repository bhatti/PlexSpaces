// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Subscription Billing Example - Timer vs Reminder Facets
//
// Demonstrates the difference between TRANSIENT timers and DURABLE reminders:
//
// ## TimerFacet (Transient)
// - In-memory only, lost on crash/restart
// - Fast, no storage overhead
// - Use for: heartbeats, health checks, short-lived timeouts
//
// ## ReminderFacet (Durable)
// - Persisted to JournalStorage, survives crashes
// - Requires storage backend (SQLite, Postgres, etc.)
// - Use for: billing, SLA enforcement, scheduled tasks
//
// ## Industry Standard (Orleans Model)
// This design follows the Orleans/Akka convention where:
// - "Timer" implies transient (in-memory)
// - "Reminder" implies durable (persisted)
//
// ## SDK Annotations Used
// - `#[gen_server_actor(name = "subscription_actor", facets = ["timer", "reminder"])]` - both facets
// - `#[plexspaces_handlers]` - generates GenServer dispatch
// - `#[handler("timer_fired", cast)]` - handle transient timer events
// - `#[handler("reminder_fired", cast)]` - handle durable reminder events
//
// Use Case: Subscription billing system with trial management, monthly billing, and health monitoring

use plexspaces_core::{JournalStorage, ServiceLocator};
use plexspaces_facet::Facet;
use plexspaces_journaling::{ReminderFacet, ReminderRegistration, SqliteJournalStorage, TimerFacet};
use plexspaces_node::{CoordinationComputeTracker, NodeBuilder};
use plexspaces_proto::prost_types;
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, spawn_with_facets, ActorContext, ActorId,
    BehaviorError, Message, RequestContext,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use anyhow::Result;

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub name: String,
    pub price_cents: u64,
    pub billing_interval_days: u32,
    pub trial_days: u32,
}

impl SubscriptionPlan {
    pub fn trial() -> Self {
        Self {
            name: "trial".to_string(),
            price_cents: 0,
            billing_interval_days: 0,
            trial_days: 14,
        }
    }

    pub fn basic() -> Self {
        Self {
            name: "basic".to_string(),
            price_cents: 999, // $9.99
            billing_interval_days: 30,
            trial_days: 0,
        }
    }

    pub fn premium() -> Self {
        Self {
            name: "premium".to_string(),
            price_cents: 2999, // $29.99
            billing_interval_days: 30,
            trial_days: 0,
        }
    }
}

// =============================================================================
// Subscription Actor - Demonstrates BOTH Timer and Reminder Facets
// =============================================================================

/// Subscription actor demonstrating both transient timers and durable reminders.
///
/// ## Facets
/// - `TimerFacet` - Transient timers for heartbeats (in-memory, lost on crash)
/// - `ReminderFacet` - Durable reminders for billing (persisted, survives crash)
///
/// ## Design Pattern (Orleans Model)
/// - **Timer** = transient, fast, no persistence overhead
/// - **Reminder** = durable, requires storage, survives crashes
#[gen_server_actor(name = "subscription_actor", facets = ["timer", "reminder"])]
struct SubscriptionActor {
    user_id: String,
    email: String,
    plan: SubscriptionPlan,
    is_active: bool,
    billing_count: u32,
    heartbeat_count: u32,
    reminder_fire_count: u32,
    timer_fire_count: u32,
    last_billed_at: Option<SystemTime>,
    last_heartbeat_at: Option<SystemTime>,
}

impl SubscriptionActor {
    fn new(user_id: &str, email: &str, plan: SubscriptionPlan) -> Self {
        Self {
            user_id: user_id.to_string(),
            email: email.to_string(),
            plan,
            is_active: true,
            billing_count: 0,
            heartbeat_count: 0,
            reminder_fire_count: 0,
            timer_fire_count: 0,
            last_billed_at: None,
            last_heartbeat_at: None,
        }
    }
}

/// Handler implementations for both timer and reminder events.
#[plexspaces_handlers]
impl SubscriptionActor {
    /// Handle TRANSIENT timer events (from TimerFacet)
    ///
    /// TimerFacet fires "timer_fired" messages for in-memory timers.
    /// These are LOST on crash - use for non-critical operations.
    #[handler("timer_fired", cast)]
    async fn handle_timer_fired(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let timer_name = msg
            .headers
            .get("timer_name")
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        self.timer_fire_count += 1;

        match timer_name {
            "heartbeat" => {
                self.heartbeat_count += 1;
                self.last_heartbeat_at = Some(SystemTime::now());
            }
            "session_check" => {
                // Session verification logic
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle DURABLE reminder events (from ReminderFacet)
    ///
    /// ReminderFacet fires "reminder_fired" messages for persisted reminders.
    /// These SURVIVE crashes - use for critical operations.
    #[handler("reminder_fired", cast)]
    async fn handle_reminder_fired(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let reminder_name = msg
            .headers
            .get("reminder_name")
            .map(|s| s.as_str())
            .unwrap_or("unknown");

        self.reminder_fire_count += 1;

        match reminder_name {
            "trial_warning" => {
                // Send email notification
            }
            "trial_expired" => {
                if self.plan.name == "trial" {
                    self.is_active = false;
                }
            }
            "monthly_billing" => {
                self.billing_count += 1;
                self.last_billed_at = Some(SystemTime::now());
                // Process payment
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle subscription upgrade requests
    #[handler("upgrade", cast)]
    async fn handle_upgrade(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let new_plan: SubscriptionPlan = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid plan: {}", e)))?;

        self.plan = new_plan;
        self.is_active = true;
        Ok(())
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive("reminders=info".parse()?))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║  Subscription Billing: Timer vs Reminder Facets                ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Demonstrates industry-standard distinction:");
    info!("  • TimerFacet    = TRANSIENT (in-memory, fast, lost on crash)");
    info!("  • ReminderFacet = DURABLE (persisted, survives crashes)");
    println!();

    // Configuration
    let num_subscriptions = 50;
    let heartbeat_interval_secs = 5;
    let billing_interval_secs = 10; // Shortened for demo
    let trial_warning_delay_secs = 8;

    // =========================================================================
    // Step 1: Create node and storage
    // =========================================================================
    info!("Step 1: Create node and JournalStorage for durable reminders");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let node = NodeBuilder::new("billing-node").build_started().await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "billing".to_string());

    // Storage is ONLY needed for ReminderFacet (durable)
    // TimerFacet requires NO storage (transient)
    let storage: Arc<dyn JournalStorage> =
        Arc::new(SqliteJournalStorage::new(":memory:").await?);

    info!("  Node: billing-node");
    info!("  Storage: Arc<dyn JournalStorage> (for ReminderFacet only)");
    info!("  TimerFacet: No storage needed (in-memory)");
    println!();

    // =========================================================================
    // Step 2: Create subscriptions with both facets
    // =========================================================================
    info!("Step 2: Create {} subscriptions with TimerFacet and ReminderFacet", num_subscriptions);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut metrics_tracker = CoordinationComputeTracker::new("reminders".to_string());
    let total_start = Instant::now();
    metrics_tracker.start_coordinate();
    let spawn_start = Instant::now();

    let mut subscription_ids: Vec<ActorId> = Vec::new();

    for i in 0..num_subscriptions {
        let user_id = format!("user-{}", i);
        let email = format!("user{}@example.com", i);
        let plan = if i % 3 == 0 {
            SubscriptionPlan::trial()
        } else if i % 3 == 1 {
            SubscriptionPlan::basic()
        } else {
            SubscriptionPlan::premium()
        };

        let actor_name = format!("subscription-{}", user_id);
        
        // Create both facets for each subscription
        let timer_facet = Box::new(TimerFacet::new(serde_json::json!({}), 50, service_locator.clone())) as Box<dyn Facet>;
        let reminder_facet = Box::new(ReminderFacet::new(
            storage.clone(),
            serde_json::json!({"check_interval_ms": 100}),
            50,
            service_locator.clone(),
        )) as Box<dyn Facet>;

        let actor = SubscriptionActor::new(&user_id, &email, plan);

        // Spawn actor with both facets
        let actor_ref = spawn_with_facets(
            &ctx,
            service_locator.clone(),
            actor_name,
            "billing",
            actor,
            vec![timer_facet, reminder_facet],
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn subscription actor for {}: {}", user_id, e))?;

        subscription_ids.push(actor_ref.id().clone());
        metrics_tracker.increment_message();

        if i < 3 || i == num_subscriptions - 1 {
            info!("  Spawned subscription actor: {}", subscription_ids[i]);
        }
    }

    let spawn_time = spawn_start.elapsed();
    metrics_tracker.end_coordinate();

    info!("  Created {} subscription actors in {:.2}ms", num_subscriptions, spawn_time.as_secs_f64() * 1000.0);
    info!("  Average spawn time: {:.2}ms per actor", spawn_time.as_secs_f64() * 1000.0 / num_subscriptions as f64);
    println!();

    // =========================================================================
    // Step 3: Register timers and reminders
    // =========================================================================
    info!("Step 3: Register timers (transient) and reminders (durable)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_coordinate();
    let registration_start = Instant::now();

    let mut total_timers_registered = 0u64;
    let mut total_reminders_registered = 0u64;

    for (i, actor_id) in subscription_ids.iter().enumerate() {
        // Get facets from node
        let facets_arc = node
            .service_locator()
            .facet_container_for_actor(actor_id.as_str())
            .await
            .ok_or_else(|| anyhow::anyhow!("Facets not found for actor {}", actor_id))?;

        let facets_guard = facets_arc.read().await;
        
        // Register TimerFacet (transient heartbeat)
        if let Some(timer_facet_arc) = facets_guard.get_facet("timer") {
            let timer_facet_guard = timer_facet_arc.read().await;
            if let Some(timer_facet) = timer_facet_guard.as_any().downcast_ref::<TimerFacet>() {
                timer_facet.register_periodic("heartbeat", Duration::from_secs(heartbeat_interval_secs)).await
                    .map_err(|e| anyhow::anyhow!("Failed to register heartbeat timer for {}: {}", actor_id, e))?;
                total_timers_registered += 1;
            }
        }

        // Register ReminderFacet (durable billing)
        if let Some(reminder_facet_arc) = facets_guard.get_facet("reminder") {
            let reminder_facet_guard = reminder_facet_arc.read().await;
            if let Some(reminder_facet) = reminder_facet_guard.as_any().downcast_ref::<ReminderFacet>() {
                // Monthly billing reminder (durable)
                let billing_registration = ReminderRegistration {
                    actor_id: actor_id.to_string(),
                    reminder_name: "monthly_billing".to_string(),
                    interval: Some(prost_types::Duration {
                        seconds: billing_interval_secs as i64,
                        nanos: 0,
                    }),
                    first_fire_time: None,
                    callback_data: vec![],
                    persist_across_activations: true, // KEY: Survives crashes!
                    max_occurrences: 0, // Unlimited
                };
                reminder_facet.register_reminder(billing_registration).await
                    .map_err(|e| anyhow::anyhow!("Failed to register billing reminder for {}: {}", actor_id, e))?;
                total_reminders_registered += 1;

                // Trial warning reminder (for trial subscriptions only)
                if i % 3 == 0 {
                    let trial_warning_registration = ReminderRegistration {
                        actor_id: actor_id.to_string(),
                        reminder_name: "trial_warning".to_string(),
                        interval: Some(prost_types::Duration {
                            seconds: trial_warning_delay_secs as i64,
                            nanos: 0,
                        }),
                        first_fire_time: None,
                        callback_data: vec![],
                        persist_across_activations: true,
                        max_occurrences: 1, // One-time warning
                    };
                    reminder_facet.register_reminder(trial_warning_registration).await
                        .map_err(|e| anyhow::anyhow!("Failed to register trial warning for {}: {}", actor_id, e))?;
                    total_reminders_registered += 1;
                }
            }
        }

        if i < 3 || i == num_subscriptions - 1 {
            info!("  Registered timers and reminders for {}", actor_id);
        }

        metrics_tracker.increment_message();
    }

    let registration_time = registration_start.elapsed();
    metrics_tracker.end_coordinate();

    info!("  Registered {} timers and {} reminders in {:.2}ms", 
        total_timers_registered, total_reminders_registered, registration_time.as_secs_f64() * 1000.0);
    info!("  Average registration time: {:.2}ms per subscription", 
        registration_time.as_secs_f64() * 1000.0 / num_subscriptions as f64);
    println!();

    // =========================================================================
    // Step 4: Simulate processing (computation phase)
    // =========================================================================
    info!("Step 4: Simulate reminder and timer processing");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    metrics_tracker.start_compute();
    let processing_start = Instant::now();

    // Simulate processing time (reminders check interval is 100ms, timers fire periodically)
    tokio::time::sleep(Duration::from_secs(12)).await;

    let processing_time = processing_start.elapsed();
    metrics_tracker.end_compute();

    info!("  Simulated processing for {:.2}s", processing_time.as_secs_f64());
    println!();

    // =========================================================================
    // Step 5: Metrics Output
    // =========================================================================
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Execution Summary");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Total subscriptions: {}", num_subscriptions);
    info!("  Timers registered: {} (transient heartbeats)", total_timers_registered);
    info!("  Reminders registered: {} (durable billing)", total_reminders_registered);
    info!("  Total operations: {}", total_timers_registered + total_reminders_registered);
    println!();

    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();

    let coordinate_time = Duration::from_millis(metrics.coordinate_duration_ms);
    let compute_time = Duration::from_millis(metrics.compute_duration_ms);
    let total_time_secs = total_time.as_secs_f64();
    
    info!("Coordination vs Computation Breakdown:");
    let coordination_pct = (coordinate_time.as_secs_f64() / total_time_secs) * 100.0;
    let computation_pct = (compute_time.as_secs_f64() / total_time_secs) * 100.0;
    info!("  Coordination time: {:.2}ms ({:.1}%)", 
        coordinate_time.as_secs_f64() * 1000.0, coordination_pct);
    info!("  Computation time: {:.2}ms ({:.1}%)", 
        compute_time.as_secs_f64() * 1000.0, computation_pct);
    info!("  Total time: {:.2}ms ({:.2}s)", total_time_secs * 1000.0, total_time_secs);
    info!("  Efficiency (compute/total): {:.1}%", metrics.efficiency * 100.0);
    println!();

    let total_ops = total_timers_registered + total_reminders_registered;
    if total_time_secs > 0.0 {
        info!("Benchmark Metrics:");
        info!("  Operations per second: {:.1}", total_ops as f64 / total_time_secs);
        info!("  Subscriptions per second: {:.1}", num_subscriptions as f64 / total_time_secs);
    }
    println!();

    let granularity_ratio = if coordinate_time.as_secs_f64() > 0.0 {
        compute_time.as_secs_f64() / coordinate_time.as_secs_f64()
    } else {
        0.0
    };
    info!("Granularity Analysis:");
    info!("  Granularity ratio (compute/coordinate): {:.2}", granularity_ratio);
    if granularity_ratio >= 10.0 {
        info!("  ✅ Good granularity (coordination overhead is low)");
    } else {
        info!("  ⚠️  Low granularity (coordination overhead is high)");
    }
    println!();

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Timer vs Reminder Best Practices");
    println!();
    info!("PlexSpaces follows the Orleans model:");
    println!();
    info!("  TimerFacet (TRANSIENT):");
    info!("    • No storage required");
    info!("    • Fast, low overhead");
    info!("    • Lost on crash");
    info!("    • Use for: heartbeats, health checks, timeouts");
    println!();
    info!("  ReminderFacet (DURABLE):");
    info!("    • Requires Arc<dyn JournalStorage>");
    info!("    • Persisted to database");
    info!("    • Survives crashes");
    info!("    • Use for: billing, SLA, scheduled tasks");
    println!();
    info!("Key Insight:");
    info!("  The naming convention IS the API contract:");
    info!("    • Timer = transient (always)");
    info!("    • Reminder = durable (always)");
    info!("  No configuration flag needed - the name tells you the semantics!");

    Ok(())
}
