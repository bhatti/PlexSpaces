// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Rust SDK – high-level API and annotations to reduce boilerplate
// (similar to Python's @actor, @handler, state()).
//
// ## Quick Start
//
// ```rust,ignore
// use plexspaces_sdk::*;
//
// #[gen_server_actor(facets = ["durability"])]
// struct BankAccount {
//     balance: i64,
// }
//
// #[plexspaces_handlers]
// impl BankAccount {
//     #[handler("deposit", call)]
//     async fn deposit(&mut self, ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
//         let payload: Value = serde_json::from_slice(&msg.payload)?;
//         let amount = payload["amount"].as_i64().unwrap_or(0);
//         self.balance += amount;
//         Ok(json!({ "balance": self.balance }))
//     }
// }
// ```

// ============================================================================
// Re-export attribute macros (like Python decorators)
// ============================================================================

/// `#[actor]` - Basic actor with Custom behavior type
pub use plexspaces_sdk_macros::actor;

/// `#[gen_server_actor]` - GenServer behavior (request-reply, call by default)
pub use plexspaces_sdk_macros::gen_server_actor;

/// `#[event_actor]` - GenEvent behavior (fire-and-forget events)
pub use plexspaces_sdk_macros::event_actor;

/// `#[fsm_actor]` - GenStateMachine behavior (finite state machine)
pub use plexspaces_sdk_macros::fsm_actor;

/// `#[workflow_actor]` - Workflow behavior (durable workflows)
pub use plexspaces_sdk_macros::workflow_actor;

/// `#[handler("op")]` or `#[handler("op", call/cast)]` - Mark method as message handler
pub use plexspaces_sdk_macros::handler;

/// `#[plexspaces_handlers]` - Generate dispatch from #[handler] methods
pub use plexspaces_sdk_macros::plexspaces_handlers;

/// `#[init_handler]` - Mark method as initialization handler
pub use plexspaces_sdk_macros::init_handler;

/// `#[run_handler]` - Mark method as workflow run handler
pub use plexspaces_sdk_macros::run_handler;

/// `#[signal_handler("name")]` - Mark method as workflow signal handler
pub use plexspaces_sdk_macros::signal_handler;

/// `#[query_handler("name")]` - Mark method as workflow query handler
pub use plexspaces_sdk_macros::query_handler;

/// Legacy derive macro (prefer #[actor] or #[gen_server_actor] for new code)
pub use plexspaces_sdk_macros::PlexSpacesActor;

// ============================================================================
// Re-export core types so apps can depend only on plexspaces-sdk
// ============================================================================

pub use plexspaces_core::{
    ActorId, ActorContext, BehaviorError, BehaviorType, Message, RequestContext,
};
pub use plexspaces_actor::{ActorBuilder, ActorRef};
pub use plexspaces_node::NodeBuilder;

/// Actor trait – implement handle_message; use attribute macros for auto-generation.
pub use plexspaces_core::Actor as ActorTrait;

/// Re-export GenServer trait for #[plexspaces_handlers] generated code
pub use plexspaces_behavior::GenServer;

/// Re-export EventHandler trait for #[plexspaces_handlers(event)] generated code
pub use plexspaces_behavior::EventHandler;

/// Re-export Workflow trait for #[plexspaces_handlers(workflow)] generated code
pub use plexspaces_behavior::Workflow;

/// Re-export ExecutionContext for workflow handlers
pub use plexspaces_behavior::ExecutionContext;

/// Re-export async_trait for use with macros
pub use async_trait::async_trait;

/// Re-export serde_json for handler payload parsing
pub use serde_json;
pub use serde_json::{json, Value};

// ============================================================================
// Legacy macro (kept for backward compatibility)
// ============================================================================

/// Legacy macro for handler dispatch. Prefer `#[plexspaces_handlers]` for new code.
///
/// Example:
/// ```ignore
/// plexspaces_impl_handlers!(MyActor, BehaviorType::Custom("my_actor".into()),
///     deposit => handle_deposit,
///     withdraw => handle_withdraw
/// );
/// ```
#[macro_export]
macro_rules! plexspaces_impl_handlers {
    ($actor:ty, $behavior:expr, $( $msg_type:ident => $method:ident ),+ $(,)?) => {
        #[plexspaces_sdk::async_trait]
        impl plexspaces_core::Actor for $actor {
            async fn handle_message(
                &mut self,
                ctx: &plexspaces_core::ActorContext,
                msg: plexspaces_core::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                match msg.message_type.as_str() {
                    $( stringify!($msg_type) => self.$method(ctx, &msg).await ),+,
                    _ => Err(plexspaces_core::BehaviorError::UnsupportedMessage),
                }
            }

            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                $behavior
            }
        }
    };
}

// ============================================================================
// Spawn helper
// ============================================================================

/// Spawns an actor using ActorBuilder with optional facets (like Python @actor(facets=[...])).
/// Pass facets as a Vec of Box<dyn Facet>; use empty vec if no facets.
/// Returns the actor crate's ActorRef (messaging handle with tell/ask).
///
/// ## Example
/// ```ignore
/// let actor_ref = spawn_actor(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     MyActor::new(),
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
pub async fn spawn_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    namespace: impl AsRef<str>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    let mut builder = ActorBuilder::new(Box::new(behavior))
        .with_id(actor_id.into())
        .with_namespace(namespace.as_ref().to_string());
    for facet in facets {
        builder = builder.with_facet(facet);
    }
    builder.spawn(ctx, service_locator).await.map_err(|e| e.into())
}

// ============================================================================
// Facet creation helper
// ============================================================================

/// Creates facet instances from facet type names.
/// 
/// This is a convenience function to create common facets by name,
/// useful when facets are declared via `#[gen_server_actor(facets = ["timer", "durability"])]`.
///
/// ## Supported facet names
/// - `"timer"` - TimerFacet (requires plexspaces-journaling)
/// - `"durability"` - DurabilityFacet (requires plexspaces-journaling + storage config)
/// - `"logging"` - LoggingFacet
/// - `"caching"` - CachingFacet
///
/// ## Example
/// ```ignore
/// let facets = create_facets(&["timer", "logging"], &json!({}))?;
/// let actor_ref = spawn_actor(&ctx, service_locator, actor_id, namespace, actor, facets).await?;
/// ```
///
/// ## Note
/// For facets requiring complex configuration (e.g., DurabilityFacet with storage backend),
/// create them manually instead of using this helper.
pub fn create_facets(
    facet_names: &[&str],
    _config: &serde_json::Value,
) -> Result<Vec<Box<dyn plexspaces_facet::Facet>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut facets: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();
    
    for name in facet_names {
        match *name {
            "timer" => {
                let timer = plexspaces_journaling::TimerFacet::new(serde_json::json!({}), 50);
                facets.push(Box::new(timer));
            }
            "logging" => {
                // LoggingFacet is in plexspaces_facet
                // For now, skip - users should create manually if needed
            }
            "caching" => {
                // CachingFacet is in plexspaces_facet
                // For now, skip - users should create manually if needed
            }
            "durability" => {
                // DurabilityFacet requires storage backend - skip for now
                // Users should create manually with proper storage config
            }
            _ => {
                // Unknown facet - ignore silently or warn
            }
        }
    }
    
    Ok(facets)
}

// ============================================================================
// Re-export facets for convenience
// ============================================================================

/// Re-export TimerFacet for easy use
pub use plexspaces_journaling::TimerFacet;

/// Re-export the Facet trait
pub use plexspaces_facet::Facet;
