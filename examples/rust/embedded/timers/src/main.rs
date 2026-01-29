// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Session Manager Example (TimerFacet)
//
// Demonstrates in-memory timers using PlexSpaces TimerFacet:
// - register_once(): One-shot timer (idle timeout)
// - register_periodic(): Repeating timer (heartbeat)
// - Timer fires message to actor.handle_message()
//
// Use Case: Session timeout, heartbeat, retry with backoff

use async_trait::async_trait;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, RequestContext,
};
use plexspaces_journaling::TimerFacet;
use plexspaces_node::NodeBuilder;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// Session Actor - handles session events and timer callbacks
// =============================================================================

struct SessionActor {
    user_id: String,
    is_active: bool,
    activity_count: u32,
}

impl SessionActor {
    fn new(user_id: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            is_active: true,
            activity_count: 0,
        }
    }
}

#[async_trait]
impl ActorTrait for SessionActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        // Timer events have message_type = "timer_fired"
        if message.message_type == "timer_fired" {
            let timer_name = String::from_utf8_lossy(&message.payload).to_string();
            
            match timer_name.as_str() {
                "idle_timeout" => {
                    println!("  [TIMER] idle_timeout fired - session expired!");
                    self.is_active = false;
                }
                "heartbeat" => {
                    println!("  [TIMER] heartbeat fired - session still active");
                }
                _ => {
                    println!("  [TIMER] {} fired", timer_name);
                }
            }
            return Ok(());
        }

        // Handle regular messages (activity, etc.)
        if message.message_type == "activity" {
            self.activity_count += 1;
            println!("  Activity #{} from {} - would reset idle timer", 
                self.activity_count, self.user_id);
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("SessionActor".to_string())
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Session Manager Example (TimerFacet)                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Demonstrates PlexSpaces TimerFacet for in-memory timers");
    println!();

    // =========================================================================
    // Step 1: Create node
    // =========================================================================
    println!("Step 1: Create node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let node = Arc::new(NodeBuilder::new("session-node").build().await);
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "sessions".to_string());
    
    println!("  Node: session-node");
    println!();

    // =========================================================================
    // Step 2: Create TimerFacet
    // =========================================================================
    println!("Step 2: Create TimerFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let timer_facet = TimerFacet::new(json!({}), 50);
    println!("  TimerFacet created (priority: 50)");
    println!();

    // =========================================================================
    // Step 3: Create actor WITH TimerFacet attached
    // =========================================================================
    println!("Step 3: Create session actor with TimerFacet");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // ActorBuilder.with_facet() attaches facet before spawning
    let _actor_ref = ActorBuilder::new(Box::new(SessionActor::new("user-123")))
        .with_id("session-user-123@session-node")
        .with_namespace("sessions")
        .with_facet(Box::new(timer_facet))  // <-- Attach TimerFacet
        .spawn(&ctx, service_locator.clone())
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    println!("  Actor: session-user-123@session-node");
    println!("  TimerFacet attached via ActorBuilder.with_facet()");
    println!();

    // =========================================================================
    // Step 4: TimerFacet API demonstration
    // =========================================================================
    println!("Step 4: TimerFacet API");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  TimerFacet provides:");
    println!();
    println!("    // One-shot timer (fires once after delay)");
    println!("    facet.register_once(\"idle_timeout\", Duration::from_secs(30)).await?;");
    println!();
    println!("    // Periodic timer (fires repeatedly)");
    println!("    facet.register_periodic(\"heartbeat\", Duration::from_secs(5)).await?;");
    println!();
    println!("    // Cancel a timer");
    println!("    facet.cancel(\"idle_timeout\").await?;");
    println!();
    println!("    // List active timers");
    println!("    let timers = facet.list_active_timers().await;");
    println!();
    println!("  When timer fires, actor receives message with:");
    println!("    message.message_type = \"timer_fired\"");
    println!("    message.payload = timer_name.as_bytes()");
    println!();

    // =========================================================================
    // Step 5: Example timer usage
    // =========================================================================
    println!("Step 5: Example timer workflow");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  User logs in:");
    println!("    → register_once(\"idle_timeout\", 30min)");
    println!("    → register_periodic(\"heartbeat\", 5min)");
    println!();
    println!("  User activity detected:");
    println!("    → cancel(\"idle_timeout\")");
    println!("    → register_once(\"idle_timeout\", 30min)  // Reset");
    println!();
    println!("  Heartbeat timer fires:");
    println!("    → handle_message() receives \"timer_fired\" + \"heartbeat\"");
    println!("    → Send ping to client");
    println!();
    println!("  Idle timeout fires (no activity):");
    println!("    → handle_message() receives \"timer_fired\" + \"idle_timeout\"");
    println!("    → Close session, disconnect user");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Session Manager Example Complete");
    println!();
    println!("PlexSpaces APIs Used:");
    println!("  • TimerFacet - In-memory timer management");
    println!("  • ActorBuilder.with_facet() - Attach facet to actor");
    println!("  • register_once/periodic() - Create timers");
    println!("  • cancel() - Stop a timer");
    println!();
    println!("Key Concepts:");
    println!("  • Timers are IN-MEMORY (lost on crash)");
    println!("  • Timer fires message to actor.handle_message()");
    println!("  • Use ReminderFacet for DURABLE timers");
    println!();
    println!("Use Cases:");
    println!("  • Session idle timeout");
    println!("  • Heartbeat / keep-alive");
    println!("  • Retry with exponential backoff");
    println!("  • Debounce / throttle");
    println!();

    Ok(())
}
