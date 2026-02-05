// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Session Manager Example (TimerFacet) - SDK Annotations Demo
//
// Demonstrates in-memory timers using PlexSpaces TimerFacet:
// - register_once(): One-shot timer (idle timeout)
// - register_periodic(): Repeating timer (heartbeat)
// - Timer fires message to actor.handle_message()
//
// ## SDK Annotations Used
// - `#[actor(facets = ["timer"])]` - marks struct as actor with timer facet
// - `#[plexspaces_handlers(custom)]` - generates Actor impl for custom behavior
// - `#[handler("timer_fired", cast)]` - fire-and-forget handler for timers
//
// Use Case: Session timeout, heartbeat, retry with backoff

use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, RequestContext, spawn_actor, TimerFacet,
};
use plexspaces_node::NodeBuilder;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

// Required for macro-generated code
extern crate plexspaces_core;

// =============================================================================
// Session Actor - SDK annotations style (like Python @actor, @handler)
// =============================================================================

/// Session actor with timer facet for idle timeout and heartbeat.
/// 
/// ## Annotations
/// - `#[actor(facets = ["timer"])]` - declares timer facet (documentation)
/// - `#[plexspaces_handlers(custom)]` - generates Actor impl dispatching to handlers
/// - `#[handler("op", cast)]` - fire-and-forget handlers
#[actor(facets = ["timer"])]
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

/// Handler implementations - SDK generates Actor impl with dispatch.
/// 
/// Using `#[handler("op", cast)]` for fire-and-forget semantics (no reply).
#[plexspaces_handlers(custom)]
impl SessionActor {
    /// Handle timer_fired events (from TimerFacet)
    #[handler("timer_fired", cast)]
    async fn handle_timer_fired(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let timer_name = String::from_utf8_lossy(&msg.payload).to_string();
        
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
        Ok(())
    }

    /// Handle activity events
    #[handler("activity", cast)]
    async fn handle_activity(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.activity_count += 1;
        println!("  Activity #{} from {} - would reset idle timer", 
            self.activity_count, self.user_id);
        Ok(())
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
    // Step 3: Create actor WITH TimerFacet attached (using SDK spawn_actor)
    // =========================================================================
    println!("Step 3: Create session actor with TimerFacet (SDK style)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Using SDK spawn_actor (like Python @actor(facets=["timer"]))
    let _actor_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        "session-user-123@session-node",
        "sessions",
        SessionActor::new("user-123"),
        vec![Box::new(timer_facet)],  // <-- Attach TimerFacet
    )
    .await
    .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    println!("  Actor: session-user-123@session-node");
    println!("  TimerFacet attached via spawn_actor(..., facets)");
    println!("  Using SDK annotations: #[actor], #[plexspaces_handlers], #[handler]");
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
