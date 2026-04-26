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

pub mod simple_actor;

/// Ergonomic outbound HTTP client for actors using named service links.
///
/// ## Purpose
/// Zero-boilerplate wrapper around `OutboundHttpClient` for common HTTP patterns
/// (GET/POST/PUT/DELETE JSON) with automatic error handling.
///
/// ## Usage
/// ```rust,ignore
/// use plexspaces_sdk::http_client::ServiceHttpClient;
/// let http = ServiceHttpClient::from_context(ctx, "payments-api").await?;
/// let balance: serde_json::Value = http.get_json("/v1/balance").await?;
/// ```
pub mod http_client;

#[cfg(feature = "native")]
pub use http_client::{ServiceHttpClient, ServiceHttpClientError};

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

#[cfg(feature = "native")]
pub use plexspaces_actor::{ActorBuilder, ActorRef};
#[cfg(feature = "native")]
pub use plexspaces_core::{
    ActorContext, ActorId, BehaviorError, BehaviorType, Message, RequestContext,
};
#[cfg(feature = "native")]
pub use plexspaces_node::NodeBuilder;

/// Actor trait – implement handle_message; use attribute macros for auto-generation.
///
/// For examples and user code, prefer SDK annotations (`#[gen_server_actor]`, `#[event_actor]`, etc.)
/// which automatically implement this trait.
#[cfg(feature = "native")]
pub use plexspaces_core::Actor;

// ============================================================================
// Message Creation Helpers
// ============================================================================

/// Create a new message with JSON payload and message invocation.
///
/// ## Message Patterns
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
#[cfg(feature = "native")]
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
#[cfg(feature = "native")]
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
#[cfg(feature = "native")]
pub fn cast_message(payload: serde_json::Value) -> Message {
    new_message("cast", payload)
}

/// Re-export GenServer trait for #[plexspaces_handlers] generated code
#[cfg(feature = "native")]
pub use plexspaces_behavior::GenServer;

/// Re-export EventHandler trait for #[plexspaces_handlers(event)] generated code
#[cfg(feature = "native")]
pub use plexspaces_behavior::EventHandler;

/// Re-export Workflow trait for #[plexspaces_handlers(workflow)] generated code
#[cfg(feature = "native")]
pub use plexspaces_behavior::Workflow;

/// Re-export ExecutionContext for workflow handlers
#[cfg(feature = "native")]
pub use plexspaces_behavior::ExecutionContext;

/// Re-export async_trait for use with macros
#[cfg(feature = "native")]
pub use async_trait::async_trait;

// ============================================================================
// Workflow API (Temporal/Restate-inspired)
// ============================================================================

/// Workflow-specific error type for typed error handling.
#[cfg(feature = "native")]
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
#[cfg(feature = "native")]
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
#[cfg(feature = "native")]
pub use plexspaces_workflow::spawn_workflow;

/// Default timeout for workflow operations (queries, signals) - 30 seconds.
#[cfg(feature = "native")]
pub use plexspaces_workflow::DEFAULT_OPERATION_TIMEOUT;

/// Default timeout for workflow run operations - 5 minutes.
#[cfg(feature = "native")]
pub use plexspaces_workflow::DEFAULT_RUN_TIMEOUT;

#[cfg(feature = "native")]
fn actor_ref_from_sender(
    sender: std::sync::Arc<dyn plexspaces_core::MessageSender>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
    sender
        .as_any()
        .downcast_ref::<ActorRef>()
        .cloned()
        .ok_or_else(|| "ActorFactory returned a MessageSender that is not a local ActorRef".into())
}

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
#[cfg(feature = "native")]
pub async fn spawn_workflow_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<WorkflowRef, WorkflowRefError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    let actor_ref = spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        ctx.namespace(),
        behavior,
        facets,
    )
    .await
    .map_err(|e| WorkflowRefError::Spawn(e.to_string()))?;
    Ok(WorkflowRef::new(actor_ref))
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
/// let actor_ref = spawn(&ctx, service_locator, "my-actor", "default", MyActor::new()).await?;
/// ```
///
/// ## Note
/// For actors with `durability` facet, use `spawn_with_storage` instead
/// (requires storage backend for journaling).
#[cfg(feature = "native")]
pub async fn spawn<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    namespace: impl AsRef<str>,
    behavior: B,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
where
    B: plexspaces_core::Actor + DeclaredFacets + Send + 'static,
{
    let declared = B::declared_facets();
    let facets = create_facets(declared, &serde_json::json!({}), service_locator.clone())?;
    spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        namespace,
        behavior,
        facets,
    )
    .await
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
///     "my-actor",
///     "default",
///     MyDurableActor::new(),
///     storage,
/// ).await?;
/// ```
#[cfg(feature = "native")]
pub async fn spawn_with_storage<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    namespace: impl AsRef<str>,
    behavior: B,
    storage: std::sync::Arc<dyn plexspaces_journaling::JournalStorage>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>
where
    B: plexspaces_core::Actor + DeclaredFacets + Send + 'static,
{
    let declared = B::declared_facets();
    let facets = create_facets_with_storage(
        declared,
        &serde_json::json!({}),
        Some(storage),
        service_locator.clone(),
    )?;
    spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        namespace,
        behavior,
        facets,
    )
    .await
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
///     "my-actor",
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
///     "my-actor",
///     "default",
///     MyActor::new(),
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
#[cfg(feature = "native")]
pub async fn spawn_with_facets<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
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
        let behavior_type = behavior.behavior_type();
        let actor_type = actor_type_from_behavior(&behavior_type);

        let _ = plexspaces_core::register_virtual_actor_definition(
            &service_locator,
            plexspaces_proto::actor::v1::ActorSpawnSpec {
                identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: String::new(), // standalone virtual actors: name == type
                    actor_type: actor_type.clone(),
                }),
                role: String::new(),
                namespace: namespace_str.clone(),
                tenant_id: ctx.tenant_id().to_string(),
                behavior_kind: actor_type,
                args: std::collections::HashMap::new(),
                facets: plexspaces_core::proto_facets_for_registration(Some(&facets), None),
                labels: std::collections::HashMap::new(),
                config: None,
            },
        )
        .await;
    }

    // Use ActorBuilder from main crate - core functionality stays in crates/actor
    let mut builder = ActorBuilder::new(Box::new(behavior))
        .with_name(actor_name.into())
        .with_namespace(namespace_str);
    for facet in facets {
        builder = builder.with_facet(facet);
    }
    // ActorBuilder.spawn() delegates to ActorFactory internally
    builder
        .spawn(ctx, service_locator)
        .await
        .map_err(|e| e.into())
}

/// Spawn an actor using a behavior type name from BehaviorRegistry.
///
/// Use this for actors registered via `BehaviorRegistry` (custom behaviors).
/// The behavior is created from `initial_state` bytes using the registered factory.
///
/// ## Architecture
/// Delegates to `ActorFactory` via `ServiceLocator`. Core functionality remains
/// in main crates; SDK provides a typed wrapper around the framework-owned spawn result.
///
/// ## Design Notes
/// - **Core Functionality**: the framework-owned spawn path handles BehaviorRegistry lookup and actor creation
/// - **SDK Role**: Converts the spawned `MessageSender` into the framework's canonical `ActorRef`
/// - **Local Spawn Contract**: BehaviorRegistry-based spawns are local, so the returned sender must already be a local `ActorRef`
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
///     "my-actor//my_behavior::default@node-1",
///     "default",
///     "MyBehavior",
///     serde_json::to_vec(&initial_state)?,
///     vec![Box::new(TimerFacet::new(json!({}), 50))],
/// ).await?;
/// ```
#[cfg(feature = "native")]
pub async fn spawn_with_behavior_type(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    _namespace: impl AsRef<str>,
    behavior_type: impl AsRef<str>,
    initial_state: Vec<u8>,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
    let actor_name = actor_name.into();
    let behavior_type = behavior_type.as_ref().to_string();

    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .ok_or_else(|| "ActorFactory not available in ServiceLocator".to_string())?;

    let local_node_id = service_locator
        .actor_registry()
        .await
        .ok_or_else(|| "ActorRegistry not available in ServiceLocator".to_string())?
        .local_node_id()
        .to_string();
    let actor_id = ActorId::new(
        actor_name,
        behavior_type.clone(),
        ctx.namespace().to_string(),
        local_node_id,
    )
    .map_err(|e| format!("Failed to construct actor ID: {}", e))?;

    let spawn_spec = {
        use plexspaces_core::ActorSpawnSpec;
        use plexspaces_proto::common::v1::ActorIdentity;
        ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: actor_id.name().to_string(),
                actor_type: behavior_type.clone(),
            }),
            role: String::new(),
            namespace: ctx.namespace().to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            behavior_kind: String::new(),
            args: std::collections::HashMap::new(),
            facets: vec![],
            config: None,
            labels: std::collections::HashMap::new(),
        }
    };
    let message_sender = actor_factory
        .spawn_actor(ctx, &spawn_spec, facets)
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;

    let actor_ref = actor_ref_from_sender(message_sender)?;
    debug_assert_eq!(actor_ref.id(), &actor_id);
    Ok(actor_ref)
}

#[cfg(feature = "native")]
fn actor_type_from_behavior(behavior_type: &plexspaces_core::BehaviorType) -> String {
    match behavior_type {
        plexspaces_core::BehaviorType::Custom(s) => s.clone(),
        bt => bt.actor_type_slug().into_owned(),
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
/// let actor_ref = spawn_actor(&ctx, service_locator, "counter", namespace, actor, facets).await?;
/// ```
///
/// ## Note
/// For facets requiring complex configuration (e.g., DurabilityFacet with storage backend),
/// create them manually instead of using this helper.
#[cfg(feature = "native")]
pub fn create_facets(
    facet_names: &[&str],
    config: &serde_json::Value,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
) -> Result<Vec<Box<dyn plexspaces_facet::Facet>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut facets: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();

    for name in facet_names {
        match *name {
            "timer" => {
                let timer_config =
                    plexspaces_core::facet_helpers::facet_config_value(config, "timer");
                let timer = plexspaces_journaling::TimerFacet::new(
                    timer_config,
                    50,
                    service_locator.clone(),
                );
                facets.push(Box::new(timer));
            }
            "virtual_actor" => {
                let facet_config =
                    plexspaces_core::facet_helpers::default_virtual_actor_facet_config(Some(
                        config,
                    ));
                use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
                let virtual_facet = plexspaces_journaling::VirtualActorFacet::new(
                    facet_config,
                    VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY,
                );
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
#[cfg(feature = "native")]
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
                let timer_config =
                    plexspaces_core::facet_helpers::facet_config_value(config, "timer");
                let timer = plexspaces_journaling::TimerFacet::new(
                    timer_config,
                    50,
                    service_locator.clone(),
                );
                facets.push(Box::new(timer));
            }
            "virtual_actor" => {
                let facet_config =
                    plexspaces_core::facet_helpers::default_virtual_actor_facet_config(Some(
                        config,
                    ));
                use plexspaces_journaling::VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY;
                let virtual_facet = plexspaces_journaling::VirtualActorFacet::new(
                    facet_config,
                    VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY,
                );
                facets.push(Box::new(virtual_facet));
            }
            "durability" => {
                if let Some(ref storage) = storage {
                    let facet_config =
                        plexspaces_core::facet_helpers::facet_config_value(config, "durability");
                    let durability_facet = plexspaces_journaling::DurabilityFacet::new(
                        storage.clone(),
                        facet_config,
                        90, // priority
                    );
                    facets.push(Box::new(durability_facet));
                }
            }
            "reminder" => {
                if let Some(ref storage) = storage {
                    let facet_config =
                        plexspaces_core::facet_helpers::facet_config_value(config, "reminder");
                    let reminder_facet = plexspaces_journaling::ReminderFacet::new(
                        storage.clone(),
                        facet_config,
                        plexspaces_journaling::REMINDER_FACET_DEFAULT_PRIORITY,
                        service_locator.clone(),
                    );
                    facets.push(Box::new(reminder_facet));
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
#[cfg(feature = "native")]
pub use plexspaces_journaling::TimerFacet;

/// Re-export VirtualActorFacet for Orleans-style virtual actor lifecycle
/// Virtual actors are always addressable but activated on-demand
#[cfg(feature = "native")]
pub use plexspaces_journaling::{
    ActivationStrategy, VirtualActorFacet, VirtualActorLifecycleState,
};

/// Re-export DurabilityFacet for durable execution (journaling)
#[cfg(feature = "native")]
pub use plexspaces_journaling::DurabilityFacet;

/// Re-export ReminderFacet for durable reminders (survive restarts)
#[cfg(feature = "native")]
pub use plexspaces_journaling::ReminderFacet;

/// Re-export SqliteJournalStorage for in-memory or file-based journaling
/// Note: Requires enabling sqlite-backend feature in plexspaces-journaling crate
#[cfg(all(feature = "grpc"))]
pub use plexspaces_journaling::SqliteJournalStorage;

/// Re-export JournalStorage trait for custom storage implementations
#[cfg(feature = "native")]
pub use plexspaces_journaling::JournalStorage;

/// Re-export the Facet trait
#[cfg(feature = "native")]
pub use plexspaces_facet::Facet;

// ============================================================================
// ShardGroup and Node Connectivity Helpers
// ============================================================================

#[cfg(feature = "native")]
pub mod elastic_pool_client;
#[cfg(feature = "grpc")]
pub mod leader_worker;
#[cfg(feature = "grpc")]
pub mod node_client;
#[cfg(feature = "native")]
pub mod shard_group;

#[cfg(feature = "native")]
pub use elastic_pool_client::ElasticPoolClient;
#[cfg(feature = "grpc")]
pub use node_client::{HealthCheckConfig, NodeClient};
/// Re-export pool service error for elastic pool client callers
#[cfg(feature = "native")]
pub use plexspaces_core::PoolServiceError;
#[cfg(feature = "grpc")]
#[cfg(all(feature = "native", feature = "grpc"))]
pub use shard_group::{ShardGroupClient, ShardGroupClientGrpc};
#[cfg(feature = "native")]
pub use shard_group::{ShardGroupClientLocal, ShardGroupClientTrait, UnifiedShardGroupClient};

/// Re-export CoordinationComputeTracker for metrics tracking
#[cfg(feature = "native")]
pub use plexspaces_node::CoordinationComputeTracker;

// ============================================================================
// Re-export typed actor references from actor crate
// ============================================================================

/// Re-export WorkflowRefError for typed workflow error handling
#[cfg(feature = "native")]
pub use plexspaces_actor::WorkflowRefError;

/// Re-export GenServerRef for typed GenServer interactions
#[cfg(feature = "native")]
pub use plexspaces_actor::{GenServerError, GenServerRef, DEFAULT_CALL_TIMEOUT};

/// Re-export FsmRef for typed FSM interactions
#[cfg(feature = "native")]
pub use plexspaces_actor::{FsmError, FsmRef, DEFAULT_FSM_TIMEOUT};

/// Re-export EventRef for typed event emission
#[cfg(feature = "native")]
pub use plexspaces_actor::{EventError, EventRef};

// ============================================================================
// Spawn helpers for typed actor references
// ============================================================================
// These helpers provide typed ActorRef wrappers (GenServerRef, FsmRef, etc.)
// while keeping core functionality in main crates. SDK provides convenience
// wrappers around the canonical spawn helpers and typed ref constructors.

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
#[cfg(feature = "native")]
pub async fn spawn_fsm_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<FsmRef, FsmError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    let actor_ref = spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        ctx.namespace(),
        behavior,
        facets,
    )
    .await
    .map_err(|e| FsmError::Spawn(e.to_string()))?;
    Ok(FsmRef::new(actor_ref))
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
#[cfg(feature = "native")]
pub async fn spawn_event_actor<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<EventRef, EventError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    let actor_ref = spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        ctx.namespace(),
        behavior,
        facets,
    )
    .await
    .map_err(|e| EventError::Spawn(e.to_string()))?;
    Ok(EventRef::new(actor_ref))
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
#[cfg(feature = "native")]
pub async fn spawn_gen_server<B>(
    ctx: &RequestContext,
    service_locator: std::sync::Arc<dyn plexspaces_core::ServiceLocator>,
    actor_name: impl Into<String>,
    behavior: B,
    facets: Vec<Box<dyn plexspaces_facet::Facet>>,
) -> Result<GenServerRef, GenServerError>
where
    B: plexspaces_core::Actor + Send + 'static,
{
    let actor_ref = spawn_with_facets(
        ctx,
        service_locator,
        actor_name,
        ctx.namespace(),
        behavior,
        facets,
    )
    .await
    .map_err(|e| GenServerError::Spawn(e.to_string()))?;
    Ok(GenServerRef::new(actor_ref))
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::{actor_ref_from_sender, create_facets, create_facets_with_storage};
    use async_trait::async_trait;
    use plexspaces_actor::{ActorRef, TestServiceLocatorStub};
    use plexspaces_core::{Message, MessageSender, ServiceLocator};
    use plexspaces_journaling::SqliteJournalStorage;
    use std::sync::Arc;

    struct NonActorRefSender;

    #[async_trait]
    impl MessageSender for NonActorRefSender {
        async fn tell(
            &self,
            _message: Message,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn actor_ref_from_sender_returns_actor_ref() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let actor_ref = ActorRef::remote(
            "sdk-test@remote-node",
            "",
            "test",
            "remote-node",
            service_locator,
        );
        let sender: Arc<dyn MessageSender> = Arc::new(actor_ref.clone());

        let resolved = actor_ref_from_sender(sender).expect("expected actor ref");

        assert_eq!(resolved.id(), actor_ref.id());
    }

    #[tokio::test]
    async fn actor_ref_from_sender_rejects_non_actor_ref_sender() {
        let sender: Arc<dyn MessageSender> = Arc::new(NonActorRefSender);

        let error = actor_ref_from_sender(sender).expect_err("expected conversion to fail");

        assert!(
            error.to_string().contains("not a local ActorRef"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn create_facets_uses_shared_virtual_actor_defaults() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let facets = create_facets(&["virtual_actor"], &serde_json::json!({}), service_locator)
            .expect("expected virtual actor facet");

        assert_eq!(facets.len(), 1);
        let config = facets[0].get_config();
        assert_eq!(
            config["activation_strategy"].as_str(),
            Some("lazy"),
            "shared default activation strategy should be used"
        );
        assert!(
            config.get("idle_timeout").is_some(),
            "shared default idle timeout should be present"
        );
    }

    #[tokio::test]
    async fn create_facets_with_storage_builds_reminder_and_durability_facets() {
        let service_locator: Arc<dyn ServiceLocator> = Arc::new(TestServiceLocatorStub::new());
        let storage = Arc::new(
            SqliteJournalStorage::new(":memory:")
                .await
                .expect("sqlite journal storage"),
        );

        let facets = create_facets_with_storage(
            &["reminder", "durability", "virtual_actor"],
            &serde_json::json!({
                "reminder": { "tick_interval_ms": 2500 },
                "durability": { "checkpoint_interval": 5 }
            }),
            Some(storage),
            service_locator,
        )
        .expect("expected reminder and durability facets");

        assert_eq!(facets.len(), 3);
        let facet_types: Vec<&str> = facets.iter().map(|facet| facet.facet_type()).collect();
        assert!(facet_types.contains(&"reminder"));
        assert!(facet_types.contains(&"durability"));
        assert!(facet_types.contains(&"virtual_actor"));
    }
}
