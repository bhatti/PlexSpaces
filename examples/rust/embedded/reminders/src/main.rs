// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Subscription Billing Example (ReminderFacet)
//
// Demonstrates DURABLE reminders using PlexSpaces ReminderFacet:
// - Persisted to storage (survives crashes)
// - Monthly billing cycles
// - Trial expiration notifications
//
// Use Case: SaaS billing, scheduled notifications, renewal reminders

use async_trait::async_trait;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, RequestContext,
};
use plexspaces_journaling::{MemoryJournalStorage, ReminderFacet, ReminderRegistration};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::prost_types;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

// =============================================================================
// Subscription Actor - handles billing events and reminder callbacks
// =============================================================================

struct SubscriptionActor {
    user_id: String,
    plan: String,
    is_active: bool,
}

impl SubscriptionActor {
    fn new(user_id: &str, plan: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            plan: plan.to_string(),
            is_active: true,
        }
    }
}

#[async_trait]
impl ActorTrait for SubscriptionActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        // Reminder events have message_type = "reminder_fired"
        if message.message_type == "reminder_fired" {
            let reminder_name = String::from_utf8_lossy(&message.payload).to_string();
            
            match reminder_name.as_str() {
                "trial_warning" => {
                    println!("  [REMINDER] trial_warning - Trial expires in 3 days!");
                    println!("             Sending email to {}...", self.user_id);
                }
                "trial_expired" => {
                    println!("  [REMINDER] trial_expired - Trial ended");
                    if self.plan == "trial" {
                        self.is_active = false;
                        println!("             Subscription deactivated");
                    }
                }
                "monthly_billing" => {
                    println!("  [REMINDER] monthly_billing - Processing payment");
                    println!("             Charging credit card for {}...", self.user_id);
                    println!("             Payment successful!");
                }
                "renewal_reminder" => {
                    println!("  [REMINDER] renewal_reminder - Annual renewal in 30 days");
                    println!("             Sending renewal notice to {}...", self.user_id);
                }
                _ => {
                    println!("  [REMINDER] {} fired", reminder_name);
                }
            }
            return Ok(());
        }

        // Handle other messages
        println!("  Message received: {}", message.message_type);
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("SubscriptionActor".to_string())
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Subscription Billing Example (ReminderFacet)         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Demonstrates PlexSpaces ReminderFacet for DURABLE reminders");
    println!();

    // =========================================================================
    // Step 1: Create node and storage
    // =========================================================================
    println!("Step 1: Create node and storage");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let node = Arc::new(NodeBuilder::new("billing-node").build().await);
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "billing".to_string());
    
    // ReminderFacet requires JournalStorage for persistence
    let storage = Arc::new(MemoryJournalStorage::new());
    
    println!("  Node: billing-node");
    println!("  Storage: MemoryJournalStorage (use SQLite for production)");
    println!();

    // =========================================================================
    // Step 2: Create ReminderFacet
    // =========================================================================
    println!("Step 2: Create ReminderFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let reminder_facet = ReminderFacet::new(storage.clone(), json!({}), 50);
    println!("  ReminderFacet created (backed by JournalStorage)");
    println!();

    // =========================================================================
    // Step 3: Create actor WITH ReminderFacet attached
    // =========================================================================
    println!("Step 3: Create subscription actor with ReminderFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let _actor_ref = ActorBuilder::new(Box::new(SubscriptionActor::new("user-456", "trial")))
        .with_id("subscription-user-456@billing-node")
        .with_namespace("billing")
        .with_facet(Box::new(reminder_facet))  // <-- Attach ReminderFacet
        .spawn(&ctx, service_locator.clone())
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    println!("  Actor: subscription-user-456@billing-node");
    println!("  ReminderFacet attached via ActorBuilder.with_facet()");
    println!();

    // =========================================================================
    // Step 4: ReminderFacet API demonstration
    // =========================================================================
    println!("Step 4: ReminderFacet API");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  ReminderFacet provides:");
    println!();
    println!("    // Register a durable reminder");
    println!("    facet.register_reminder(ReminderRegistration {{");
    println!("        actor_id: \"subscription-user-456@billing-node\".to_string(),");
    println!("        reminder_name: \"monthly_billing\".to_string(),");
    println!("        interval: Some(Duration::from_days(30)),");
    println!("        first_fire_time: Some(now + Duration::from_days(30)),");
    println!("        persist_across_activations: true,  // <-- DURABLE!");
    println!("        max_occurrences: 0,  // 0 = unlimited");
    println!("        callback_data: vec![],");
    println!("    }}).await?;");
    println!();
    println!("    // Unregister a reminder");
    println!("    facet.unregister_reminder(\"monthly_billing\").await?;");
    println!();
    println!("    // List all reminders");
    println!("    let reminders = facet.list_reminders().await;");
    println!();
    println!("  When reminder fires, actor receives message with:");
    println!("    message.message_type = \"reminder_fired\"");
    println!("    message.payload = reminder_name.as_bytes()");
    println!();

    // =========================================================================
    // Step 5: Example billing workflow
    // =========================================================================
    println!("Step 5: Example billing workflow");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  User signs up for trial:");
    println!("    → register_reminder(trial_warning, delay: 11 days)");
    println!("    → register_reminder(trial_expired, delay: 14 days)");
    println!();
    println!("  User upgrades to paid:");
    println!("    → unregister_reminder(trial_warning)");
    println!("    → unregister_reminder(trial_expired)");
    println!("    → register_reminder(monthly_billing, interval: 30 days)");
    println!();
    println!("  Reminder fires (even after crash!):");
    println!("    → handle_message() receives \"reminder_fired\" + name");
    println!("    → Process billing, send notifications, etc.");
    println!();
    println!("  User cancels:");
    println!("    → unregister_reminder(monthly_billing)");
    println!();

    // =========================================================================
    // Step 6: Show ReminderRegistration structure
    // =========================================================================
    println!("Step 6: ReminderRegistration structure");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Create example registration
    let now = SystemTime::now();
    let first_fire = now + Duration::from_secs(30 * 24 * 60 * 60); // 30 days
    
    let _example_registration = ReminderRegistration {
        actor_id: "subscription-user-456@billing-node".to_string(),
        reminder_name: "monthly_billing".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 30 * 24 * 60 * 60, // 30 days in seconds
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(first_fire)),
        callback_data: vec![],
        persist_across_activations: true, // DURABLE - survives crashes
        max_occurrences: 0, // 0 = unlimited
    };
    
    println!();
    println!("  ReminderRegistration {{");
    println!("    actor_id: \"subscription-user-456@billing-node\"");
    println!("    reminder_name: \"monthly_billing\"");
    println!("    interval: 30 days");
    println!("    first_fire_time: now + 30 days");
    println!("    persist_across_activations: true  ← KEY DIFFERENCE from Timer!");
    println!("    max_occurrences: 0 (unlimited)");
    println!("  }}");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Subscription Billing Example Complete");
    println!();
    println!("PlexSpaces APIs Used:");
    println!("  • ReminderFacet - Durable reminder management");
    println!("  • JournalStorage - Persistence backend (Memory/SQLite/Postgres)");
    println!("  • ReminderRegistration - Reminder configuration");
    println!("  • ActorBuilder.with_facet() - Attach facet to actor");
    println!();
    println!("Key Concepts:");
    println!("  • Reminders are DURABLE (persisted to storage)");
    println!("  • Survive crashes and restarts");
    println!("  • persist_across_activations = true");
    println!("  • Use TimerFacet for in-memory, high-frequency timers");
    println!();
    println!("Timer vs Reminder:");
    println!("  ┌─────────────┬──────────────┬────────────────┐");
    println!("  │ Feature     │ TimerFacet   │ ReminderFacet  │");
    println!("  ├─────────────┼──────────────┼────────────────┤");
    println!("  │ Persistence │ In-memory    │ Durable (DB)   │");
    println!("  │ Survives    │ No           │ Yes            │");
    println!("  │ Use case    │ Heartbeat    │ Billing        │");
    println!("  └─────────────┴──────────────┴────────────────┘");
    println!();
    println!("Use Cases:");
    println!("  • Monthly/annual billing");
    println!("  • Trial expiration");
    println!("  • Renewal notifications");
    println!("  • Scheduled reports");
    println!("  • SLA reminders");
    println!();

    Ok(())
}
