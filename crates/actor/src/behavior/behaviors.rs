// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Actor behaviors module
//!
//! Implements OTP-inspired behaviors (GenServer, GenEvent, etc.) as composable
//! capabilities rather than separate actor types.

// workflow module is declared in lib.rs, not here

use async_trait::async_trait;

// Import from core module (moved into actor crate)
use crate::core::{Actor, BehaviorError, BehaviorType, RequestContextExt};
use plexspaces_proto::common::v1::Message;

// Re-export workflow execution context and retry config (single run-with-config API; proto RetryConfig)
pub use super::workflow::{default_retry_config, ExecutionContext, RetryConfig};

/// GenServer-like behavior trait for request/reply patterns
///
/// ## Design Philosophy (Phase 2 Cleanup)
/// GenServer handles ONLY synchronous requests (request-reply pattern).
/// For fire-and-forget events, use GenEvent instead.
///
/// ## Single Handler
/// - `handle_request`: Synchronous request-reply (expects response)
///
/// ## Why Only One Handler?
/// - **Separation of concerns**: GenServer = requests, GenEvent = events
/// - **Simpler mental model**: "GenServer handles calls, GenEvent handles events"
/// - **Type-safe**: Return type enforces semantics (Message = must reply)
/// - **Matches Erlang/OTP**: gen_server:call vs gen_event:notify
///
/// ## Inspiration from Multiple Frameworks
/// - **Erlang/OTP**: gen_server:call (request only)
/// - **Orleans**: Grain methods returning Task<T> (request)
/// - **Restate**: Exclusive handlers with return value
/// - **Cloudflare Durable Objects**: fetch() HTTP request handler
///
/// ## Proto-First Design
/// This trait implements the contract defined in proto/plexspaces/v1/behaviors.proto:
/// ```protobuf
/// service GenServerService {
///   rpc HandleRequest(HandleRequestRequest) returns (HandleRequestResponse);
/// }
/// ```
///
/// ## Note
/// For event handling (fire-and-forget), use GenEvent instead.
///
/// ## Example
/// ```rust,ignore
/// struct MyGenServer {
///     state: MyState,
/// }
///
/// impl ActorBehavior for MyGenServer {
///     async fn handle_message(&mut self, ctx: BehaviorContext) -> Result<(), BehaviorError> {
///         self.route_message(ctx).await
///     }
///
///     fn behavior_type(&self) -> BehaviorType {
///         BehaviorType::GenServer
///     }
/// }
///
/// impl GenServer for MyGenServer {
///     async fn handle_request(&mut self, msg: Message, ctx: &BehaviorContext) -> Result<Message, BehaviorError> {
///         // Synchronous - MUST return a reply
///         // Like: gen_server:call, Orleans Task<T>, Cloudflare fetch()
///     }
/// }
///
/// // For events, use GenEvent instead:
/// impl ActorBehavior for MyGenEvent {
///     async fn handle_message(&mut self, ctx: BehaviorContext) -> Result<(), BehaviorError> {
///         // Handle events (fire-and-forget)
///     }
/// }
/// ```
#[async_trait]
/// GenServer trait - OTP-inspired request/reply behavior
///
/// ## Purpose
/// Trait for implementing GenServer-style actors that handle request/reply patterns.
/// This is what you implement to create a GenServer actor.
///
/// ## Note
/// Previously called `GenServerBehavior` - renamed to `GenServer` for consistency
/// with the Actor trait (removed Behavior suffix).
pub trait GenServer: Actor {
    /// Handle synchronous request (expects reply)
    ///
    /// ## Semantics
    /// - Handler uses ActorRef::send_reply() to send reply (Erlang-style)
    /// - No need to return Message - reply is sent via ActorRef::send_reply()
    /// - Blocks caller until reply is sent
    ///
    /// ## Similar To
    /// - Erlang: `gen_server:handle_call(Message, From, State)` - `From` is sender PID, use `gen_server:reply(From, Reply)`
    /// - Akka: `receive { case msg => sender() ! reply }` - `sender()` is ActorRef, reply goes directly
    /// - Orleans: Grain method returning `Task<T>`
    /// - Restate: Exclusive handler with return value
    /// - Cloudflare: `fetch()` HTTP request handler
    ///
    /// ## Proto Definition
    /// Maps to: `rpc HandleRequest(HandleRequestRequest) returns (HandleRequestResponse)`
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    ///
    /// ## Note
    /// For fire-and-forget events, use GenEvent instead of GenServer.
    ///
    /// ## Example
    /// ```rust,ignore
    /// async fn handle_request(
    ///     &mut self,
    ///     ctx: &ActorContext,
    ///     msg: Message,
    /// ) -> Result<(), BehaviorError> {
    ///     if !msg.sender_id.is_empty() {
    ///         let mut reply = Message { payload: b"response".to_vec(), ..Default::default() };
    ///         reply.receiver_id = msg.sender_id.clone();
    ///         reply.sender_id = msg.receiver_id.clone();
    ///         if !msg.correlation_id.is_empty() {
    ///             reply.correlation_id = msg.correlation_id.clone();
    ///         }
    ///         let actor_service = ctx.service_locator.get_actor_service().await?;
    ///         actor_service.send(&msg.sender_id, reply).await?;
    ///     }
    ///     Ok(())
    /// }
    /// ```
    async fn handle_request(
        &mut self,
        ctx: &crate::core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError>;

    /// Route message to appropriate handler based on MessageType
    ///
    /// This is a convenience method that Actor::handle_message can delegate to.
    /// It examines the message type and calls the appropriate handler.
    ///
    /// ## Default Implementation
    /// Routes based on MessageType:
    /// - `MessageType::Call` → `handle_request()` (sync, needs reply)
    /// - `MessageType::Cast` or `MessageType::Info` → Error (use GenEvent for events)
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    ///
    /// ## Note (Phase 2 Cleanup)
    /// GenServer only handles Call messages. For Cast/Info, use GenEvent instead.
    async fn route_message(
        &mut self,
        ctx: &crate::core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        use crate::behavior::MessageTypeExt;
        use std::thread_local;

        // RECURSION DETECTION: Track call depth to detect infinite loops
        thread_local! {
            static ROUTE_MESSAGE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }

        let depth = ROUTE_MESSAGE_DEPTH.with(|d| {
            let current = d.get();
            d.set(current + 1);
            current
        });

        // Safety check: prevent infinite recursion
        const MAX_RECURSION_DEPTH: usize = 100;
        if depth > MAX_RECURSION_DEPTH {
            let _ = ROUTE_MESSAGE_DEPTH.with(|d| d.set(0)); // Reset on panic
            let backtrace = std::backtrace::Backtrace::capture();
            tracing::error!(
                "INFINITE RECURSION DETECTED IN route_message! depth={}, max={}, sender={}, target={}, correlation_id={}, backtrace={:?}",
                depth, MAX_RECURSION_DEPTH, msg.sender_id, msg.receiver_id, msg.correlation_id, backtrace
            );
            return Err(BehaviorError::ProcessingError(format!(
                "Infinite recursion detected in route_message (depth: {})",
                depth
            )));
        }

        let msg_type = msg.message_type();

        // Get target actor ID from message.receiver_id (the actor receiving this message)
        let target_actor_id = msg.receiver_id.clone();

        // VALIDATION: Check for self-messaging (source == target)
        if !msg.sender_id.is_empty() {
            if msg.sender_id == target_actor_id {
                let _ = ROUTE_MESSAGE_DEPTH.with(|d| d.set(0)); // Reset on error
                tracing::error!(
                    "🟡 [ROUTE_MESSAGE] SELF-MESSAGING DETECTED! sender_id={}, target_actor_id={}, message_type={:?}, depth={}",
                    msg.sender_id, target_actor_id, msg_type, depth
                );
                return Err(BehaviorError::ProcessingError(format!(
                    "Self-messaging detected: actor {} cannot send message to itself",
                    msg.sender_id
                )));
            }
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    "[ROUTE_MESSAGE] START: depth={}, message_id={}, sender={}, target_actor_id={}, message_type={:?}, message_type_str={}, correlation_id={}",
                    depth, msg.id, msg.sender_id, target_actor_id, msg_type, msg.message_type, msg.correlation_id
                );
            }
        } else if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "[ROUTE_MESSAGE] START: depth={}, message_id={}, No sender (fire-and-forget), target_actor_id={}, message_type={:?}, message_type_str={}",
                depth, msg.id, target_actor_id, msg_type, msg.message_type
            );
        }

        match msg_type {
            MessageType::Call => {
                // Clone values needed for reply before moving msg into handle_request
                let message_id = msg.id.clone();
                let correlation_id = msg.correlation_id.clone();
                let sender_id = msg.sender_id.clone();
                let receiver_id = msg.receiver_id.clone();

                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        "[ROUTE_MESSAGE] Call: message_id={}, target={}, correlation_id={}",
                        message_id,
                        target_actor_id,
                        correlation_id
                    );
                }

                let result = self.handle_request(ctx, msg).await;
                if tracing::enabled!(tracing::Level::DEBUG) {
                    if result.is_ok() {
                        tracing::debug!(
                            "[ROUTE_MESSAGE] Completed: message_id={}, target={}, correlation_id={}",
                            message_id, target_actor_id, correlation_id
                        );
                    }
                }

                if let Err(ref e) = result {
                    tracing::warn!(
                        "[ROUTE_MESSAGE] Failed: message_id={}, target={}, correlation_id={}, error={}",
                        message_id, target_actor_id, correlation_id, e
                    );
                    // Erlang gen_server semantics: send error reply so caller gets fast failure
                    // instead of timeout, but actor continues processing subsequent messages.
                    if !sender_id.is_empty() && !correlation_id.is_empty() {
                        let error_payload = serde_json::json!({
                            "error": e.to_string(),
                            "success": false,
                        });
                        let error_reply = Message {
                            id: format!("res-{}", ulid::Ulid::new()),
                            payload: serde_json::to_vec(&error_payload).unwrap_or_default(),
                            message_type: "error_reply".to_string(),
                            receiver_id: sender_id.clone(),
                            sender_id: receiver_id.clone(),
                            correlation_id: correlation_id.clone(),
                            ..Default::default()
                        };
                        let from_actor_id = ctx
                            .self_ref()
                            .map(|r| r.id().clone())
                            .or_else(|| crate::ActorId::from_canonical(&target_actor_id).ok());
                        if let Some(actor_id) = from_actor_id {
                            let _ = ctx
                                .send_reply(
                                    Some(&correlation_id),
                                    &sender_id,
                                    actor_id,
                                    error_reply,
                                )
                                .await;
                        }
                        // Actor survives — matches Erlang where a failed handle_call
                        // returns {reply, {error, Reason}, State} and the process continues.
                    } else {
                        // No caller to reply to — propagate error up to mailbox loop
                        // which may trigger supervisor restart depending on policy.
                        result?;
                    }
                }

                metrics::counter!("plexspaces_behavior_genserver_replies_sent_total", "behavior" => "genserver").increment(1);

                // Decrement recursion depth on success
                let _ = ROUTE_MESSAGE_DEPTH.with(|d| {
                    let current = d.get();
                    if current > 0 {
                        d.set(current - 1);
                    }
                });

                Ok(())
            }
            MessageType::Cast => {
                // Handle Cast (fire-and-forget) - call handle_request but don't require reply
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        "[ROUTE_MESSAGE] CAST: Routing to handle_request (fire-and-forget): message_id={}, sender={}, target_actor_id={}",
                        msg.id, msg.sender_id, target_actor_id
                    );
                }

                // Clone values for logging before moving msg
                let message_id = msg.id.clone();
                let _sender_id = msg.sender_id.clone();

                // Call handle_request - actor can choose to send reply or not
                // For Cast, reply is optional (fire-and-forget)
                let _ = self.handle_request(ctx, msg).await;

                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        "[ROUTE_MESSAGE] CAST handle_request completed: message_id={}, target_actor_id={}",
                        message_id, target_actor_id
                    );
                }

                // Decrement recursion depth on success
                let _ = ROUTE_MESSAGE_DEPTH.with(|d| {
                    let current = d.get();
                    if current > 0 {
                        d.set(current - 1);
                    }
                });

                Ok(())
            }
            MessageType::Info => {
                // Decrement recursion depth on error
                let _ = ROUTE_MESSAGE_DEPTH.with(|d| {
                    let current = d.get();
                    if current > 0 {
                        d.set(current - 1);
                    }
                });
                // GenServer doesn't handle Info messages - use GenEvent instead
                Err(BehaviorError::UnsupportedMessage)
            }
            _ => {
                // Decrement recursion depth
                let _ = ROUTE_MESSAGE_DEPTH.with(|d| {
                    let current = d.get();
                    if current > 0 {
                        d.set(current - 1);
                    }
                });
                Ok(()) // Other message types not handled by GenServer
            }
        }
    }
}

/// GenEvent-like behavior for event handling
pub struct GenEventBehavior {
    handlers: Vec<Box<dyn EventHandler>>,
}

/// Event handler trait
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Handle an event
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    async fn handle_event(
        &mut self,
        ctx: &crate::core::ActorContext,
        event: Message,
    ) -> Result<(), BehaviorError>;
}

impl Default for GenEventBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl GenEventBehavior {
    /// Create a new GenEvent behavior
    pub fn new() -> Self {
        GenEventBehavior {
            handlers: Vec::new(),
        }
    }

    /// Add an event handler
    pub fn add_handler(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    /// Remove all handlers
    pub fn clear_handlers(&mut self) {
        self.handlers.clear();
    }
}

#[async_trait]
impl Actor for GenEventBehavior {
    async fn handle_message(
        &mut self,
        ctx: &crate::core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Notify all handlers
        for handler in &mut self.handlers {
            handler.handle_event(ctx, msg.clone()).await?;
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenEvent
    }
}

/// State transition function for `GenStateMachineBehavior`.
type TransitionFn<S, E> = Box<dyn Fn(&S, &E) -> Option<S> + Send + Sync>;

/// GenStateMachine-like FSM behavior
pub struct GenStateMachineBehavior<S, E> {
    current_state: S,
    transition_fn: TransitionFn<S, E>,
    state_handlers: std::collections::HashMap<String, Box<dyn StateHandler<S, E>>>,
}

impl<S, E> GenStateMachineBehavior<S, E>
where
    S: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    /// Create new FSM behavior with initial state and transition function
    pub fn new(initial_state: S, transition_fn: TransitionFn<S, E>) -> Self {
        GenStateMachineBehavior {
            current_state: initial_state,
            transition_fn,
            state_handlers: Default::default(),
        }
    }

    /// Get current state
    pub fn current_state(&self) -> &S {
        &self.current_state
    }

    /// Add state handler for a specific state
    pub fn add_state_handler(&mut self, state_name: String, handler: Box<dyn StateHandler<S, E>>) {
        self.state_handlers.insert(state_name, handler);
    }

    /// Perform state transition
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the event.
    pub async fn transition(
        &mut self,
        ctx: &crate::core::ActorContext,
        event: E,
    ) -> Result<(), BehaviorError> {
        // 1. Get next state from transition function
        let next_state = (self.transition_fn)(&self.current_state, &event);

        // 2. If there's a next state, call handler and update
        if let Some(new_state) = next_state {
            // Note: Since we don't know how to convert state to string for lookup,
            // we call the first handler that doesn't return None
            // In practice, users should only add handlers for specific states
            let mut handler_override = None;
            for (_, handler) in self.state_handlers.iter_mut() {
                if let Some(handler_state) = handler.handle(ctx, &new_state, event.clone()).await? {
                    // Handler can override the state
                    handler_override = Some(handler_state);
                    break; // Only call first handler that returns Some
                }
            }

            // Apply handler override or use transition_fn result
            if let Some(override_state) = handler_override {
                self.current_state = override_state;
            } else {
                self.current_state = new_state;
            }
        }

        // No transition occurred (stayed in same state)
        Ok(())
    }
}

// Note: ActorBehavior implementation is limited because we cannot deserialize
// Message into Event E generically without type information
#[async_trait]
impl<S, E> Actor for GenStateMachineBehavior<S, E>
where
    S: Clone + Send + Sync + 'static,
    E: Clone + Send + Sync + 'static,
{
    async fn handle_message(
        &mut self,
        _ctx: &crate::core::ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        // Note: Generic message -> event deserialization is intentionally not implemented.
        // Users should call transition() directly with typed events for type safety.
        // This follows Rust's philosophy: explicit is better than implicit.
        // Similar frameworks (Erlang gen_event, Akka, Ray) also require explicit deserialization.
        Err(BehaviorError::UnsupportedMessage)
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenStateMachine
    }
}

/// State handler trait
#[async_trait]
pub trait StateHandler<S, E>: Send + Sync {
    /// Handle event in current state
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by state and event.
    async fn handle(
        &mut self,
        ctx: &crate::core::ActorContext,
        state: &S,
        event: E,
    ) -> Result<Option<S>, BehaviorError>;
}

/// Workflow behavior trait (Restate-inspired)
///
/// ## Design Philosophy
/// This trait extends ActorBehavior for durable workflow patterns inspired by:
/// - **Restate**: run/signal/query handlers for workflows
/// - **Temporal**: Workflow execution with signals and queries
/// - **Cloudflare Durable Objects**: Durable state with fetch pattern
///
/// ## Handler Types
/// - `run`: Main workflow execution (exclusive, one at a time)
/// - `signal`: External events that modify workflow state
/// - `query`: Read-only queries (can be concurrent)
///
/// ## Proto-First Design
/// This trait implements the contract defined in proto/plexspaces/v1/behaviors.proto:
/// ```protobuf
/// service WorkflowService {
///   rpc Run(WorkflowRunRequest) returns (WorkflowRunResponse);
///   rpc Signal(WorkflowSignalRequest) returns (google.protobuf.Empty);
///   rpc Query(WorkflowQueryRequest) returns (WorkflowQueryResponse);
/// }
/// ```
///
/// ## Example
/// ```rust,ignore
/// struct MyWorkflow {
///     state: WorkflowState,
/// }
///
/// impl Actor for MyWorkflow {
///     async fn handle_message(&mut self, ctx: BehaviorContext) -> Result<(), BehaviorError> {
///         self.route_workflow_message(ctx).await
///     }
///
///     fn behavior_type(&self) -> BehaviorType {
///         BehaviorType::Workflow
///     }
/// }
///
/// impl Workflow for MyWorkflow {
///     async fn run(&mut self, input: Message, ctx: &BehaviorContext) -> Result<Message, BehaviorError> {
///         // Main workflow execution
///     }
///
///     async fn signal(&mut self, name: String, data: Message, ctx: &BehaviorContext) -> Result<(), BehaviorError> {
///         // Handle external signals
///     }
///
///     async fn query(&self, name: String, params: Message, ctx: &BehaviorContext) -> Result<Message, BehaviorError> {
///         // Read-only queries
///     }
/// }
/// ```
#[async_trait]
/// Workflow trait - OTP-inspired durable workflow behavior
///
/// ## Purpose
/// Trait for implementing workflow-style actors that handle durable execution patterns.
/// This is what you implement to create a Workflow actor.
///
/// ## Note
/// Previously called `WorkflowBehavior` - renamed to `Workflow` for consistency
/// with the Actor trait (removed Behavior suffix).
pub trait Workflow: Actor {
    /// Run the workflow (exclusive, one at a time)
    ///
    /// ## Semantics
    /// - Main workflow execution
    /// - Always exclusive (only one run() at a time)
    /// - Returns workflow result
    ///
    /// ## Similar To
    /// - Restate: `workflow.run()`
    /// - Temporal: Workflow execution function
    /// - Cloudflare: Durable Object main handler
    ///
    /// ## Proto Definition
    /// Workflow behavior is defined in workflow.proto (source of truth).
    /// This trait maps to workflow execution patterns (run/signal/query).
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    async fn run(
        &mut self,
        ctx: &crate::core::ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError>;

    /// Handle workflow signal (modifies state)
    ///
    /// ## Semantics
    /// - External events that modify workflow state
    /// - Can be called while workflow is running
    /// - Fire-and-forget (no return value)
    ///
    /// ## Similar To
    /// - Restate: `workflow.signal()`
    /// - Temporal: Signal handlers
    /// - Event-driven state modifications
    ///
    /// ## Proto Definition
    /// Workflow behavior is defined in workflow.proto (source of truth).
    /// This maps to SignalExecution RPC in workflow.proto.
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by name and data.
    async fn signal(
        &mut self,
        ctx: &crate::core::ActorContext,
        name: String,
        data: Message,
    ) -> Result<(), BehaviorError>;

    /// Handle workflow query (read-only)
    ///
    /// ## Semantics
    /// - Read-only queries (does not modify state)
    /// - Can be concurrent (multiple queries at once)
    /// - Returns query result
    ///
    /// ## Similar To
    /// - Restate: `workflow.query()`
    /// - Temporal: Query handlers
    /// - Read-only state inspection
    ///
    /// ## Proto Definition
    /// Workflow behavior is defined in workflow.proto (source of truth).
    /// This maps to QueryExecution RPC in workflow.proto.
    ///
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by name and params.
    async fn query(
        &self,
        ctx: &crate::core::ActorContext,
        name: String,
        params: Message,
    ) -> Result<Message, BehaviorError>;

    /// Route workflow message to appropriate handler
    ///
    /// Default implementation routes based on MessageType:
    /// - `MessageType::WorkflowRun` → `run()`
    /// - `MessageType::WorkflowSignal(name)` → `signal(name, ...)`
    /// - `MessageType::WorkflowQuery(name)` → `query(name, ...)`
    /// ## Go-Style Signature
    /// Context is the first parameter (Go convention), followed by the message.
    async fn route_workflow_message(
        &mut self,
        ctx: &crate::core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        use crate::behavior::MessageTypeExt;
        match msg.message_type() {
            MessageType::WorkflowRun => {
                let span = tracing::span!(tracing::Level::DEBUG, "workflow.run");
                let _enter = span.enter();

                metrics::counter!("plexspaces_behavior_workflow_runs_total", "behavior" => "workflow").increment(1);
                let start = std::time::Instant::now();

                let result = self.run(ctx, msg.clone()).await?;

                let duration = start.elapsed();
                metrics::histogram!("plexspaces_behavior_workflow_run_duration_seconds", "behavior" => "workflow").record(duration.as_secs_f64());

                // Store result for queries or send back to caller using ActorRef
                // Prefer reply_to (for ask pattern) over sender_id (for backward compatibility)
                let reply_target = if !msg.reply_to.is_empty() {
                    Some(msg.reply_to.as_str())
                } else if !msg.sender_id.is_empty() {
                    Some(msg.sender_id.as_str())
                } else {
                    None
                };

                if let Some(target_id) = reply_target {
                    // Use correlation_id from message (set by ask()) or from headers (backward compatibility)
                    let mut reply_msg = result;
                    if !msg.correlation_id.is_empty() {
                        reply_msg.correlation_id = msg.correlation_id.clone();
                    } else if let Some(corr_id) = msg.headers.get("correlation_id") {
                        reply_msg.correlation_id = corr_id.clone();
                    }

                    // Use ActorService::send() to send reply (handles local/remote automatically)
                    // ActorService::send() will route via ActorRef::tell() which handles temporary sender IDs and correlation_id routing
                    let actor_service =
                        ctx.service_locator
                            .get_actor_service()
                            .await
                            .ok_or_else(|| {
                                BehaviorError::ProcessingError(
                                    "ActorService not available in ServiceLocator".to_string(),
                                )
                            })?;

                    // Set receiver_id to sender_id (the actor that called ask())
                    reply_msg.receiver_id = target_id.to_string();
                    // Set sender_id to this actor's ID
                    reply_msg.sender_id = msg.receiver_id.clone();

                    let reply_ctx = crate::core::RequestContext::new_without_auth(
                        ctx.tenant_id.clone(),
                        ctx.namespace.clone(),
                    );
                    actor_service.send(&reply_ctx, target_id.as_ref(), reply_msg).await
                        .map_err(|e| {
                            metrics::counter!("plexspaces_behavior_workflow_reply_errors_total", "behavior" => "workflow", "error" => "send_failed", "type" => "run").increment(1);
                            tracing::error!(error = %e, "Failed to send workflow run reply");
                            BehaviorError::ProcessingError(format!("Failed to send workflow run reply: {}", e))
                        })?;
                    metrics::counter!("plexspaces_behavior_workflow_replies_sent_total", "behavior" => "workflow", "type" => "run").increment(1);
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(target_id = %target_id, "Workflow run reply sent");
                    }
                } else {
                    tracing::warn!("Workflow run has no reply_to or sender_id, reply not sent");
                    metrics::counter!("plexspaces_behavior_workflow_reply_errors_total", "behavior" => "workflow", "error" => "no_sender", "type" => "run").increment(1);
                }

                Ok(())
            }
            MessageType::WorkflowSignal(name) => {
                let span =
                    tracing::span!(tracing::Level::DEBUG, "workflow.signal", signal_name = %name);
                let _enter = span.enter();

                metrics::counter!("plexspaces_behavior_workflow_signals_total", "behavior" => "workflow", "signal" => name.clone()).increment(1);
                let start = std::time::Instant::now();

                self.signal(ctx, name.clone(), msg.clone()).await?;

                let duration = start.elapsed();
                metrics::histogram!("plexspaces_behavior_workflow_signal_duration_seconds", "behavior" => "workflow", "signal" => name.clone()).record(duration.as_secs_f64());

                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(signal_name = %name, "Workflow signal processed");
                }
                Ok(())
            }
            MessageType::WorkflowQuery(name) => {
                let span =
                    tracing::span!(tracing::Level::DEBUG, "workflow.query", query_name = %name);
                let _enter = span.enter();

                metrics::counter!("plexspaces_behavior_workflow_queries_total", "behavior" => "workflow", "query" => name.clone()).increment(1);
                let start = std::time::Instant::now();

                let result = self.query(ctx, name.clone(), msg.clone()).await?;

                let duration = start.elapsed();
                metrics::histogram!("plexspaces_behavior_workflow_query_duration_seconds", "behavior" => "workflow", "query" => name.clone()).record(duration.as_secs_f64());

                // Send result back to caller using ActorService
                // ActorService automatically handles local/remote routing based on node_id
                // Prefer reply_to (for ask pattern) over sender_id (for backward compatibility)
                let reply_target = if !msg.reply_to.is_empty() {
                    Some(msg.reply_to.as_str())
                } else if !msg.sender_id.is_empty() {
                    Some(msg.sender_id.as_str())
                } else {
                    None
                };

                if let Some(target_id) = reply_target {
                    // Use correlation_id from message (set by ask()) or from headers (backward compatibility)
                    let mut reply_msg = result;
                    if !msg.correlation_id.is_empty() {
                        reply_msg.correlation_id = msg.correlation_id.clone();
                    } else if let Some(corr_id) = msg.headers.get("correlation_id") {
                        reply_msg.correlation_id = corr_id.clone();
                    }

                    // Ensure reply message ID has "res-" prefix
                    // Note: Message IDs are managed by send_reply() in actor_context.rs, but we ensure prefix here for consistency
                    if reply_msg.id.is_empty()
                        || (!reply_msg.id.starts_with("res-") && !reply_msg.id.starts_with("req-"))
                    {
                        // Generate new ID with res- prefix if empty or missing prefix
                        // Use timestamp-based ID generation since ulid may not be available
                        use std::time::{SystemTime, UNIX_EPOCH};
                        let timestamp = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_nanos();
                        reply_msg.id = format!("res-{:x}", timestamp);
                    }

                    // Use ActorService::send() to send reply (handles local/remote automatically)
                    // ActorService::send() will route via ActorRef::tell() which handles temporary sender IDs and correlation_id routing
                    let actor_service =
                        ctx.service_locator
                            .get_actor_service()
                            .await
                            .ok_or_else(|| {
                                BehaviorError::ProcessingError(
                                    "ActorService not available in ServiceLocator".to_string(),
                                )
                            })?;

                    // Set receiver_id to sender_id (the actor that called ask())
                    reply_msg.receiver_id = target_id.to_string();
                    // Set sender_id to this actor's ID
                    reply_msg.sender_id = msg.receiver_id.clone();

                    let reply_ctx = crate::core::RequestContext::new_without_auth(
                        ctx.tenant_id.clone(),
                        ctx.namespace.clone(),
                    );
                    actor_service.send(&reply_ctx, target_id.as_ref(), reply_msg).await
                        .map_err(|e| {
                        metrics::counter!("plexspaces_behavior_workflow_reply_errors_total", "behavior" => "workflow", "error" => "send_failed", "type" => "query").increment(1);
                        tracing::error!(error = %e, query_name = %name, "Failed to send workflow query reply");
                        BehaviorError::ProcessingError(format!("Failed to send workflow query reply: {}", e))
                    })?;
                    metrics::counter!("plexspaces_behavior_workflow_replies_sent_total", "behavior" => "workflow", "type" => "query").increment(1);
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(target_id = %target_id, query_name = %name, "Workflow query reply sent");
                    }
                } else {
                    metrics::counter!("plexspaces_behavior_workflow_reply_errors_total", "behavior" => "workflow", "error" => "no_sender", "type" => "query").increment(1);
                    tracing::warn!(query_name = %name, "Workflow query has no reply_to or sender_id, reply not sent");
                }

                Ok(())
            }
            _ => Err(BehaviorError::UnsupportedMessage),
        }
    }
}

/// Message types for routing
/// These correspond to different handling patterns in Erlang/OTP:
/// - Call: Synchronous request expecting a reply (gen_server:call)
/// - Cast: Asynchronous message with no reply expected (gen_server:cast)
/// - Info: System or timeout messages (handle_info)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    /// Synchronous call (expects reply)
    Call,
    /// Asynchronous cast (fire-and-forget)
    Cast,
    /// System info message
    Info,
    /// Workflow run command
    WorkflowRun,
    /// Workflow signal
    WorkflowSignal(String),
    /// Workflow query
    WorkflowQuery(String),
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageType::Call => write!(f, "call"),
            MessageType::Cast => write!(f, "cast"),
            MessageType::Info => write!(f, "info"),
            MessageType::WorkflowRun => write!(f, "workflow_run"),
            MessageType::WorkflowSignal(name) => write!(f, "workflow_signal:{}", name),
            MessageType::WorkflowQuery(name) => write!(f, "workflow_query:{}", name),
        }
    }
}

/// Extension trait to add message_type() method to mailbox::Message
pub trait MessageTypeExt {
    /// Get message type for routing
    fn message_type(&self) -> MessageType;
}

impl MessageTypeExt for Message {
    fn message_type(&self) -> MessageType {
        // Extract from message_type field or headers
        let msg_type = if !self.message_type.is_empty() {
            self.message_type.as_str()
        } else if let Some(mt) = self.headers.get("message_type") {
            mt.as_str()
        } else {
            "info"
        };

        match msg_type {
            "call" => MessageType::Call,
            "cast" => MessageType::Cast,
            "info" => MessageType::Info,
            "workflow_run" => MessageType::WorkflowRun,
            s if s.starts_with("workflow_signal:") => {
                let name = s.strip_prefix("workflow_signal:").unwrap();
                MessageType::WorkflowSignal(name.to_string())
            }
            s if s.starts_with("workflow_query:") => {
                let name = s.strip_prefix("workflow_query:").unwrap();
                MessageType::WorkflowQuery(name.to_string())
            }
            _ => MessageType::Cast,
        }
    }
}

// BehaviorError is now imported from core crate (see imports at top of file)

/// Mock behavior for testing
pub struct MockBehavior {
    /// List of all messages received by this behavior
    pub messages_received: Vec<Message>,
}

impl Default for MockBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBehavior {
    /// Creates a new MockBehavior instance
    pub fn new() -> Self {
        MockBehavior {
            messages_received: Vec::new(),
        }
    }
}

#[async_trait]
impl Actor for MockBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &crate::core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.messages_received.push(msg);
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("Mock".to_string())
    }
}

// MockReply removed - Reply trait no longer exists, use ActorContext::send_reply() instead

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_behavior_type_equality() {
        assert_eq!(BehaviorType::GenServer, BehaviorType::GenServer);
        assert_ne!(BehaviorType::GenServer, BehaviorType::GenEvent);

        let custom1 = BehaviorType::Custom("test".to_string());
        let custom2 = BehaviorType::Custom("test".to_string());
        assert_eq!(custom1, custom2);
    }

    #[tokio::test]
    async fn test_gen_event_behavior() {
        let mut behavior = GenEventBehavior::new();
        assert_eq!(behavior.behavior_type(), BehaviorType::GenEvent);

        // Test with no handlers (should succeed) - Go-style: context first, then message
        use crate::core::ActorContext;
        use std::sync::Arc;
        let service_locator: Arc<dyn crate::core::ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new());
        let ctx = Arc::new(ActorContext::new(
            "test-node".to_string(),
            String::new(),         // tenant_id (empty if auth disabled)
            "test-ns".to_string(), // namespace
            service_locator,
            None,
        ));
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: vec![],
            ..Default::default()
        };
        behavior.handle_message(&*ctx, msg).await.unwrap();
    }
}
