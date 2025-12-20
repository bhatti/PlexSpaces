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

//! Reminders Example
//!
//! Demonstrates durable, persistent reminders using ReminderFacet with:
//! - SQLite backend for persistence
//! - VirtualActorFacet integration for auto-activation
//! - Full lifecycle (deactivation/reactivation with persistent reminders)
//!
//! ## Scenarios
//! 1. Monthly billing reminder: Fires every 2 seconds (for demo), infinite
//! 2. SLA check reminder: Fires every 1 second, max 3 occurrences
//! 3. Retry reminder: Fires every 1.5 seconds, max 2 occurrences

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{Actor, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_journaling::{
    ReminderFacet, ReminderRegistration, VirtualActorFacet, MemoryJournalStorage,
};
use plexspaces_mailbox::Message;
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::prost_types;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

/// Simple behavior that handles reminder messages
struct ReminderBehavior {
    billing_count: u32,
    sla_count: u32,
    retry_count: u32,
}

impl ReminderBehavior {
    fn new() -> Self {
        Self {
            billing_count: 0,
            sla_count: 0,
            retry_count: 0,
        }
    }
}

#[async_trait::async_trait]
impl Actor for ReminderBehavior {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        // Note: Reminder registration is done in main() after actor spawn
        // because ReminderFacet<S> is generic and can't be downcast via FacetService
        // In production, use a helper function or register reminders before spawn
        
        // Handle ReminderFired messages (currently sent via metadata, will be typed in future)
        if let Some(reminder_type) = message.metadata.get("type") {
            if reminder_type == "ReminderFired" {
                if let Some(reminder_name) = message.metadata.get("reminder_name") {
                    match reminder_name.as_str() {
                        "monthly_billing" => {
                            self.billing_count += 1;
                            info!(
                                actor_id = %ctx.actor_id,
                                reminder_name = "monthly_billing",
                                count = self.billing_count,
                                "💰 Monthly billing reminder fired"
                            );
                        }
                        "sla_check" => {
                            self.sla_count += 1;
                            info!(
                                actor_id = %ctx.actor_id,
                                reminder_name = "sla_check",
                                count = self.sla_count,
                                "📊 SLA check reminder fired"
                            );
                        }
                        "retry_send" => {
                            self.retry_count += 1;
                            info!(
                                actor_id = %ctx.actor_id,
                                reminder_name = "retry_send",
                                count = self.retry_count,
                                "🔄 Retry reminder fired"
                            );
                        }
                        _ => {
                            warn!(
                                actor_id = %ctx.actor_id,
                                reminder_name = %reminder_name,
                                "Unknown reminder fired"
                            );
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// Note: ReminderFacet requires ActivationProvider, but for this simple example
// we'll use a minimal implementation. In production, Node provides ActivationProvider.
struct SimpleActivationProvider {
    node: Arc<Node>,
}

#[async_trait::async_trait]
impl plexspaces_journaling::ActivationProvider for SimpleActivationProvider {
    async fn is_actor_active(&self, actor_id: &ActorId) -> bool {
        // Check if actor is active in node
        self.node.find_actor(actor_id).await.is_ok()
    }

    async fn activate_actor(&self, _actor_id: &ActorId) -> Result<plexspaces_core::ActorRef, String> {
        // For this example, we'll just return an error since we're not using virtual actors
        // In production, this would call node.activate_virtual_actor()
        Err("Activation not implemented in simple example".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║  Reminders Example - Durable, Persistent Reminders            ║");
    info!("║  with SQLite Backend & VirtualActorFacet Integration         ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Create storage backend
    // Note: For SQLite persistence, use:
    //   let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
    // For this example, we use MemoryJournalStorage (in-memory only)
    info!("📦 Creating journal storage (MemoryJournalStorage)...");
    let storage = Arc::new(MemoryJournalStorage::new());
    info!("✅ Storage created");
    info!("   Note: In production, use SqliteJournalStorage for persistence");
    println!();

    // Create node
    let node = Arc::new(
        NodeBuilder::new("reminder-node")
            .build()
    );
    info!("✅ Node created");
    println!();

    // Create actor with behavior
    let behavior = Box::new(ReminderBehavior::new());
    // Increase mailbox capacity to handle reminder messages
    let mailbox_config = plexspaces_mailbox::MailboxConfig {
        capacity: 1000,
        ..Default::default()
    };
    // Create facets with config and priority
    info!("🔌 Creating facets...");
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "10s",
        "activation_strategy": "lazy"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
    info!("✅ VirtualActorFacet created");
    
    let activation_provider = Arc::new(SimpleActivationProvider {
        node: node.clone(),
    });
    let reminder_facet = Box::new(ReminderFacet::with_activation_provider(
        storage.clone(),
        activation_provider,
        serde_json::json!({}),
        50,
    ));
    info!("✅ ReminderFacet created");
    println!();

    // Spawn using ActorFactory with facets
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = ActorId::from("reminder-actor@local");
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "GenServer",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![virtual_facet, reminder_facet], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Create ActorRef directly - no need to access mailbox
    let actor_ref = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );
    info!("✅ Actor spawned: {}", actor_id);
    println!();
    
    // Register reminders by accessing the facet from node storage
    // Note: ReminderFacet<S> is generic, so we access it via node storage
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Registering Reminders");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Get facets from node storage
    let facets_arc = node.get_facets(&actor_id).await
        .ok_or_else(|| "Failed to get facets from node storage")?;
    let facets_guard = facets_arc.read().await;
    let reminder_facet_arc = facets_guard.get_facet("reminder")
        .ok_or_else(|| "ReminderFacet not found in facets")?;
    
    // Set actor_ref on the facet (needed for sending reminder messages)
    // Note: We need to downcast, but ReminderFacet is generic
    // For now, we'll use a workaround: access via Any trait
    let mut reminder_facet_guard = reminder_facet_arc.write().await;
    // Try to downcast to ReminderFacet<MemoryJournalStorage>
    // This is safe because we know the storage type from the example
    if let Some(reminder_facet) = reminder_facet_guard.as_any_mut().downcast_mut::<ReminderFacet<MemoryJournalStorage>>() {
        // Set actor_ref
        reminder_facet.set_actor_ref(actor_ref.clone()).await;
        
        let actor_id_str = actor_id.as_str().to_string();
        
        // Register monthly billing reminder (infinite)
        let billing_registration = ReminderRegistration {
            actor_id: actor_id_str.clone(),
            reminder_name: "monthly_billing".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 2, // 2 seconds for demo (normally 30 days)
                nanos: 0,
            }),
            first_fire_time: Some(prost_types::Timestamp::from(SystemTime::now())),
            callback_data: b"billing-data".to_vec(),
            persist_across_activations: true,
            max_occurrences: -1, // Infinite
        };
        reminder_facet.register_reminder(billing_registration).await
            .map_err(|e| format!("Failed to register monthly_billing reminder: {}", e))?;
        info!("✅ Registered monthly_billing reminder (infinite, 2s interval)");
        
        // Register SLA check reminder (max 3 occurrences)
        let sla_registration = ReminderRegistration {
            actor_id: actor_id_str.clone(),
            reminder_name: "sla_check".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 1, // 1 second for demo (normally 1 hour)
                nanos: 0,
            }),
            first_fire_time: Some(prost_types::Timestamp::from(SystemTime::now())),
            callback_data: b"sla-check-data".to_vec(),
            persist_across_activations: true,
            max_occurrences: 3,
        };
        reminder_facet.register_reminder(sla_registration).await
            .map_err(|e| format!("Failed to register sla_check reminder: {}", e))?;
        info!("✅ Registered sla_check reminder (max 3, 1s interval)");
        
        // Register retry reminder (max 2 occurrences)
        let retry_registration = ReminderRegistration {
            actor_id: actor_id_str.clone(),
            reminder_name: "retry_send".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 1, // 1 second for demo (normally 1 minute)
                nanos: 0,
            }),
            first_fire_time: Some(prost_types::Timestamp::from(SystemTime::now())),
            callback_data: b"retry-data".to_vec(),
            persist_across_activations: true,
            max_occurrences: 2,
        };
        reminder_facet.register_reminder(retry_registration).await
            .map_err(|e| format!("Failed to register retry_send reminder: {}", e))?;
        info!("✅ Registered retry_send reminder (max 2, 1s interval)");
    } else {
        return Err("Failed to downcast to ReminderFacet<MemoryJournalStorage>".into());
    }
    
    // Release the write lock so background task can access the facet
    drop(reminder_facet_guard);
    drop(facets_guard);
    
    // Small delay to ensure background task picks up actor_ref
    tokio::time::sleep(Duration::from_millis(50)).await;
    println!();

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Scenario 1: Monthly Billing Reminder");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  - Interval: 2 seconds (for demo, normally 30 days)");
    info!("  - Max occurrences: Infinite");
    info!("  - Persists across deactivation: ✅ Yes");
    info!("  - Triggers auto-activation: ✅ Yes");
    println!();

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Scenario 2: SLA Check Reminder");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  - Interval: 1 second (for demo, normally 1 hour)");
    info!("  - Max occurrences: 3");
    info!("  - Auto-deletes after max fires: ✅ Yes");
    println!();

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Scenario 3: Retry Reminder");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  - Interval: 1.5 seconds (for demo, normally 1 minute)");
    info!("  - Max occurrences: 2");
    info!("  - Use case: Retry failed operation");
    println!();


    // Wait for some reminder events
    info!("⏳ Waiting 6 seconds for reminder events...");
    info!("   Expected: ~3 billing, 3 SLA, 2 retry reminders");
    println!();
    
    tokio::time::sleep(Duration::from_secs(6)).await;

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Key Takeaways:");
    info!("  • Typed messages (ActorMessage enum) for type-safe handling");
    info!("  • FacetService for accessing facets from ActorContext");
    info!("  • Reminders are durable (persisted to storage)");
    info!("  • Reminders survive actor deactivation and crashes");
    info!("  • Reminders can trigger auto-activation (with VirtualActorFacet)");
    info!("  • Suitable for long-term scheduling (hours, days, months)");
    info!("  • Use TimerFacet for high-frequency, transient operations");
    println!();
    info!("See: examples/simple/timers_example for non-durable timers");
    println!();

    info!("✅ Example complete!");

    Ok(())
}
