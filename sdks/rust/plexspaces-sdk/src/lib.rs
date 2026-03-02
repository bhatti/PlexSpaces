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


// ============================================================================
// DeclaredFacets trait - allows spawn_actor to auto-attach facets
// ============================================================================

/// Trait for actors that declare facets via annotations.
///
/// This trait is automatically implemented by the `#[gen_server_actor]`, `#[actor]`,
/// and other actor macros when `facets = [...]` is specified.
///
/// ## Example
/// ```ignore
/// #[gen_server_actor(facets = ["virtual_actor", "durability"])]
/// struct MyActor { ... }
///
/// // The macro generates:
/// impl DeclaredFacets for MyActor {
///     fn declared_facets() -> &'static [&'static str] {
///         &["virtual_actor", "durability"]
///     }
/// }
/// ```
///
/// ## Usage with spawn_actor
/// When spawning with empty facets vec, `spawn_actor` will automatically
/// use the actor's declared facets if it implements this trait.
pub trait DeclaredFacets {
    /// Returns the facets declared for this actor type.
    fn declared_facets() -> &'static [&'static str];
}

// ============================================================================
// Re-export core types so apps can depend only on plexspaces-sdk
// ============================================================================

pub use plexspaces_core::{
    ActorId, ActorContext, BehaviorError, BehaviorType, Message, RequestContext,
};
pub use plexspaces_actor::{ActorBuilder, ActorRef};
pub use plexspaces_node::NodeBuilder;

/// Actor trait – implement handle_message; use attribute macros for auto-generation.
/// 
/// For examples and user code, prefer SDK annotations (`#[gen_server_actor]`, `#[event_actor]`, etc.)
/// which automatically implement this trait.
pub use plexspaces_core::Actor;

// ============================================================================
// Message Creation Helpers
// ============================================================================

/// Create a new message with JSON payload and invocation type.
///
/// ## Invocation Types
/// - `"call"` - Request-reply (use with `ask()`)
/// - `"cast"` - Fire-and-forget (use with `tell()`)
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{new_message, json};
///
/// // For GenServer call (request-reply)
/// let msg = new_message("call", json!({
///     "action": "deposit",
///     "amount": 100,
/// }));
/// let reply = actor_ref.ask(msg, Duration::from_secs(5)).await?;
///
/// // For fire-and-forget
/// let msg = new_message("cast", json!({
///     "action": "log_event",
///     "event": "user_login",
/// }));
/// actor_ref.tell(msg).await?;
/// ```
pub fn new_message(invocation: &str, payload: serde_json::Value) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        message_type: invocation.to_string(),
        payload: serde_json::to_vec(&payload).unwrap_or_default(),
        ..Default::default()
    }
}

/// Create a call message (request-reply, use with `ask()`).
///
/// Shorthand for `new_message("call", payload)`.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{call_message, json};
///
/// let msg = call_message(json!({ "action": "get_balance" }));
/// let reply = actor_ref.ask(msg, Duration::from_secs(5)).await?;
/// ```
pub fn call_message(payload: serde_json::Value) -> Message {
    new_message("call", payload)
}

/// Create a cast message (fire-and-forget, use with `tell()`).
///
/// Shorthand for `new_message("cast", payload)`.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{cast_message, json};
///
/// let msg = cast_message(json!({ "event": "user_login" }));
/// actor_ref.tell(msg).await?;
/// ```
pub fn cast_message(payload: serde_json::Value) -> Message {
    new_message("cast", payload)
}

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

// ============================================================================
// Workflow API (Temporal/Restate-inspired)
// ============================================================================

/// Workflow-specific error type for typed error handling.
pub use plexspaces_workflow::WorkflowError;

/// High-level workflow handle with typed signal/query methods.
/// 
/// ## Design Philosophy (Temporal-inspired)
/// - **Workflow lifetime**: Workflows can run for days, months, or years
/// - **Operation timeouts**: Per-call timeouts with sensible defaults
/// - **No global timeout**: The handle itself has no timeout - only operations do
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{WorkflowRef, spawn_workflow};
///
/// // Spawn workflow
/// let workflow = spawn_workflow(
///     Box::new(MyWorkflow::new()),
///     "workflow-123",
///     "default",
///     service_locator.clone(),
/// ).await?;
///
/// // Run with default timeout (5 minutes)
/// let result: Output = workflow.run(&input).await?;
///
/// // Run with custom timeout for long-running workflows
/// let result: Output = workflow.run_with_timeout(
///     &input,
///     Duration::from_secs(3600), // 1 hour
/// ).await?;
///
/// // Signal (fire-and-forget)
/// workflow.signal("approve", &data).await?;
///
/// // Query with default timeout (30 seconds)
/// let status: Status = workflow.query("status").await?;
/// ```
pub use plexspaces_workflow::WorkflowRef;

/// Spawn a workflow actor and return a WorkflowRef.
/// 
/// ## Example
/// ```ignore
/// let workflow = spawn_workflow(
///     Box::new(ApprovalWorkflow::new()),
///     "approval-123",
///     "default",
///     service_locator.clone(),
/// ).await?;
/// ```
pub use plexspaces_workflow::spawn_workflow;

/// Default timeout for workflow operations (queries, signals) - 30 seconds.
pub use plexspaces_workflow::DEFAULT_OPERATION_TIMEOUT;

/// Default timeout for workflow run operations - 5 minutes.
pub use plexspaces_workflow::DEFAULT_RUN_TIMEOUT;

/// Spawn a workflow actor and return a WorkflowRef for interacting with it.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{spawn_workflow_actor, WorkflowRef};
///
/// let workflow: WorkflowRef = spawn_workflow_actor(
///     &ctx,
///     service_locator.clone(),
///     "approval-workflow-123",
///     ApprovalWorkflow::new(),
///     vec![Box::new(DurabilityFacet::new(...))],
/// ).await?;
///
/// let result: ApprovalResult = workflow.run(&request).await?;
/// workflow.signal("approve", &approval_data).await?;
/// ```
pub async fn spawn_workflow_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<WorkflowRef, WorkflowRefError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    // Get ActorFactory from ServiceLocator - core functionality in crates/actor
    let factory_dyn = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| WorkflowRefError::Spawn("ActorFactory not found in ServiceLocator".to_string()))?;
    
    // Downcast to ActorFactoryImpl for typed spawn methods
    let factory = factory_dyn
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| WorkflowRefError::Spawn("ActorFactory is not ActorFactoryImpl".to_string()))?;
    
    // Delegate to ActorFactoryImpl - core functionality stays in main crate
    factory.spawn_workflow(ctx, actor_id, behavior, facets).await
}

/// Re-export serde_json for handler payload parsing
pub use serde_json;
pub use serde_json::{json, Value};


// ============================================================================
// Spawn Helpers
// ============================================================================
// These helpers provide a convenient API for spawning actors while keeping
// core functionality in the main crates (ActorFactory, ActorBuilder).
// SDK helpers reduce boilerplate but delegate to core implementations.

/// Spawn an actor using its declared facets from annotations.
///
/// This is the **recommended** way to spawn actors. Facets declared via
/// `#[gen_server_actor(facets = [...])]` are automatically attached.
///
/// ## Example
/// ```ignore
/// #[gen_server_actor(facets = ["virtual_actor"])]
/// struct MyActor { ... }
///
/// // Facets are automatically created from the annotation!
/// let actor_ref = spawn(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     MyActor::new(),
/// ).await?;
/// ```
///
/// ## Note
/// For actors with `durability` facet, use `spawn_with_storage` instead
/// (requires storage backend for journaling).
pub async fn spawn<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    namespace: impl AsRef<str>,
    behavior: B,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
where
    B: plexspaces_core::Actor + DeclaredFacets + Send + 'static,
{
    let declared = B::declared_facets();
    let facets = create_facets(declared, &serde_json::json!({}), service_locator.clone())?;
    spawn_with_facets(ctx, service_locator, actor_id, namespace, behavior, facets).await
}

/// Spawn a durable actor using its declared facets with storage backend.
///
/// Use this for actors that declare `durability` facet - the storage backend
/// is required for journaling state changes.
///
/// ## Example
/// ```ignore
/// #[gen_server_actor(facets = ["virtual_actor", "durability"])]
/// struct MyDurableActor { ... }
///
/// let storage = Arc::new(SqliteJournalStorage::new(":memory:").await?);
///
/// // Facets are automatically created, durability uses provided storage
/// let actor_ref = spawn_with_storage(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     MyDurableActor::new(),
///     storage,
/// ).await?;
/// ```
pub async fn spawn_with_storage<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    namespace: impl AsRef<str>,
    behavior: B,
    storage: std::sync::Arc<dyn plexspaces_journaling::JournalStorage>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
where
    B: plexspaces_core::Actor + DeclaredFacets + Send + 'static,
{
    let declared = B::declared_facets();
    let facets = create_facets_with_storage(declared, &serde_json::json!({}), Some(storage), service_locator.clone())?;
    spawn_with_facets(ctx, service_locator, actor_id, namespace, behavior, facets).await
}

/// Spawn an actor with explicit facets (low-level API).
///
/// Use this when you need full control over facet creation, or for actors
/// that don't use the annotation macros.
///
/// ## Example
/// ```ignore
/// let actor_ref = spawn_with_facets(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     MyActor::new(),
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
/// Spawn an actor with explicit facets (low-level API).
///
/// Use this when you need full control over facet creation, or for actors
/// that don't use the annotation macros.
///
/// ## Architecture
/// Delegates to `ActorBuilder` which uses `ActorFactory` internally.
/// Core functionality remains in main crates; SDK provides convenience wrapper.
///
/// ## Example
/// ```ignore
/// let actor_ref = spawn_with_facets(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     MyActor::new(),
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
pub async fn spawn_with_facets<B>(
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
    let namespace_str = namespace.as_ref().to_string();
    
    // Phase 2.1: Register virtual actor type if virtual_actor facet is present
    // This enables automatic activation of any actor ID matching the type pattern
    // Uses centralized helper for consistent behavior across SDK, WASM, and app-config.toml
    let has_virtual_actor_facet = facets.iter().any(|f| f.facet_type() == "virtual_actor");
    if has_virtual_actor_facet {
        // Get actor_type from behavior.behavior_type()
        let behavior_type = behavior.behavior_type();
        let actor_type = match behavior_type {
            plexspaces_core::BehaviorType::GenServer => "GenServer".to_string(),
            plexspaces_core::BehaviorType::GenEvent => "GenEvent".to_string(),
            plexspaces_core::BehaviorType::GenStateMachine => "GenStateMachine".to_string(),
            plexspaces_core::BehaviorType::Workflow => "Workflow".to_string(),
            plexspaces_core::BehaviorType::Custom(s) => s,
        };
        
        // Use centralized helper for consistent registration
        // Errors are logged but non-fatal (actor will still work, just no auto-activation)
        let _ = plexspaces_core::register_virtual_actor_type_consistent(
            &service_locator,
            actor_type,
            namespace_str.clone(),
            Some(&facets),
            None, // No proto facets for SDK
            None, // config - can be provided later if needed
            Some(ctx.tenant_id().to_string()), // tenant_id from context
            None, // init_config_template - not used for SDK actors (only WASM actors)
        ).await;
    }
    
    // Use ActorBuilder from main crate - core functionality stays in crates/actor
    let mut builder = ActorBuilder::new(Box::new(behavior))
        .with_id(actor_id.into())
        .with_namespace(namespace_str);
    for facet in facets {
        builder = builder.with_facet(facet);
    }
    // ActorBuilder.spawn() delegates to ActorFactory internally
    builder.spawn(ctx, service_locator).await.map_err(|e| e.into())
}

/// Spawn an actor using a behavior type name from BehaviorRegistry.
///
/// Use this for actors registered via `BehaviorRegistry` (custom behaviors).
/// The behavior is created from `initial_state` bytes using the registered factory.
///
/// ## Architecture
/// Delegates to `ActorFactory` via `ServiceLocator`. Core functionality remains
/// in main crates; SDK provides convenience wrapper for BehaviorRegistry-based spawning.
///
/// ## Design Notes
/// - **Core Functionality**: `ActorFactory::spawn_actor()` handles BehaviorRegistry lookup and actor creation
/// - **SDK Role**: Provides convenient API that returns `ActorRef` directly (wraps `Arc<dyn MessageSender>`)
/// - **Local vs Remote**: Always creates `ActorRef::local()` since `ActorFactory::spawn_actor()` always spawns locally
/// - **Registration Timing**: Uses retry logic with exponential backoff to handle asynchronous actor instance registration
/// - **Mailbox Retrieval**: Gets mailbox from actor instance stored in ActorRegistry (required for `ActorRef::local()`)
///
/// ## Example
/// ```ignore
/// // Register behavior first
/// let registry = BehaviorRegistry::new();
/// registry.register("MyBehavior", |state: &[u8]| {
///     Box::pin(async move {
///         Ok(Box::new(MyActor::from_state(state)) as Box<dyn Actor>)
///     })
/// }).await;
/// service_locator.register_behavior_registry(Arc::new(registry)).await;
///
/// // Then spawn using behavior type name
/// let actor_ref = spawn_with_behavior_type(
///     &ctx,
///     service_locator,
///     "my-actor@node",
///     "default",
///     "MyBehavior",
///     serde_json::to_vec(&initial_state)?,
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
pub async fn spawn_with_behavior_type(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    namespace: impl AsRef<str>,
    behavior_type: impl AsRef<str>,
    initial_state: Vec<u8>,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::Arc;
    
    let actor_id: ActorId = actor_id.into();
    let behavior_type = behavior_type.as_ref().to_string();
    
    // Get ActorFactory from ServiceLocator - core functionality in crates/actor
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| "ActorFactory not available in ServiceLocator".to_string())?;
    
    // Use ActorFactory trait method - works with any implementation
    // ActorFactory handles BehaviorRegistry lookup and actor creation
    // Note: ActorFactory::spawn_actor() always spawns actors locally
    let _message_sender = actor_factory
        .spawn_actor(
            ctx,
            &actor_id,
            &behavior_type,
            initial_state,
            None,
            std::collections::HashMap::new(),
            facets,
        )
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Get ActorRegistry to lookup the spawned actor and get its mailbox
    let registry = service_locator
        .actor_registry()
        .await
        .ok_or_else(|| "ActorRegistry not available in ServiceLocator".to_string())?;
    
    // Extract tenant_id and namespace from RequestContext
    let tenant_id = ctx.tenant_id().to_string();
    let namespace_str = namespace.as_ref().to_string();
    
    // Get local node ID to determine if actor is local
    let local_node_id = registry.local_node_id();
    
    // Extract node_id from actor_id (format: "name@node-id")
    use plexspaces_core::ActorRef as CoreActorRef;
    let actor_id_str = actor_id.to_string();
    let (_, node_id_from_actor) = CoreActorRef::parse_actor_id(&actor_id_str)
        .unwrap_or_else(|_| {
            // Fallback: extract from actor_id string
            let parts: Vec<&str> = actor_id_str.split('@').collect();
            if parts.len() == 2 {
                (parts[0].to_string(), parts[1].to_string())
            } else {
                (actor_id_str.clone(), local_node_id.to_string())
            }
        });
    
    // Check if actor is local (ActorFactory always spawns locally, so this should be true)
    let is_local = node_id_from_actor == local_node_id || node_id_from_actor.is_empty();
    
    // Convert ServiceLocator to the type expected by ActorRef
    use plexspaces_core::ServiceLocator as ServiceLocatorTrait;
    let service_locator_trait: Arc<dyn ServiceLocatorTrait> = service_locator.clone();
    
    if is_local {
        // Actor is local - get mailbox from actor instance stored in registry
        // The actor instance is stored asynchronously during registration, so we retry
        // with exponential backoff to handle registration timing
        const MAX_RETRIES: usize = 10;
        const INITIAL_DELAY_MS: u64 = 10;
        
        let mut actor_instance = None;
        let mut delay_ms = INITIAL_DELAY_MS;
        
        for attempt in 0..MAX_RETRIES {
            if let Some(inst) = registry.get_actor_instance(&actor_id).await {
                actor_instance = Some(inst);
                if attempt > 0 {
                    tracing::debug!(
                        actor_id = %actor_id,
                        attempts = attempt + 1,
                        "Actor instance found after retry"
                    );
                }
                break;
            }
            
            if attempt < MAX_RETRIES - 1 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(100); // Exponential backoff, max 100ms
            }
        }
        
        let actor_instance = actor_instance
            .ok_or_else(|| {
                // Calculate total wait time: sum of exponential backoff delays
                let total_wait_ms: u64 = (0..MAX_RETRIES - 1)
                    .map(|i| (INITIAL_DELAY_MS * (1 << i)).min(100))
                    .sum();
                
                format!(
                    "Actor {} instance not found in registry after spawning (waited {}ms, {} attempts). \
                    This may indicate a registration failure or timing issue.",
                    actor_id,
                    total_wait_ms,
                    MAX_RETRIES
                )
            })?;
        
        // Downcast to Actor struct to get mailbox
        // The instance is stored as Arc<dyn Any>, we need to downcast to Arc<Actor>
        // Following the same pattern as actor_factory_impl.rs (lines 477, 760)
        use plexspaces_actor::Actor as ActorStruct;
        let actor_arc = actor_instance
            .downcast::<ActorStruct>()
            .map_err(|_| {
                format!(
                    "Actor {} instance is not an Actor struct. \
                    Expected Arc<Actor>, got different type. \
                    This indicates a registration or storage issue.",
                    actor_id
                )
            })?;
        
        let mailbox = actor_arc.mailbox().clone();
        
        Ok(ActorRef::local(actor_id, tenant_id, namespace_str, mailbox, service_locator_trait))
    } else {
        // Actor is remote - create ActorRef::remote()
        // Note: This branch should rarely execute since ActorFactory::spawn_actor() always spawns locally
        Ok(ActorRef::remote(actor_id, tenant_id, namespace_str, node_id_from_actor, service_locator_trait))
    }
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
    config: &serde_json::Value,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
) -> Result<Vec<Box<dyn plexspaces_facet::Facet>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut facets: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();
    
    for name in facet_names {
        match *name {
            "timer" => {
                let timer = plexspaces_journaling::TimerFacet::new(serde_json::json!({}), 50, service_locator.clone());
                facets.push(Box::new(timer));
            }
            "virtual_actor" => {
                // VirtualActorFacet with default config (uses constants from plexspaces-common)
                // TODO: Use runtime config from ServiceLocator when available (Phase 1.4)
                use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, DEFAULT_ACTIVATION_STRATEGY};
                use plexspaces_common::to_config_str;
                let facet_config = if config.get("virtual_actor").is_some() {
                    config["virtual_actor"].clone()
                } else {
                    serde_json::json!({
                        "idle_timeout": format!("{}s", DEFAULT_IDLE_TIMEOUT_SECONDS),
                        "activation_strategy": to_config_str(&DEFAULT_ACTIVATION_STRATEGY)
                    })
                };
                use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
                let virtual_facet = plexspaces_journaling::VirtualActorFacet::new(facet_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY);
                facets.push(Box::new(virtual_facet));
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
                // DurabilityFacet requires storage backend
                // Users must provide storage via config or create manually
                // Skip here - use create_facets_with_storage() instead
            }
            _ => {
                // Unknown facet - ignore silently or warn
            }
        }
    }
    
    Ok(facets)
}

/// Create facets from names with storage backend for durability
///
/// ## Purpose
/// Creates facets including DurabilityFacet with the provided storage backend.
/// Use this when you need durable actors with state persistence.
///
/// ## Supported Facets
/// - `"timer"` - TimerFacet (in-memory timers)
/// - `"virtual_actor"` - VirtualActorFacet (Orleans-style lifecycle)
/// - `"durability"` - DurabilityFacet (state persistence via journaling)
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{create_facets_with_storage, SqliteJournalStorage};
/// 
/// let storage = Arc::new(SqliteJournalStorage::new_in_memory().await?);
/// let facets = create_facets_with_storage(
///     &["virtual_actor", "durability"],
///     &json!({}),
///     Some(storage),
/// )?;
/// ```
pub fn create_facets_with_storage(
    facet_names: &[&str],
    config: &serde_json::Value,
    storage: Option<std::sync::Arc<dyn plexspaces_journaling::JournalStorage>>,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
) -> Result<Vec<Box<dyn plexspaces_facet::Facet>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut facets: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();
    
    for name in facet_names {
        match *name {
            "timer" => {
                let timer = plexspaces_journaling::TimerFacet::new(serde_json::json!({}), 50, service_locator.clone());
                facets.push(Box::new(timer));
            }
            "virtual_actor" => {
                // VirtualActorFacet with default config (uses constants from plexspaces-common)
                // TODO: Use runtime config from ServiceLocator when available (Phase 1.4)
                use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, DEFAULT_ACTIVATION_STRATEGY};
                use plexspaces_common::to_config_str;
                let facet_config = if config.get("virtual_actor").is_some() {
                    config["virtual_actor"].clone()
                } else {
                    serde_json::json!({
                        "idle_timeout": format!("{}s", DEFAULT_IDLE_TIMEOUT_SECONDS),
                        "activation_strategy": to_config_str(&DEFAULT_ACTIVATION_STRATEGY)
                    })
                };
                use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
                let virtual_facet = plexspaces_journaling::VirtualActorFacet::new(facet_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY);
                facets.push(Box::new(virtual_facet));
            }
            "durability" => {
                if let Some(ref storage) = storage {
                    let facet_config = if config.get("durability").is_some() {
                        config["durability"].clone()
                    } else {
                        serde_json::json!({})
                    };
                    let durability_facet = plexspaces_journaling::DurabilityFacet::new(
                        storage.clone(),
                        facet_config,
                        90, // priority
                    );
                    facets.push(Box::new(durability_facet));
                }
            }
            _ => {
                // Unknown facet - ignore silently
            }
        }
    }
    
    Ok(facets)
}

// ============================================================================
// Re-export facets for convenience
// ============================================================================

/// Re-export TimerFacet for in-memory timers (non-durable)
pub use plexspaces_journaling::TimerFacet;

/// Re-export VirtualActorFacet for Orleans-style virtual actor lifecycle
/// Virtual actors are always addressable but activated on-demand
pub use plexspaces_journaling::{VirtualActorFacet, VirtualActorLifecycleState, ActivationStrategy};

/// Re-export DurabilityFacet for durable execution (journaling)
pub use plexspaces_journaling::DurabilityFacet;

/// Re-export ReminderFacet for durable reminders (survive restarts)
pub use plexspaces_journaling::ReminderFacet;

/// Re-export SqliteJournalStorage for in-memory or file-based journaling
/// Note: Requires enabling sqlite-backend feature in plexspaces-journaling crate
#[cfg(all(feature = "grpc"))]
pub use plexspaces_journaling::SqliteJournalStorage;

/// Re-export JournalStorage trait for custom storage implementations
pub use plexspaces_journaling::JournalStorage;

/// Re-export the Facet trait
pub use plexspaces_facet::Facet;

// ============================================================================
// ShardGroup and Node Connectivity Helpers
// ============================================================================

pub mod shard_group;
pub mod node_client;
pub mod parallel;

pub use shard_group::{ShardGroupClientTrait, UnifiedShardGroupClient, ShardGroupClientLocal};
#[cfg(feature = "grpc")]
pub use shard_group::{ShardGroupClientGrpc, ShardGroupClient};
pub use node_client::{NodeClient, HealthCheckConfig};
pub use parallel::ParallelClient;

/// Re-export CoordinationComputeTracker for metrics tracking
pub use plexspaces_node::CoordinationComputeTracker;

// ============================================================================
// Re-export typed actor references from actor crate
// ============================================================================

/// Re-export WorkflowRefError for typed workflow error handling
pub use plexspaces_actor::WorkflowRefError;

/// Re-export GenServerRef for typed GenServer interactions
pub use plexspaces_actor::{GenServerRef, GenServerError, DEFAULT_CALL_TIMEOUT};

/// Re-export FsmRef for typed FSM interactions
pub use plexspaces_actor::{FsmRef, FsmError, DEFAULT_FSM_TIMEOUT};

/// Re-export EventRef for typed event emission
pub use plexspaces_actor::{EventRef, EventError};

/// Re-export ActorFactoryImpl for direct factory access
pub use plexspaces_actor::ActorFactoryImpl;

// ============================================================================
// Spawn helpers for typed actor references
// ============================================================================
// These helpers provide typed ActorRef wrappers (GenServerRef, FsmRef, etc.)
// while keeping core functionality in main crates. SDK provides convenience
// wrappers that delegate to ActorFactoryImpl for typed interactions.

/// Spawn an FSM actor and return an FsmRef for interacting with it.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{spawn_fsm_actor, FsmRef};
///
/// let fsm: FsmRef = spawn_fsm_actor(
///     &ctx,
///     service_locator.clone(),
///     "order-workflow-123",
///     OrderWorkflow::new(),
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
///
/// fsm.transition("submit", &order_data).await?;
/// let state: OrderState = fsm.query_state().await?;
/// ```
pub async fn spawn_fsm_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<FsmRef, FsmError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    // Get ActorFactory from ServiceLocator - core functionality in crates/actor
    let factory_dyn = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| FsmError::Spawn("ActorFactory not found in ServiceLocator".to_string()))?;
    
    // Downcast to ActorFactoryImpl for typed spawn methods
    // ActorFactoryImpl provides spawn_fsm, spawn_gen_server, etc.
    let factory = factory_dyn
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| FsmError::Spawn("ActorFactory is not ActorFactoryImpl".to_string()))?;
    
    // Delegate to ActorFactoryImpl - core functionality stays in main crate
    factory.spawn_fsm(ctx, actor_id, behavior, facets).await
}

/// Spawn a GenEvent actor and return an EventRef for interacting with it.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{spawn_event_actor, EventRef};
///
/// let logger: EventRef = spawn_event_actor(
///     &ctx,
///     service_locator.clone(),
///     "audit-logger",
///     AuditLogger::new(),
///     vec![],
/// ).await?;
///
/// logger.emit("user_login", &login_event).await?;
/// ```
pub async fn spawn_event_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<EventRef, EventError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    // Get ActorFactory from ServiceLocator - core functionality in crates/actor
    let factory_dyn = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| EventError::Spawn("ActorFactory not found in ServiceLocator".to_string()))?;
    
    // Downcast to ActorFactoryImpl for typed spawn methods
    let factory = factory_dyn
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| EventError::Spawn("ActorFactory is not ActorFactoryImpl".to_string()))?;
    
    // Delegate to ActorFactoryImpl - core functionality stays in main crate
    factory.spawn_event(ctx, actor_id, behavior, facets).await
}

/// Spawn a GenServer actor and return a GenServerRef for interacting with it.
///
/// ## Example
/// ```ignore
/// use plexspaces_sdk::{spawn_gen_server, GenServerRef};
///
/// let extractor: GenServerRef = spawn_gen_server(
///     &ctx,
///     service_locator.clone(),
///     "entity-extractor",
///     EntityExtractor::new(),
///     vec![],
/// ).await?;
///
/// let entities: Vec<Entity> = extractor.call("extract", &document).await?;
/// ```
pub async fn spawn_gen_server<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_id: impl Into<ActorId>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<GenServerRef, GenServerError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    // Get ActorFactory from ServiceLocator - core functionality in crates/actor
    let factory_dyn = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| GenServerError::Spawn("ActorFactory not found in ServiceLocator".to_string()))?;
    
    // Downcast to ActorFactoryImpl for typed spawn methods
    let factory = factory_dyn
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .ok_or_else(|| GenServerError::Spawn("ActorFactory is not ActorFactoryImpl".to_string()))?;
    
    // Delegate to ActorFactoryImpl - core functionality stays in main crate
    factory.spawn_gen_server(ctx, actor_id, behavior, facets).await
}
