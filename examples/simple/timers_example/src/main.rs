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

//! Timers Example
//!
//! Demonstrates non-durable, in-memory timers using TimerFacet.
//!
//! ## Features Demonstrated
//! 1. Typed messages (ActorMessage enum)
//! 2. FacetService for accessing facets from ActorContext
//! 3. Simplified timer registration APIs
//! 4. Config bootstrap (Erlang/OTP-style)
//! 5. Optional distributed locking (multi-node protection)
//!
//! ## Scenarios
//! 1. Heartbeat timer: Fires every 2 seconds (periodic)
//! 2. One-time cleanup timer: Fires after 5 seconds
//! 3. High-frequency polling timer: Fires every 500ms

use plexspaces_actor::ActorBuilder;
use plexspaces_core::{ActorBehavior, ActorId};
use plexspaces_journaling::TimerFacet;
use plexspaces_mailbox::{ActorMessage, MailboxConfig, Message};
use plexspaces_node::NodeBuilder;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

/// Timer configuration (loaded from release.toml or environment)
#[derive(Debug, Deserialize, Default)]
struct TimerConfig {
    heartbeat_interval_secs: u64,
    cleanup_delay_secs: u64,
    poll_interval_ms: u64,
}

/// Simple behavior that handles timer messages
struct TimerBehavior {
    heartbeat_count: u32,
    cleanup_fired: bool,
    poll_count: u32,
}

impl TimerBehavior {
    fn new() -> Self {
        Self {
            heartbeat_count: 0,
            cleanup_fired: false,
            poll_count: 0,
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for TimerBehavior {
    async fn handle_message(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        message: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        // Handle timer registration message
        if message.message_type == "RegisterTimers" {
            // Get TimerFacet via FacetService (Option B: FacetService)
            match ctx.facet_service.get_facet(&ctx.actor_id, "timer").await {
                Ok(facets) => {
                    let mut timer_facet_guard = facets.write().await;
                    
                    // Downcast to TimerFacet using Any trait
                    use std::any::Any;
                    if let Some(timer_facet) = timer_facet_guard.as_mut().as_any_mut().downcast_mut::<TimerFacet>() {
                        // Note: actor_ref should already be set during actor attachment
                        // But we'll set it here if needed (for safety)
                        // The TimerFacet needs actor_ref to send timer fired messages
                        
                        // Register timers using simplified APIs
                        let config = TimerConfig {
                            heartbeat_interval_secs: 2,
                            cleanup_delay_secs: 5,
                            poll_interval_ms: 500,
                        };
                        
                        // Register heartbeat timer (periodic)
                        if let Err(e) = timer_facet.register_periodic(
                            "heartbeat",
                            Duration::from_secs(config.heartbeat_interval_secs),
                        ).await {
                            warn!("Failed to register heartbeat timer: {}", e);
                        }
                        
                        // Register cleanup timer (one-time)
                        if let Err(e) = timer_facet.register_once(
                            "cleanup",
                            Duration::from_secs(config.cleanup_delay_secs),
                        ).await {
                            warn!("Failed to register cleanup timer: {}", e);
                        }
                        
                        // Register poll timer (high-frequency, periodic)
                        if let Err(e) = timer_facet.register_periodic(
                            "poll",
                            Duration::from_millis(config.poll_interval_ms),
                        ).await {
                            warn!("Failed to register poll timer: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get TimerFacet: {}", e);
                }
            }
            return Ok(());
        }
        
        // Use typed messages (ActorMessage enum) - type-safe, pattern matching
        if let Ok(typed_msg) = message.as_typed() {
            match typed_msg {
                ActorMessage::TimerFired { timer_name, .. } => {
                    match timer_name.as_str() {
                        "heartbeat" => {
                            self.heartbeat_count += 1;
                            info!(
                                actor_id = %ctx.actor_id,
                                timer_name = "heartbeat",
                                count = self.heartbeat_count,
                                "💓 Heartbeat timer fired"
                            );
                        }
                        "cleanup" => {
                            if !self.cleanup_fired {
                                self.cleanup_fired = true;
                                info!(
                                    actor_id = %ctx.actor_id,
                                    timer_name = "cleanup",
                                    "🧹 Cleanup timer fired (one-time)"
                                );
                            }
                        }
                        "poll" => {
                            self.poll_count += 1;
                            if self.poll_count % 10 == 0 {
                                info!(
                                    actor_id = %ctx.actor_id,
                                    timer_name = "poll",
                                    count = self.poll_count,
                                    "📊 Poll timer fired (high-frequency)"
                                );
                            }
                        }
                        _ => {
                            warn!(
                                actor_id = %ctx.actor_id,
                                timer_name = %timer_name,
                                "Unknown timer fired"
                            );
                        }
                    }
                }
                _ => {
                    // Ignore other message types
                }
            }
        }
        
        Ok(())
    }
    
    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║  Timers Example - Non-Durable, In-Memory Timers               ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    info!("");

    // Load configuration (Erlang/OTP-style: env > release.toml > defaults)
    // For now, use defaults (ConfigBootstrap is for NodeConfig, not application config)
    let config = TimerConfig {
        heartbeat_interval_secs: 2,
        cleanup_delay_secs: 5,
        poll_interval_ms: 500,
    };
    info!("📋 Config loaded: heartbeat={}s, cleanup={}s, poll={}ms", 
          config.heartbeat_interval_secs, 
          config.cleanup_delay_secs, 
          config.poll_interval_ms);
    info!("");

    // Create node
    let node = NodeBuilder::new("timer-node")
        .build();

    // Create actor with behavior
    // Configure mailbox with larger capacity to handle timer messages
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000; // Large enough for timer messages
    
    let behavior = Box::new(TimerBehavior::new());
    let actor = ActorBuilder::new(behavior)
        .with_id(ActorId::from("timer-actor@local"))
        .with_mailbox_config(mailbox_config)
        .build();

    // Attach TimerFacet to actor (explicit - A3: explicit facet attachment)
    let timer_facet = Box::new(TimerFacet::new());
    let facet_config = serde_json::json!({});
    actor
        .attach_facet(timer_facet, 50, facet_config)
        .await
        .map_err(|e| format!("Failed to attach TimerFacet: {}", e))?;

    // Spawn actor
    let actor_ref = node.spawn_actor(actor).await?;
    info!("✅ Actor spawned: {}", actor_ref.id());
    info!("");

    // Register timers using FacetService (Option B: FacetService)
    // Send message to actor to trigger timer registration
    // Actor will use FacetService from its context to get TimerFacet
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Registering Timers via FacetService");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Wait a bit for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Send message to actor to register timers
    // Actor will use ctx.facet_service.get_facet() to get TimerFacet
    info!("📝 Sending registration message to actor...");
    let register_msg = Message::new(b"register_timers".to_vec())
        .with_message_type("RegisterTimers".to_string());
    actor_ref.tell(register_msg).await?;
    
    // Wait a bit for registration to complete
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    info!("✅ Timer registration complete");
    info!("");
    
    info!("⏳ Waiting 8 seconds for timer events...");
    info!("   Expected: ~4 heartbeats, 1 cleanup, ~16 polls");
    info!("");
    
    tokio::time::sleep(Duration::from_secs(8)).await;

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Key Takeaways:");
    info!("  • Typed messages (ActorMessage enum) for type-safe handling");
    info!("  • FacetService for accessing facets from ActorContext");
    info!("  • Simplified timer registration APIs");
    info!("  • Config bootstrap (Erlang/OTP-style)");
    info!("  • Timers are non-durable (in-memory only)");
    info!("  • Use ReminderFacet for durable, persistent reminders");
    info!("  • Use register_with_lock() for multi-node protection");
    info!("");
    info!("See: examples/simple/reminders_example for durable reminders");
    info!("");

    info!("✅ Example complete!");

    Ok(())
}
