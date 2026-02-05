// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Session Manager App – real-world timers (idle timeout, heartbeat)
//
// Demonstrates **PlexSpaces Rust SDK annotations** for non-GenServer actors:
// - `#[actor(facets = ["timer"])]` - marks struct as actor with timer facet
// - `#[plexspaces_handlers(custom)]` - generates Actor impl for custom behavior
// - `#[handler("timer_fired", cast)]` - fire-and-forget handler
//
// Use case: User session with idle timeout (e.g. 5s) and periodic heartbeat (2s).

use plexspaces_sdk::{
    actor, plexspaces_handlers, handler,
    ActorContext, ActorId, BehaviorError, Message, NodeBuilder, RequestContext,
    spawn_actor, TimerFacet, json,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};
use tracing_subscriber;

// -----------------------------------------------------------------------------
// Session Actor – SDK annotations style (like Python @actor, @handler)
// -----------------------------------------------------------------------------

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
                info!("[session] idle_timeout fired – session expired");
                self.is_active = false;
            }
            "heartbeat" => {
                info!("[session] heartbeat fired – session alive");
            }
            _ => {
                info!("[session] timer fired: {}", timer_name);
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
        info!(
            "[session] activity #{} from {}",
            self.activity_count, self.user_id
        );
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("session_manager=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Session Manager App (SDK Annotations Demo)                    ║");
    println!("║  Using #[actor], #[plexspaces_handlers], #[handler]            ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    let node = Arc::new(
        NodeBuilder::new("session-node")
            .with_clustering_enabled(false)
            .build()
            .await,
    );
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "sessions".to_string());

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    sleep(Duration::from_millis(500)).await;
    info!("Node started");

    // Spawn with TimerFacet (like Python @actor(facets=["timer"]))
    let timer_facet = TimerFacet::new(json!({}), 50);
    let actor_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        ActorId::from("session-user-123@session-node"),
        "sessions",
        SessionActor::new("user-123"),
        vec![Box::new(timer_facet)],
    )
    .await
    .map_err(|e| format!("spawn: {}", e))?;
    let actor_id = actor_ref.id().clone();
    info!("Session actor spawned: {}", actor_id);

    // Register timers via node.get_facets + downcast (real-world: e.g. after login)
    let facets_arc = node
        .get_facets(&actor_id)
        .await
        .ok_or("get_facets")?;
    let facets_guard = facets_arc.read().await;
    let timer_facet_arc = facets_guard
        .get_facet("timer")
        .ok_or("timer facet not found")?;
    let timer_guard = timer_facet_arc.read().await;
    let timer_facet_ref = timer_guard
        .as_any()
        .downcast_ref::<TimerFacet>()
        .ok_or("downcast TimerFacet")?;
    timer_facet_ref
        .register_once("idle_timeout", Duration::from_secs(5))
        .await
        .map_err(|e| format!("register_once: {}", e))?;
    timer_facet_ref
        .register_periodic("heartbeat", Duration::from_secs(2))
        .await
        .map_err(|e| format!("register_periodic: {}", e))?;
    drop(timer_guard);
    drop(facets_guard);
    info!("Timers registered: idle_timeout=5s, heartbeat=2s");

    // Run long enough to see heartbeat (2s) and optionally idle_timeout (5s)
    println!("Running 7s to observe heartbeat and idle_timeout...");
    sleep(Duration::from_secs(7)).await;

    node.shutdown(Duration::from_secs(3)).await?;
    println!("Session Manager app complete.");
    Ok(())
}
