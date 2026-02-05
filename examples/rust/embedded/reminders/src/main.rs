// SPDX-License-Identifier: LGPL-2.1-or-later
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
// - `#[actor(facets = ["timer", "reminder"])]` - both facets
// - `#[plexspaces_handlers(custom)]` - generates Actor impl
// - `#[handler("timer_fired", cast)]` - handle transient timer events
// - `#[handler("reminder_fired", cast)]` - handle durable reminder events

use plexspaces_core::JournalStorage;
use plexspaces_journaling::{ReminderFacet, ReminderRegistration, SqliteJournalStorage, TimerFacet};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::prost_types;
use plexspaces_proto::timer::v1::TimerRegistration;
use plexspaces_sdk::{
    actor, handler, plexspaces_handlers, spawn_actor, ActorContext, BehaviorError, Message,
    RequestContext,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::SystemTime;

// Required for macro-generated code
extern crate plexspaces_core;

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
///
/// This is the industry standard naming convention:
/// - Orleans, Akka, Dapr all use this Timer vs Reminder distinction
#[actor(facets = ["timer", "reminder"])]
struct SubscriptionActor {
    user_id: String,
    email: String,
    plan: SubscriptionPlan,
    is_active: bool,
    billing_count: u32,
    heartbeat_count: u32,
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
            last_billed_at: None,
            last_heartbeat_at: None,
        }
    }
}

/// Handler implementations for both timer and reminder events.
#[plexspaces_handlers(custom)]
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

        match timer_name {
            "heartbeat" => {
                self.heartbeat_count += 1;
                self.last_heartbeat_at = Some(SystemTime::now());
                println!(
                    "  💓 [TIMER] heartbeat #{} - User {} is active",
                    self.heartbeat_count, self.user_id
                );
                println!("     → TRANSIENT: Lost on crash (in-memory only)");
            }
            "session_check" => {
                println!("  🔍 [TIMER] session_check - Verifying session for {}", self.user_id);
                println!("     → TRANSIENT: No persistence overhead");
            }
            _ => {
                println!("  ⏱️  [TIMER] {} fired (transient)", timer_name);
            }
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

        match reminder_name {
            "trial_warning" => {
                println!("  📧 [REMINDER] trial_warning - DURABLE");
                println!(
                    "     → Sending email to {}: 'Your trial expires in 3 days!'",
                    self.email
                );
                println!("     → SURVIVES CRASH: Persisted to JournalStorage");
            }
            "trial_expired" => {
                println!("  ⏰ [REMINDER] trial_expired - DURABLE");
                if self.plan.name == "trial" {
                    self.is_active = false;
                    println!("     → Trial ended for user: {}", self.user_id);
                    println!("     → SURVIVES CRASH: Will fire even after restart");
                }
            }
            "monthly_billing" => {
                println!("  💳 [REMINDER] monthly_billing - DURABLE");
                self.billing_count += 1;
                self.last_billed_at = Some(SystemTime::now());
                println!(
                    "     → Processing payment #{} for user: {}",
                    self.billing_count, self.user_id
                );
                println!(
                    "     → Charging ${:.2} to card on file",
                    self.plan.price_cents as f64 / 100.0
                );
                println!("     → CRITICAL: Must survive crashes!");
            }
            _ => {
                println!("  🔔 [REMINDER] {} fired (durable)", reminder_name);
            }
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

        println!("  ⬆️  [UPGRADE] {} → {}", self.plan.name, new_plan.name);
        self.plan = new_plan;
        self.is_active = true;
        Ok(())
    }

    /// Handle other messages
    #[handler("message", cast)]
    async fn handle_message_event(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        println!("  📨 Message received: {}", msg.message_type);
        Ok(())
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Timer vs Reminder: Transient vs Durable Scheduling         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("This example demonstrates the industry-standard distinction:");
    println!("  • TimerFacet    = TRANSIENT (in-memory, fast, lost on crash)");
    println!("  • ReminderFacet = DURABLE (persisted, survives crashes)");
    println!();

    // =========================================================================
    // Step 1: Create node and storage
    // =========================================================================
    println!("Step 1: Create node and JournalStorage for durable reminders");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let node = Arc::new(NodeBuilder::new("billing-node").build().await);
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "billing".to_string());

    // Storage is ONLY needed for ReminderFacet (durable)
    // TimerFacet requires NO storage (transient)
    let storage: Arc<dyn JournalStorage> =
        Arc::new(SqliteJournalStorage::new(":memory:").await?);

    println!("  Node: billing-node");
    println!("  Storage: Arc<dyn JournalStorage> (for ReminderFacet only)");
    println!("  TimerFacet: No storage needed (in-memory)");
    println!();

    // =========================================================================
    // Step 2: Create BOTH facets
    // =========================================================================
    println!("Step 2: Create TimerFacet (transient) and ReminderFacet (durable)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // TimerFacet - TRANSIENT (no storage required)
    let timer_facet = TimerFacet::new(serde_json::json!({}), 50);
    println!("  TimerFacet created:");
    println!("    → No storage required");
    println!("    → In-memory only");
    println!("    → Lost on crash/restart");
    println!("    → Use for: heartbeats, health checks, timeouts");
    println!();

    // ReminderFacet - DURABLE (requires storage)
    let reminder_facet = ReminderFacet::new(
        storage.clone(),
        serde_json::json!({"check_interval_ms": 100}),
        50,
    );
    println!("  ReminderFacet created:");
    println!("    → Requires Arc<dyn JournalStorage>");
    println!("    → Persisted to database");
    println!("    → Survives crash/restart");
    println!("    → Use for: billing, SLA, scheduled tasks");
    println!();

    // =========================================================================
    // Step 3: Create actor with BOTH facets
    // =========================================================================
    println!("Step 3: Create actor with both TimerFacet and ReminderFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let actor = SubscriptionActor::new("user-456", "user456@example.com", SubscriptionPlan::trial());

    // Attach BOTH facets to the actor
    let _actor_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        "subscription-user-456@billing-node",
        "billing",
        actor,
        vec![
            Box::new(timer_facet),    // Transient timers
            Box::new(reminder_facet), // Durable reminders
        ],
    )
    .await
    .map_err(|e| format!("Failed to spawn actor: {}", e))?;

    println!("  Actor: subscription-user-456@billing-node");
    println!("  Facets attached:");
    println!("    → TimerFacet (transient heartbeats)");
    println!("    → ReminderFacet (durable billing)");
    println!();

    // =========================================================================
    // Step 4: Timer vs Reminder Registration
    // =========================================================================
    println!("Step 4: Timer vs Reminder Registration APIs");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Show TimerRegistration (transient)
    let _heartbeat_timer = TimerRegistration {
        actor_id: "subscription-user-456@billing-node".to_string(),
        timer_name: "heartbeat".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 30, // Every 30 seconds
            nanos: 0,
        }),
        due_time: None, // Fire immediately
        callback_data: vec![],
        periodic: true, // Repeat periodically
    };

    println!("  TimerRegistration (TRANSIENT):");
    println!("  {{");
    println!("    timer_name: \"heartbeat\"");
    println!("    interval: 30 seconds");
    println!("    periodic: true");
    println!("    // NO persist_across_activations field!");
    println!("    // Timers are ALWAYS transient (in-memory)");
    println!("  }}");
    println!();

    // Show ReminderRegistration (durable)
    let _billing_reminder = ReminderRegistration {
        actor_id: "subscription-user-456@billing-node".to_string(),
        reminder_name: "monthly_billing".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 30 * 24 * 60 * 60, // 30 days
            nanos: 0,
        }),
        first_fire_time: None,
        callback_data: vec![],
        persist_across_activations: true, // KEY: Survives crashes!
        max_occurrences: 0,               // Unlimited
    };

    println!("  ReminderRegistration (DURABLE):");
    println!("  {{");
    println!("    reminder_name: \"monthly_billing\"");
    println!("    interval: 30 days");
    println!("    persist_across_activations: true  ← KEY DIFFERENCE!");
    println!("    // Reminders are persisted to JournalStorage");
    println!("  }}");
    println!();

    // =========================================================================
    // Step 5: Use Case Guidelines
    // =========================================================================
    println!("Step 5: When to Use Timer vs Reminder");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  ┌─────────────────────┬──────────────────┬────────────────────┐");
    println!("  │ Criteria            │ TimerFacet       │ ReminderFacet      │");
    println!("  ├─────────────────────┼──────────────────┼────────────────────┤");
    println!("  │ Persistence         │ In-memory        │ JournalStorage     │");
    println!("  │ Survives crash      │ ❌ No            │ ✅ Yes             │");
    println!("  │ Storage required    │ ❌ No            │ ✅ Yes             │");
    println!("  │ Performance         │ ⚡ Fast          │ 🐢 Slower (I/O)    │");
    println!("  │ Message type        │ timer_fired      │ reminder_fired     │");
    println!("  └─────────────────────┴──────────────────┴────────────────────┘");
    println!();
    println!("  USE TIMER (transient) for:");
    println!("    ✓ Heartbeats / health checks");
    println!("    ✓ Session timeouts");
    println!("    ✓ Debouncing / throttling");
    println!("    ✓ Short-lived operations");
    println!("    ✓ High-frequency events (< 1 second)");
    println!();
    println!("  USE REMINDER (durable) for:");
    println!("    ✓ Billing cycles");
    println!("    ✓ SLA enforcement");
    println!("    ✓ Trial expiration");
    println!("    ✓ Scheduled reports");
    println!("    ✓ Any operation that MUST happen");
    println!();

    // =========================================================================
    // Step 6: Industry Standard
    // =========================================================================
    println!("Step 6: Industry Standard (Orleans Model)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  This Timer vs Reminder distinction is the industry standard:");
    println!();
    println!("  ┌─────────────┬──────────────────┬────────────────────────────┐");
    println!("  │ Framework   │ Transient        │ Durable                    │");
    println!("  ├─────────────┼──────────────────┼────────────────────────────┤");
    println!("  │ Orleans     │ Timer            │ Reminder                   │");
    println!("  │ Akka        │ Scheduler        │ Akka Persistence           │");
    println!("  │ Dapr        │ -                │ Reminder (only durable)    │");
    println!("  │ Temporal    │ -                │ All durable                │");
    println!("  │ PlexSpaces  │ TimerFacet       │ ReminderFacet              │");
    println!("  └─────────────┴──────────────────┴────────────────────────────┘");
    println!();
    println!("  The naming convention communicates durability semantics:");
    println!("    • \"Timer\" → think stopwatch, temporary, in-memory");
    println!("    • \"Reminder\" → think calendar, persistent, database");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Summary: Timer vs Reminder Best Practices");
    println!();
    println!("PlexSpaces follows the Orleans model:");
    println!();
    println!("  TimerFacet (TRANSIENT):");
    println!("    • No storage required");
    println!("    • Fast, low overhead");
    println!("    • Lost on crash");
    println!("    • #[handler(\"timer_fired\", cast)]");
    println!();
    println!("  ReminderFacet (DURABLE):");
    println!("    • Requires Arc<dyn JournalStorage>");
    println!("    • Persisted to database");
    println!("    • Survives crashes");
    println!("    • #[handler(\"reminder_fired\", cast)]");
    println!();
    println!("SDK Annotations:");
    println!("  #[actor(facets = [\"timer\"])]        → TimerFacet only");
    println!("  #[actor(facets = [\"reminder\"])]     → ReminderFacet only");
    println!("  #[actor(facets = [\"timer\", \"reminder\"])] → Both facets");
    println!();
    println!("Key Insight:");
    println!("  The naming convention IS the API contract:");
    println!("    • Timer = transient (always)");
    println!("    • Reminder = durable (always)");
    println!("  No configuration flag needed - the name tells you the semantics!");
    println!();

    Ok(())
}
