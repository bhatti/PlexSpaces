// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Procedural macros for PlexSpaces Rust SDK – reduce boilerplate like Python's @actor / @handler.
//
// ## Annotations (mirroring Python SDK)
//
// | Rust                                  | Python                          | Generated                           |
// |---------------------------------------|--------------------------------|-------------------------------------|
// | `#[actor]`                            | `@actor`                       | impl Actor (Custom behavior)        |
// | `#[actor(facets = ["durability"])]`   | `@actor(facets=[...])`         | + FACETS const                      |
// | `#[gen_server_actor]`                 | `@gen_server_actor`            | impl Actor + impl GenServer         |
// | `#[event_actor]`                      | `@event_actor`                 | impl Actor (GenEvent)               |
// | `#[fsm_actor]`                        | `@fsm_actor`                   | impl Actor (GenStateMachine)        |
// | `#[workflow_actor]`                   | `@workflow_actor`              | impl Actor (Workflow)               |
// | `#[handler("op")]`                    | `@handler("op")`               | Route msg to method (GenServer=call)|
// | `#[handler("op", call)]`              | `@handler("op", "call")`       | Explicit call (request-reply)       |
// | `#[handler("op", cast)]`              | `@handler("op", "cast")`       | Explicit cast (fire-and-forget)     |
// | `#[init_handler]`                     | `@init_handler`                | Called on actor initialization      |
//
// NOTE: For `#[gen_server_actor]`, handlers default to "call" - second param not needed.
// | `#[plexspaces_handlers]`              | (automatic)                    | Generates dispatch from handlers    |
//
// ## Behavior Types
// - GenServer: Request-reply pattern (call by default)
// - GenEvent: Fire-and-forget events (cast by default)
// - GenStateMachine: Finite state machine with state transitions
// - Workflow: Durable workflow orchestration (run/signal/query)
// - Custom: User-defined behavior type
//
// ## Facet Types (can be attached to any actor)
// - timer: In-memory timers (lost on deactivation)
// - reminder: Durable reminders (persisted, survives restarts)
// - durability: Restate-inspired durable execution with journaling
// - event_sourcing: Event sourcing with complete event history
// - virtual_actor: Automatic activation/deactivation
// - key_value: Key-value storage capability
// - http_client: HTTP client capability
// - lock: Distributed locking
// - registry: Actor registry/discovery
// - process_group: Process group coordination
// - logging: Structured logging
// - caching: In-memory caching
// - metrics: Metrics collection
// - event_emitter: Pub/sub event emission

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{parse_macro_input, Attribute, ImplItem, ItemImpl, ItemStruct};

// ============================================================================
// Helper: Parse facets from attribute like `facets = ["durability", "timer"]`
// ============================================================================

fn parse_facets(attr: TokenStream) -> Vec<String> {
    let mut facets = Vec::new();
    if attr.is_empty() {
        return facets;
    }

    // Parse as Meta
    let attr_str = attr.to_string();
    if attr_str.contains("facets") {
        // Extract facets = [...]
        if let Some(start) = attr_str.find('[') {
            if let Some(end) = attr_str.find(']') {
                let facets_str = &attr_str[start + 1..end];
                for facet in facets_str.split(',') {
                    let facet = facet.trim().trim_matches('"').trim_matches('\'');
                    if !facet.is_empty() {
                        facets.push(facet.to_string());
                    }
                }
            }
        }
    }
    facets
}

fn parse_name_attr(attr: TokenStream) -> Option<String> {
    let attr_str = attr.to_string();
    if attr_str.contains("name") {
        // Extract name = "..."
        if let Some(eq_pos) = attr_str.find("name") {
            let rest = &attr_str[eq_pos + 4..];
            if let Some(start) = rest.find('"') {
                if let Some(end) = rest[start + 1..].find('"') {
                    return Some(rest[start + 1..start + 1 + end].to_string());
                }
            }
        }
    }
    None
}

fn attr_contains_mode(attr: &TokenStream, needle: &str) -> bool {
    let attr_str = attr.to_string();
    attr_str.contains(needle)
}

// ============================================================================
// Helper: Generate facets const
// ============================================================================

fn gen_facets_const(name: &syn::Ident, facets: &[String]) -> TokenStream2 {
    if facets.is_empty() {
        quote! {
            impl #name {
                /// Facets declared for this actor (empty if none)
                pub const FACETS: &'static [&'static str] = &[];
            }

            impl plexspaces_sdk::DeclaredFacets for #name {
                fn declared_facets() -> &'static [&'static str] {
                    &[]
                }
            }
        }
    } else {
        let facet_strs: Vec<_> = facets.iter().map(|f| quote! { #f }).collect();
        let facet_strs2: Vec<_> = facets.iter().map(|f| quote! { #f }).collect();
        quote! {
            impl #name {
                /// Facets declared for this actor
                pub const FACETS: &'static [&'static str] = &[#(#facet_strs),*];
            }

            impl plexspaces_sdk::DeclaredFacets for #name {
                fn declared_facets() -> &'static [&'static str] {
                    &[#(#facet_strs2),*]
                }
            }
        }
    }
}

// ============================================================================
// #[actor] - Basic actor with Custom behavior type
// ============================================================================

/// Marks a struct as a PlexSpaces actor with Custom behavior type.
///
/// ## Usage
/// ```ignore
/// #[actor]
/// struct MyActor { ... }
///
/// #[actor(facets = ["durability", "timer"])]
/// struct DurableActor { ... }
///
/// #[actor(name = "custom_name")]
/// struct NamedActor { ... }
/// ```
///
/// ## Generated
/// - `const FACETS` with declared facets
/// - Note: Use `#[plexspaces_handlers(custom)]` on impl block to generate `impl Actor`
///
/// ## Facets
/// Facets extend actor capabilities without changing behavior type:
/// - `timer`: In-memory timers (lost on deactivation)
/// - `reminder`: Durable reminders (persisted, survives restarts)
/// - `durability`: Restate-inspired durable execution
/// - `event_sourcing`: Event sourcing with complete history
/// - `virtual_actor`: Automatic activation/deactivation
/// - `key_value`: Key-value storage capability
/// - `http_client`: HTTP client capability
/// - `lock`: Distributed locking
/// - `registry`: Actor registry/discovery
/// - `process_group`: Process group coordination
#[proc_macro_attribute]
pub fn actor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let facets = parse_facets(attr.clone());
    let _custom_name = parse_name_attr(attr).unwrap_or_else(|| name.to_string());

    let facets_impl = gen_facets_const(name, &facets);

    let expanded = quote! {
        #input

        #facets_impl
    };

    TokenStream::from(expanded)
}

// ============================================================================
// #[gen_server_actor] - GenServer behavior (request-reply, call by default)
// ============================================================================

/// Marks a struct as a PlexSpaces GenServer actor.
///
/// GenServer is the most common behavior pattern - request/reply (like Erlang gen_server).
/// Handlers default to "call" semantics (synchronous, expects reply).
///
/// ## Usage
/// ```ignore
/// #[gen_server_actor]
/// struct WebhookHandler { ... }
///
/// #[gen_server_actor(facets = ["durability"])]
/// struct DurableHandler { ... }
///
/// #[gen_server_actor(name = "webhook_handler")]
/// struct WebhookHandlerActor { ... }  // Registers as "webhook_handler" type
/// ```
///
/// ## Generated
/// - `impl Actor` with `behavior_type() = GenServer` (or Custom(name) if name specified)
/// - `handle_message` -> `route_message` (delegates to GenServer trait)
/// - `const FACETS` with declared facets
/// - Use `#[plexspaces_handlers]` on impl block to generate `impl GenServer` dispatch
///
/// ## Automatic Virtual Actor Type Registration
/// When `facets = ["virtual_actor"]` is included, calling `spawn_with_facets()` or `spawn()`
/// automatically calls `register_virtual_actor_type_consistent()` so the actor type is registered
/// for resurrection. The type registration persists across actor vacation (deactivation) and is
/// only removed when explicitly unregistered. Subsequent spawns of the same actor ID will
/// reuse the registered type metadata including all facet configs (timer, reminder, etc.).
///
/// ## Handler Semantics
/// GenServer handlers default to "call" (request-reply):
/// - Handler receives message, processes it, returns Result<Value, BehaviorError>
/// - Reply is automatically sent back to caller
/// - Use `#[handler("op", cast)]` for fire-and-forget operations
#[proc_macro_attribute]
pub fn gen_server_actor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let facets = parse_facets(attr.clone());
    let custom_name = parse_name_attr(attr.clone());
    let is_wasm = attr_contains_mode(&attr, "wasm");

    let facets_impl = gen_facets_const(name, &facets);

    if is_wasm {
        let expanded = quote! {
            #input

            #facets_impl
        };

        return TokenStream::from(expanded);
    }

    // If custom name is provided, use Custom(name) behavior type for HTTP gateway routing
    let behavior_type_expr = if let Some(ref custom) = custom_name {
        quote! { plexspaces_core::BehaviorType::Custom(#custom.to_string()) }
    } else {
        quote! { plexspaces_core::BehaviorType::GenServer }
    };

    let expanded = quote! {
        #input

        #facets_impl

        #[plexspaces_sdk::async_trait]
        impl plexspaces_core::Actor for #name {
            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                #behavior_type_expr
            }

            async fn handle_message(
                &mut self,
                ctx: &plexspaces_core::ActorContext,
                msg: plexspaces_core::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                <Self as plexspaces_behavior::GenServer>::route_message(self, ctx, msg).await
            }
        }
    };

    TokenStream::from(expanded)
}

// ============================================================================
// #[event_actor] - GenEvent behavior (fire-and-forget, cast by default)
// ============================================================================

/// Marks a struct as a PlexSpaces GenEvent actor (fire-and-forget events).
///
/// GenEvent is for event-driven actors that don't need to reply.
/// Handlers default to "cast" semantics (asynchronous, no reply expected).
///
/// ## Usage
/// ```ignore
/// #[event_actor]
/// struct AuditLogger { ... }
///
/// #[event_actor(facets = ["durability"])]
/// struct DurableLogger { ... }
///
/// #[event_actor(name = "audit_logger")]
/// struct AuditLoggerActor { ... }
/// ```
///
/// ## Generated
/// - `impl Actor` with `behavior_type() = GenEvent`
/// - `handle_message` dispatches to handlers (no reply sent)
/// - `const FACETS` with declared facets
/// - Use `#[plexspaces_handlers(event)]` on impl block to generate dispatch
///
/// ## Handler Semantics
/// GenEvent handlers default to "cast" (fire-and-forget):
/// - Handler receives message, processes it, returns Result<(), BehaviorError>
/// - No reply is sent back to caller
/// - Use for logging, auditing, notifications, etc.
#[proc_macro_attribute]
pub fn event_actor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let facets = parse_facets(attr.clone());
    let custom_name = parse_name_attr(attr);

    let facets_impl = gen_facets_const(name, &facets);

    // If custom name is provided, use Custom(name) behavior type
    let behavior_type_expr = if let Some(ref custom) = custom_name {
        quote! { plexspaces_core::BehaviorType::Custom(#custom.to_string()) }
    } else {
        quote! { plexspaces_core::BehaviorType::GenEvent }
    };

    let expanded = quote! {
        #input

        #facets_impl

        #[plexspaces_sdk::async_trait]
        impl plexspaces_core::Actor for #name {
            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                #behavior_type_expr
            }

            async fn handle_message(
                &mut self,
                ctx: &plexspaces_core::ActorContext,
                msg: plexspaces_core::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                // GenEvent: dispatch to handlers, no reply expected
                // Use #[plexspaces_handlers(event)] to generate dispatch
                <Self as plexspaces_behavior::EventHandler>::handle_event(self, ctx, msg).await
            }
        }
    };

    TokenStream::from(expanded)
}

// ============================================================================
// #[fsm_actor] - GenStateMachine behavior (finite state machine)
// ============================================================================

/// Marks a struct as a PlexSpaces FSM actor (finite state machine).
///
/// FSM actors manage state transitions based on events.
/// Useful for order workflows, payment processing, approval flows, etc.
///
/// ## Usage
/// ```ignore
/// #[fsm_actor]
/// struct OrderWorkflow {
///     state: OrderState,
/// }
///
/// #[fsm_actor(facets = ["durability"])]
/// struct DurableWorkflow { ... }
///
/// #[fsm_actor(name = "order_workflow")]
/// struct OrderWorkflowActor { ... }
/// ```
///
/// ## Generated
/// - `impl Actor` with `behavior_type() = GenStateMachine`
/// - `handle_message` dispatches to state handlers
/// - `const FACETS` with declared facets
/// - Use `#[plexspaces_handlers(fsm)]` on impl block to generate dispatch
///
/// ## Handler Semantics
/// FSM handlers receive events and can trigger state transitions:
/// - `#[handler("event_name")]` - handle specific event
/// - `#[state("state_name")]` - only handle in specific state
/// - Return `Ok(Some(new_state))` to transition, `Ok(None)` to stay
#[proc_macro_attribute]
pub fn fsm_actor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let facets = parse_facets(attr.clone());
    let custom_name = parse_name_attr(attr);

    let facets_impl = gen_facets_const(name, &facets);

    // If custom name is provided, use Custom(name) behavior type
    let behavior_type_expr = if let Some(ref custom) = custom_name {
        quote! { plexspaces_core::BehaviorType::Custom(#custom.to_string()) }
    } else {
        quote! { plexspaces_core::BehaviorType::GenStateMachine }
    };

    let expanded = quote! {
        #input

        #facets_impl

        #[plexspaces_sdk::async_trait]
        impl plexspaces_core::Actor for #name {
            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                #behavior_type_expr
            }

            async fn handle_message(
                &mut self,
                ctx: &plexspaces_core::ActorContext,
                msg: plexspaces_core::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                // FSM: dispatch to state handlers based on current state and event
                // Use #[plexspaces_handlers(fsm)] to generate dispatch
                self.handle_fsm_message(ctx, msg).await
            }
        }
    };

    TokenStream::from(expanded)
}

// ============================================================================
// #[workflow_actor] - Workflow behavior (durable workflows)
// ============================================================================

/// Marks a struct as a PlexSpaces Workflow actor (durable workflows).
///
/// Workflow actors implement the Restate-inspired run/signal/query pattern
/// for durable, long-running workflows.
///
/// ## Usage
/// ```ignore
/// #[workflow_actor]
/// struct PaymentPipeline { ... }
///
/// #[workflow_actor(facets = ["durability"])]
/// struct DurablePipeline { ... }
///
/// #[workflow_actor(name = "payment_pipeline")]
/// struct PaymentPipelineActor { ... }
/// ```
///
/// ## Generated
/// - `impl Actor` with `behavior_type() = Workflow`
/// - `handle_message` -> `route_workflow_message`
/// - `const FACETS` with declared facets
/// - Use `#[plexspaces_handlers(workflow)]` on impl block to generate dispatch
///
/// ## Handler Types
/// Workflow actors have three handler types:
/// - `#[run_handler]` - Main workflow execution (exclusive, one at a time)
/// - `#[signal_handler("name")]` - External events that modify state
/// - `#[query_handler("name")]` - Read-only queries (can be concurrent)
///
/// ## ExecutionContext
/// Workflow handlers receive an ExecutionContext with durable operations:
/// - `ctx.run(name, retry, || ...)` - Execute side-effect durably (retry = None or RetryConfig)
/// - `ctx.sleep(duration)` - Durable sleep
/// - `ctx.promise()` - Create awaitable promise
/// - `ctx.now()` - Deterministic timestamp
#[proc_macro_attribute]
pub fn workflow_actor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let facets = parse_facets(attr.clone());
    let custom_name = parse_name_attr(attr);

    let facets_impl = gen_facets_const(name, &facets);

    // If custom name is provided, use Custom(name) behavior type
    let behavior_type_expr = if let Some(ref custom) = custom_name {
        quote! { plexspaces_core::BehaviorType::Custom(#custom.to_string()) }
    } else {
        quote! { plexspaces_core::BehaviorType::Workflow }
    };

    let expanded = quote! {
        #input

        #facets_impl

        #[plexspaces_sdk::async_trait]
        impl plexspaces_core::Actor for #name {
            fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                #behavior_type_expr
            }

            async fn handle_message(
                &mut self,
                ctx: &plexspaces_core::ActorContext,
                msg: plexspaces_core::Message,
            ) -> Result<(), plexspaces_core::BehaviorError> {
                <Self as plexspaces_behavior::Workflow>::route_workflow_message(self, ctx, msg).await
            }
        }
    };

    TokenStream::from(expanded)
}

// ============================================================================
// #[handler("op", call/cast)] - Mark method as message handler
// ============================================================================

/// Marks a method as a message handler for the given operation.
///
/// ## Usage
/// ```ignore
/// // GenServer handlers - "call" is default, no second param needed
/// #[gen_server_actor]
/// struct BankAccount { ... }
///
/// #[plexspaces_handlers]
/// impl BankAccount {
///     #[handler("deposit")]   // GenServer defaults to call (request-reply)
///     async fn deposit(&mut self, ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> { ... }
///     
///     #[handler("withdraw")]  // GenServer defaults to call
///     async fn withdraw(&mut self, ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> { ... }
/// }
///
/// // Custom/Event actors - specify "cast" for fire-and-forget
/// #[actor]
/// struct AuditLogger { ... }
///
/// #[plexspaces_handlers(custom)]
/// impl AuditLogger {
///     #[handler("log", cast)]  // explicit cast (fire-and-forget)
///     async fn log(&mut self, ctx: &ActorContext, msg: &Message) -> Result<(), BehaviorError> { ... }
/// }
/// ```
///
/// Note: This attribute is a marker; actual dispatch is generated by `#[plexspaces_handlers]`.
#[proc_macro_attribute]
pub fn handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Just pass through - the #[plexspaces_handlers] macro will read these attributes
    item
}

// ============================================================================
// #[init_handler] - Mark method as initialization handler
// ============================================================================

/// Marks a method as the initialization handler, called when actor starts.
///
/// ## Usage
/// ```ignore
/// #[gen_server_actor]
/// struct MyActor { ... }
///
/// #[plexspaces_handlers]
/// impl MyActor {
///     #[init_handler]
///     async fn on_init(&mut self, ctx: &ActorContext) -> Result<(), BehaviorError> {
///         // Initialize resources, load state, etc.
///         Ok(())
///     }
///     
///     #[handler("process")]
///     async fn process(&mut self, ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> { ... }
/// }
/// ```
///
/// ## Semantics
/// - Called once when actor is first activated
/// - Can be async and access ActorContext
/// - Errors during init will fail actor activation
/// - Use for loading state, connecting to resources, etc.
#[proc_macro_attribute]
pub fn init_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Just pass through - the #[plexspaces_handlers] macro will read this attribute
    item
}

// ============================================================================
// #[run_handler] - Mark method as workflow run handler
// ============================================================================

/// Marks a method as the workflow run handler (main execution).
///
/// ## Usage
/// ```ignore
/// #[workflow_actor]
/// struct PaymentWorkflow { ... }
///
/// #[plexspaces_handlers(workflow)]
/// impl PaymentWorkflow {
///     #[run_handler]
///     async fn run(&mut self, ctx: &ActorContext, input: Message) -> Result<Message, BehaviorError> {
///         // Main workflow execution
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn run_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

// ============================================================================
// #[signal_handler("name")] - Mark method as workflow signal handler
// ============================================================================

/// Marks a method as a workflow signal handler.
///
/// ## Usage
/// ```ignore
/// #[workflow_actor]
/// struct PaymentWorkflow { ... }
///
/// #[plexspaces_handlers(workflow)]
/// impl PaymentWorkflow {
///     #[signal_handler("cancel")]
///     async fn on_cancel(&mut self, ctx: &ActorContext, data: Message) -> Result<(), BehaviorError> {
///         // Handle cancellation signal
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn signal_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

// ============================================================================
// #[query_handler("name")] - Mark method as workflow query handler
// ============================================================================

/// Marks a method as a workflow query handler (read-only).
///
/// ## Usage
/// ```ignore
/// #[workflow_actor]
/// struct PaymentWorkflow { ... }
///
/// #[plexspaces_handlers(workflow)]
/// impl PaymentWorkflow {
///     #[query_handler("status")]
///     async fn get_status(&self, ctx: &ActorContext, params: Message) -> Result<Message, BehaviorError> {
///         // Return current status (read-only)
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn query_handler(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

// ============================================================================
// Helper: Parse handler attributes from a method
// ============================================================================

struct HandlerInfo {
    op: String,
    pattern: String, // "call" or "cast"
    method_name: syn::Ident,
}

struct InitHandlerInfo {
    method_name: syn::Ident,
}

struct WorkflowHandlerInfo {
    handler_type: String, // "run", "signal", "query"
    name: Option<String>, // signal/query name
    method_name: syn::Ident,
}

fn parse_handler_attr(attr: &Attribute) -> Option<(String, String)> {
    if !attr.path().is_ident("handler") {
        return None;
    }

    let mut op = String::new();
    let mut pattern = "call".to_string(); // default

    // Parse #[handler("op")] or #[handler("op", call)] or #[handler("op", cast)]
    let tokens = attr.meta.require_list().ok()?.tokens.to_string();
    let parts: Vec<&str> = tokens.split(',').collect();

    if let Some(first) = parts.first() {
        op = first
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
    }

    if let Some(second) = parts.get(1) {
        let inv = second.trim();
        if inv == "call" || inv == "cast" {
            pattern = inv.to_string();
        }
    }

    if op.is_empty() {
        return None;
    }

    Some((op, pattern))
}

fn parse_init_handler_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("init_handler")
}

fn parse_workflow_handler_attr(attr: &Attribute) -> Option<(String, Option<String>)> {
    if attr.path().is_ident("run_handler") {
        return Some(("run".to_string(), None));
    }

    if attr.path().is_ident("signal_handler") {
        let name = attr.meta.require_list().ok().map(|list| {
            let tokens = list.tokens.to_string();
            tokens
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        });
        return Some(("signal".to_string(), name));
    }

    if attr.path().is_ident("query_handler") {
        let name = attr.meta.require_list().ok().map(|list| {
            let tokens = list.tokens.to_string();
            tokens
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        });
        return Some(("query".to_string(), name));
    }

    None
}

fn collect_handlers(impl_block: &ItemImpl) -> Vec<HandlerInfo> {
    let mut handlers = Vec::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if let Some((op, pattern)) = parse_handler_attr(attr) {
                    handlers.push(HandlerInfo {
                        op,
                        pattern,
                        method_name: method.sig.ident.clone(),
                    });
                }
            }
        }
    }

    handlers
}

fn collect_init_handler(impl_block: &ItemImpl) -> Option<InitHandlerInfo> {
    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if parse_init_handler_attr(attr) {
                    return Some(InitHandlerInfo {
                        method_name: method.sig.ident.clone(),
                    });
                }
            }
        }
    }
    None
}

fn collect_workflow_handlers(impl_block: &ItemImpl) -> Vec<WorkflowHandlerInfo> {
    let mut handlers = Vec::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if let Some((handler_type, name)) = parse_workflow_handler_attr(attr) {
                    handlers.push(WorkflowHandlerInfo {
                        handler_type,
                        name,
                        method_name: method.sig.ident.clone(),
                    });
                }
            }
        }
    }

    handlers
}

// ============================================================================
// #[plexspaces_handlers] - Generate dispatch from #[handler] methods
// ============================================================================

/// Scans an impl block for `#[handler]` methods and generates dispatch code.
///
/// ## Behavior Modes
/// - `#[plexspaces_handlers]` or `#[plexspaces_handlers(gen_server)]` - GenServer dispatch
/// - `#[plexspaces_handlers(event)]` - GenEvent dispatch (fire-and-forget)
/// - `#[plexspaces_handlers(custom)]` - Custom Actor dispatch
/// - `#[plexspaces_handlers(fsm)]` - FSM dispatch with state transitions
/// - `#[plexspaces_handlers(workflow)]` - Workflow dispatch (run/signal/query)
///
/// ## For GenServer actors
/// Generates `impl GenServer` with `handle_request` that dispatches to handlers.
///
/// ## For GenEvent actors
/// Generates `impl EventHandler` with `handle_event` that dispatches to handlers.
///
/// ## For Custom actors
/// Generates `impl Actor` with `handle_message` that dispatches to handlers.
///
/// ## For FSM actors
/// Generates `handle_fsm_message` that dispatches based on state and event.
///
/// ## For Workflow actors
/// Generates `impl Workflow` with `run`, `signal`, `query` handlers.
///
/// ## Usage
/// ```ignore
/// #[gen_server_actor]
/// struct WebhookHandler { ... }
///
/// #[plexspaces_handlers]
/// impl WebhookHandler {
///     #[handler("deliver")]  // GenServer defaults to call - no second param needed
///     async fn deliver(&mut self, ctx: &ActorContext, msg: &Message) -> Result<serde_json::Value, BehaviorError> {
///         // ...
///     }
///     
///     #[handler("list")]     // GenServer defaults to call
///     async fn list(&mut self, ctx: &ActorContext, msg: &Message) -> Result<serde_json::Value, BehaviorError> {
///         // ...
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn plexspaces_handlers(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut impl_block = parse_macro_input!(item as ItemImpl);
    let handlers = collect_handlers(&impl_block);
    let init_handler = collect_init_handler(&impl_block);
    let workflow_handlers = collect_workflow_handlers(&impl_block);

    // Get the type name
    let self_ty = &impl_block.self_ty;

    // Determine behavior type from attr: gen_server (default), event, custom, fsm, workflow
    let attr_str = attr.to_string();
    let is_wasm = attr_str.contains("wasm");
    let is_gen_server = attr_str.is_empty() || attr_str.contains("gen_server");
    let is_event = attr_str.contains("event");
    let is_fsm = attr_str.contains("fsm");
    let is_workflow = attr_str.contains("workflow");

    // Remove handler attributes from methods (they're processed)
    for item in &mut impl_block.items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| {
                !attr.path().is_ident("handler")
                    && !attr.path().is_ident("init_handler")
                    && !attr.path().is_ident("run_handler")
                    && !attr.path().is_ident("signal_handler")
                    && !attr.path().is_ident("query_handler")
            });
        }
    }

    if is_wasm {
        // The wasm decorator keeps the WIT boundary byte-oriented so polyglot
        // actor-world guests can decode protobuf payloads with generated models.
        let catch_all_handler = handlers.iter().find(|h| h.op == "*" || h.op == "_");

        let match_arms: Vec<TokenStream2> = handlers
            .iter()
            .filter(|h| h.op != "*" && h.op != "_")
            .map(|h| {
                let op = &h.op;
                let method = &h.method_name;
                quote! {
                    #op => self.#method(from_actor, payload),
                }
            })
            .collect();

        let default_arm = if let Some(catch_all) = catch_all_handler {
            let method = &catch_all.method_name;
            quote! {
                self.#method(from_actor, payload)
            }
        } else {
            quote! {
                Err(format!("unsupported op '{}'", op))
            }
        };

        let init_impl = if let Some(init) = init_handler {
            let method = init.method_name;
            quote! {
                fn init(&mut self, config: &[u8]) -> Result<(), String> {
                    self.#method(config)
                }
            }
        } else {
            quote! {}
        };

        let expanded = quote! {
            #impl_block

            impl plexspaces_sdk::simple_actor::ActorWorldHandlers for #self_ty {
                #init_impl

                fn handle_operation(
                    &mut self,
                    from_actor: &str,
                    op: &str,
                    payload: &[u8],
                ) -> Result<Vec<u8>, String> {
                    match op {
                        #(#match_arms)*
                        _ => #default_arm,
                    }
                }
            }
        };

        return TokenStream::from(expanded);
    }

    // Generate init call if present
    let init_call = if let Some(ref init) = init_handler {
        let method = &init.method_name;
        quote! {
            // Call init handler if present
            // Note: This is called by the actor lifecycle, not in handle_message
            #[allow(dead_code)]
            async fn __plexspaces_init(&mut self, ctx: &plexspaces_core::ActorContext) -> Result<(), plexspaces_core::BehaviorError> {
                self.#method(ctx).await
            }
        }
    } else {
        quote! {}
    };

    // Generate workflow impl if workflow mode
    if is_workflow {
        let mut run_handler_method = None;
        let mut signal_handlers: Vec<(&str, &syn::Ident)> = Vec::new();
        let mut query_handlers: Vec<(&str, &syn::Ident)> = Vec::new();

        for h in &workflow_handlers {
            match h.handler_type.as_str() {
                "run" => run_handler_method = Some(&h.method_name),
                "signal" => {
                    if let Some(ref name) = h.name {
                        signal_handlers.push((name.as_str(), &h.method_name));
                    }
                }
                "query" => {
                    if let Some(ref name) = h.name {
                        query_handlers.push((name.as_str(), &h.method_name));
                    }
                }
                _ => {}
            }
        }

        let run_impl = if let Some(method) = run_handler_method {
            quote! {
                self.#method(ctx, input).await
            }
        } else {
            quote! {
                Err(plexspaces_core::BehaviorError::UnsupportedMessage)
            }
        };

        let signal_arms: Vec<TokenStream2> = signal_handlers
            .iter()
            .map(|(name, method)| {
                quote! {
                    #name => self.#method(ctx, data).await,
                }
            })
            .collect();

        let query_arms: Vec<TokenStream2> = query_handlers
            .iter()
            .map(|(name, method)| {
                quote! {
                    #name => self.#method(ctx, params).await,
                }
            })
            .collect();

        let expanded = quote! {
            #impl_block

            #init_call

            #[plexspaces_sdk::async_trait]
            impl plexspaces_behavior::Workflow for #self_ty {
                async fn run(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    input: plexspaces_core::Message,
                ) -> Result<plexspaces_core::Message, plexspaces_core::BehaviorError> {
                    #run_impl
                }

                async fn signal(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    name: String,
                    data: plexspaces_core::Message,
                ) -> Result<(), plexspaces_core::BehaviorError> {
                    match name.as_str() {
                        #(#signal_arms)*
                        _ => Err(plexspaces_core::BehaviorError::UnsupportedMessage)
                    }
                }

                async fn query(
                    &self,
                    ctx: &plexspaces_core::ActorContext,
                    name: String,
                    params: plexspaces_core::Message,
                ) -> Result<plexspaces_core::Message, plexspaces_core::BehaviorError> {
                    match name.as_str() {
                        #(#query_arms)*
                        _ => Err(plexspaces_core::BehaviorError::UnsupportedMessage)
                    }
                }
            }
        };

        return TokenStream::from(expanded);
    }

    if handlers.is_empty() && !is_fsm {
        // No handlers, just return the impl block with init
        let expanded = quote! {
            #impl_block
            #init_call
        };
        return TokenStream::from(expanded);
    }

    // Generate dispatch match arms
    // Check for catch-all handler ("*" or "_")
    let catch_all_handler = handlers.iter().find(|h| h.op == "*" || h.op == "_");

    let match_arms: Vec<TokenStream2> = handlers.iter()
        .filter(|h| h.op != "*" && h.op != "_") // Exclude catch-all from match arms
        .map(|h| {
            let op = &h.op;
            let method = &h.method_name;
            let is_call = h.pattern == "call";

            if is_call {
                // Call semantics: handler returns Result<Value, BehaviorError>, we send reply
                quote! {
                    #op => {
                        // Log handler match at debug level
                        tracing::debug!(
                            "[HANDLER_DISPATCH] Matched handler: op={:?}, method={}, message_id={}, message_type={}, sender_id={}, receiver_id={}, correlation_id={}",
                            #op, stringify!(#method), msg.id, msg.message_type, msg.sender_id, msg.receiver_id, msg.correlation_id
                        );
                        let result = self.#method(ctx, &msg).await?;
                        // Send reply for call semantics
                        if !msg.sender_id.is_empty() {
                            let reply_payload = serde_json::to_vec(&result)
                                .unwrap_or_else(|_| b"{}".to_vec());
                            let mut reply = plexspaces_core::Message::default();
                            reply.payload = reply_payload;
                            reply.receiver_id = msg.sender_id.clone();
                            reply.sender_id = msg.receiver_id.clone();
                            if !msg.correlation_id.is_empty() {
                                reply.correlation_id = msg.correlation_id.clone();
                            }
                            // Note: reply.id will be set by send_reply() with "res-" prefix
                            ctx.send_reply(
                                Some(&msg.correlation_id),
                                &msg.sender_id,
                                ctx.actor_id().clone(),
                                reply,
                            ).await.map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
                        }
                        Ok(())
                    }
                }
            } else {
                // Cast semantics: fire-and-forget, no reply
                quote! {
                    #op => {
                        self.#method(ctx, &msg).await
                    }
                }
            }
        }).collect();

    // Add catch-all handler as default case if present
    let default_arm = if let Some(catch_all) = catch_all_handler {
        let method = &catch_all.method_name;
        let is_call = catch_all.pattern == "call";

        if is_call {
            quote! {
                _ => {
                    // Log catch-all handler match at debug level (guarded)
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            "[HANDLER_DISPATCH] Matched catch-all handler (*): method={}, message_id={}, message_type={}, sender_id={}, receiver_id={}, correlation_id={}, op={:?}",
                            stringify!(#method), msg.id, msg.message_type, msg.sender_id, msg.receiver_id, msg.correlation_id, op
                        );
                    }
                    let result = self.#method(ctx, &msg).await?;
                    // Send reply for call semantics (GenServer default)
                    // CRITICAL: For GenServer, handlers default to "call" (request-reply)
                    // Only send reply if sender_id is not empty (indicates request-reply pattern)
                    if !msg.sender_id.is_empty() {
                        let reply_payload = serde_json::to_vec(&result)
                            .unwrap_or_else(|_| b"{}".to_vec());
                        let mut reply = plexspaces_core::Message::default();
                        reply.payload = reply_payload;
                        reply.receiver_id = msg.sender_id.clone(); // Reply goes TO sender (temporary sender)
                        reply.sender_id = msg.receiver_id.clone(); // Reply comes FROM this actor
                        if !msg.correlation_id.is_empty() {
                            reply.correlation_id = msg.correlation_id.clone();
                        }
                        // Note: reply.id will be set by send_reply() with "res-" prefix
                        ctx.send_reply(
                            Some(&msg.correlation_id),
                            &msg.sender_id,
                            ctx.actor_id().clone(),
                            reply,
                        ).await.map_err(|e| {
                            plexspaces_core::BehaviorError::ProcessingError(e.to_string())
                        })?;
                    }
                    Ok(())
                }
            }
        } else {
            quote! {
                _ => {
                    self.#method(ctx, &msg).await
                }
            }
        }
    } else {
        quote! {
            _ => Err(plexspaces_core::BehaviorError::UnsupportedMessage)
        }
    };

    // Generate the trait impl
    let gen_server_impl = if is_gen_server {
        quote! {
            #[plexspaces_sdk::async_trait]
            impl plexspaces_behavior::GenServer for #self_ty {
                async fn handle_request(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    msg: plexspaces_core::Message,
                ) -> Result<(), plexspaces_core::BehaviorError> {
                    // Parse payload to determine operation
                    let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    // Get operation from payload.action, payload.op, payload.msg_type, or msg.message_type
                    let op = payload.get("action")
                        .or_else(|| payload.get("op"))
                        .or_else(|| payload.get("msg_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_else(|| {
                            // If message_type is "call" or "cast", look in payload
                            if msg.message_type == "call" || msg.message_type == "cast" {
                                ""
                            } else {
                                &msg.message_type
                            }
                        });

                    // Log operation extraction at debug level (guarded)
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            "[HANDLER_DISPATCH] Operation extracted: op={:?}, message_id={}, message_type={}, sender_id={}, receiver_id={}, correlation_id={}, payload_keys={:?}",
                            op, msg.id, msg.message_type, msg.sender_id, msg.receiver_id, msg.correlation_id,
                            payload.as_object().map(|o| o.keys().collect::<Vec<_>>())
                        );
                    }

                    match op {
                        #(#match_arms)*
                        #default_arm
                    }
                }
            }
        }
    } else if is_event {
        // GenEvent: implement EventHandler trait
        quote! {
            #[plexspaces_sdk::async_trait]
            impl plexspaces_behavior::EventHandler for #self_ty {
                async fn handle_event(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    msg: plexspaces_core::Message,
                ) -> Result<(), plexspaces_core::BehaviorError> {
                    // Parse payload to determine operation
                    let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    let op = payload.get("action")
                        .or_else(|| payload.get("op"))
                        .or_else(|| payload.get("event_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&msg.message_type);

                    match op {
                        #(#match_arms)*
                        #default_arm
                    }
                }
            }
        }
    } else if is_fsm {
        // FSM: generate handle_fsm_message method
        quote! {
            impl #self_ty {
                /// Handle FSM message - dispatch based on current state and event
                async fn handle_fsm_message(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    msg: plexspaces_core::Message,
                ) -> Result<(), plexspaces_core::BehaviorError> {
                    // Parse payload to determine event
                    let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    let event = payload.get("event")
                        .or_else(|| payload.get("action"))
                        .or_else(|| payload.get("op"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&msg.message_type);

                    match event {
                        #(#match_arms)*
                        _ => Err(plexspaces_core::BehaviorError::UnsupportedMessage)
                    }
                }
            }
        }
    } else {
        // Custom actor: dispatch in handle_message
        quote! {
            #[plexspaces_sdk::async_trait]
            impl plexspaces_core::Actor for #self_ty {
                fn behavior_type(&self) -> plexspaces_core::BehaviorType {
                    plexspaces_core::BehaviorType::Custom(stringify!(#self_ty).to_string())
                }

                async fn handle_message(
                    &mut self,
                    ctx: &plexspaces_core::ActorContext,
                    msg: plexspaces_core::Message,
                ) -> Result<(), plexspaces_core::BehaviorError> {
                    // Parse payload to determine operation
                    let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    let op = payload.get("action")
                        .or_else(|| payload.get("op"))
                        .or_else(|| payload.get("msg_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&msg.message_type);

                    match op {
                        #(#match_arms)*
                        #default_arm
                    }
                }
            }
        }
    };

    let expanded = quote! {
        #impl_block

        #init_call

        #gen_server_impl
    };

    TokenStream::from(expanded)
}
