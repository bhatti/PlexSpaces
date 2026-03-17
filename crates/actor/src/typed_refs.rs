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

//! High-level typed actor references for OTP-style behaviors
//!
//! ## Purpose
//! Provides typed wrappers around [`ActorRef`] for each behavior pattern:
//! - [`WorkflowRef`]: Durable workflow operations (run/signal/query)
//! - [`GenServerRef`]: Request-reply (call/cast) operations
//! - [`FsmRef`]: State machine transitions and queries
//! - [`EventRef`]: Fire-and-forget event emission
//!
//! ## Design Philosophy
//! These wrappers eliminate manual [`Message`] construction and provide:
//! - **Typed API**: Generic methods with automatic serialization
//! - **Clean Syntax**: `server.call("op", &req)` instead of manual Message
//! - **Per-operation timeouts**: Each method has sensible defaults with `*_with_timeout` variants
//! - **Consistent Error Handling**: Each ref type has its own error enum
//!
//! ## Example
//! ```ignore
//! use plexspaces_actor::{WorkflowRef, GenServerRef, FsmRef, EventRef};
//!
//! // Workflow: durable execution
//! let result: Output = workflow.run(&input).await?;
//! workflow.signal("approve", &data).await?;
//! let status: Status = workflow.query("status").await?;
//!
//! // GenServer: request-reply
//! let result: Response = server.call("process", &request).await?;
//! server.cast("log", &data).await?;
//!
//! // FSM: state transitions
//! fsm.transition("submit", &order).await?;
//! let state: State = fsm.query_state().await?;
//!
//! // Event: fire-and-forget
//! logger.emit("user_login", &event).await?;
//! ```

use crate::ActorRef;
use plexspaces_core::Message;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;

// ============================================================================
// Default timeouts
// ============================================================================

/// Default timeout for GenServer call operations (30 seconds).
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for FSM operations (30 seconds).
pub const DEFAULT_FSM_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for workflow operations (30 seconds).
///
/// Following Temporal's approach where:
/// - **Workflow lifetime**: Can be days/months/years (no timeout on the handle itself)
/// - **Operation timeout**: Per-call timeout with sensible defaults
pub const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Extended timeout for long-running workflow executions (5 minutes).
///
/// Reasonable default for workflow runs that may involve multiple steps,
/// external service calls, or human-in-the-loop approvals.
/// For workflows that may take hours/days, use `run_with_timeout()`.
pub const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_secs(300);

// ============================================================================
// Error types
// ============================================================================

/// Error type for GenServer operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum GenServerError {
    /// Serialization/deserialization error
    #[error("GenServer serialization error: {0}")]
    Serialization(String),

    /// Call (request-reply) error
    #[error("GenServer call error: {0}")]
    Call(String),

    /// Cast (fire-and-forget) error
    #[error("GenServer cast error: {0}")]
    Cast(String),

    /// Spawn error
    #[error("GenServer spawn error: {0}")]
    Spawn(String),
}

/// Error type for FSM operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FsmError {
    /// Serialization/deserialization error
    #[error("FSM serialization error: {0}")]
    Serialization(String),

    /// State transition error
    #[error("FSM transition error: {0}")]
    Transition(String),

    /// Query error
    #[error("FSM query error: {0}")]
    Query(String),

    /// Spawn error
    #[error("FSM spawn error: {0}")]
    Spawn(String),
}

/// Error type for event operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventError {
    /// Serialization error
    #[error("Event serialization error: {0}")]
    Serialization(String),

    /// Event emission error
    #[error("Event emit error: {0}")]
    Emit(String),

    /// Spawn error
    #[error("Event spawn error: {0}")]
    Spawn(String),
}

/// Error type for workflow operations.
///
/// Provides detailed error categorization for workflow execution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkflowRefError {
    /// Serialization/deserialization error
    #[error("Workflow serialization error: {0}")]
    Serialization(String),

    /// Workflow execution error
    #[error("Workflow execution error: {0}")]
    Execution(String),

    /// Spawn error
    #[error("Workflow spawn error: {0}")]
    Spawn(String),
}

// ============================================================================
// GenServerRef - High-level GenServer actor handle
// ============================================================================

/// High-level GenServer actor handle with typed call/cast methods.
///
/// Provides a clean API for request-reply (call) and fire-and-forget (cast)
/// operations on GenServer actors. Eliminates manual Message construction.
///
/// ## Design Philosophy
/// - **Typed API**: Generic methods with automatic serialization
/// - **Clean Syntax**: `server.call("op", &req)` instead of manual Message
/// - **Per-operation timeouts**: Each method has sensible defaults
///
/// ## Example
/// ```ignore
/// use plexspaces_actor::GenServerRef;
///
/// // Create from ActorRef
/// let server = GenServerRef::new(actor_ref);
///
/// // Request-reply (call)
/// let result: Response = server.call("process", &request).await?;
///
/// // Fire-and-forget (cast)
/// server.cast("log", &log_data).await?;
/// ```
pub struct GenServerRef {
    inner: ActorRef,
}

impl GenServerRef {
    /// Create a new GenServerRef from an ActorRef.
    pub fn new(actor_ref: ActorRef) -> Self {
        Self { inner: actor_ref }
    }

    /// Get the actor ID.
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Get the underlying ActorRef for advanced use cases.
    pub fn actor_ref(&self) -> &ActorRef {
        &self.inner
    }

    /// Call the GenServer (request-reply).
    ///
    /// Sends a request and waits for a response. Uses `DEFAULT_CALL_TIMEOUT`.
    ///
    /// ## Example
    /// ```ignore
    /// let result: ExtractResult = extractor.call("extract", &document).await?;
    /// ```
    pub async fn call<I, O>(&self, operation: &str, request: &I) -> Result<O, GenServerError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        self.call_with_timeout(operation, request, DEFAULT_CALL_TIMEOUT)
            .await
    }

    /// Call the GenServer with custom timeout.
    pub async fn call_with_timeout<I, O>(
        &self,
        operation: &str,
        request: &I,
        timeout: Duration,
    ) -> Result<O, GenServerError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: operation.to_string(),
            payload: serde_json::to_vec(request)
                .map_err(|e| GenServerError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| GenServerError::Call(e.to_string()))?;

        serde_json::from_slice(&response.payload).map_err(|e| {
            GenServerError::Serialization(format!("Failed to deserialize response: {}", e))
        })
    }

    /// Cast to the GenServer (fire-and-forget).
    ///
    /// Sends a message without waiting for a response.
    ///
    /// ## Example
    /// ```ignore
    /// server.cast("update_config", &new_config).await?;
    /// ```
    pub async fn cast<T: Serialize>(
        &self,
        operation: &str,
        data: &T,
    ) -> Result<(), GenServerError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: operation.to_string(),
            payload: serde_json::to_vec(data)
                .map_err(|e| GenServerError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        self.inner
            .tell(msg)
            .await
            .map_err(|e| GenServerError::Cast(e.to_string()))
    }
}

// ============================================================================
// FsmRef - High-level FSM actor handle
// ============================================================================

/// High-level FSM actor handle with typed transition/query methods.
///
/// Provides a clean API for state machine operations. Eliminates manual
/// Message construction for FSM events and queries.
///
/// ## Design Philosophy
/// - **Typed transitions**: `fsm.transition("event", &data)` instead of manual Message
/// - **State queries**: `fsm.query_state()` for reading FSM state
/// - **Per-operation timeouts**: Each method has sensible defaults
///
/// ## Example
/// ```ignore
/// use plexspaces_actor::FsmRef;
///
/// // Create from ActorRef
/// let fsm = FsmRef::new(actor_ref);
///
/// // Send transition events
/// fsm.transition("submit", &order_data).await?;
/// fsm.transition("pay", &payment_info).await?;
///
/// // Query current state
/// let state: OrderState = fsm.query_state().await?;
/// ```
pub struct FsmRef {
    inner: ActorRef,
}

impl FsmRef {
    /// Create a new FsmRef from an ActorRef.
    pub fn new(actor_ref: ActorRef) -> Self {
        Self { inner: actor_ref }
    }

    /// Get the FSM actor ID.
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Get the underlying ActorRef for advanced use cases.
    pub fn actor_ref(&self) -> &ActorRef {
        &self.inner
    }

    /// Send a transition event to the FSM (fire-and-forget).
    ///
    /// The FSM will process the event and transition to a new state
    /// based on its transition function.
    ///
    /// ## Example
    /// ```ignore
    /// fsm.transition("submit", &SubmitOrder { items: vec![...] }).await?;
    /// ```
    pub async fn transition<T: Serialize>(&self, event: &str, data: &T) -> Result<(), FsmError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("fsm_event:{}", event),
            payload: serde_json::to_vec(data)
                .map_err(|e| FsmError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        self.inner
            .tell(msg)
            .await
            .map_err(|e| FsmError::Transition(e.to_string()))
    }

    /// Send a transition event and wait for acknowledgment.
    ///
    /// Unlike `transition()`, this waits for the FSM to process the event.
    /// Returns the new state after the transition.
    ///
    /// ## Example
    /// ```ignore
    /// let new_state: OrderState = fsm.transition_and_wait("submit", &order).await?;
    /// ```
    pub async fn transition_and_wait<T, S>(&self, event: &str, data: &T) -> Result<S, FsmError>
    where
        T: Serialize,
        S: DeserializeOwned,
    {
        self.transition_and_wait_with_timeout(event, data, DEFAULT_FSM_TIMEOUT)
            .await
    }

    /// Send a transition event and wait with custom timeout.
    pub async fn transition_and_wait_with_timeout<T, S>(
        &self,
        event: &str,
        data: &T,
        timeout: Duration,
    ) -> Result<S, FsmError>
    where
        T: Serialize,
        S: DeserializeOwned,
    {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("fsm_event:{}", event),
            payload: serde_json::to_vec(data)
                .map_err(|e| FsmError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| FsmError::Transition(e.to_string()))?;

        serde_json::from_slice(&response.payload)
            .map_err(|e| FsmError::Serialization(format!("Failed to deserialize state: {}", e)))
    }

    /// Query the current FSM state.
    ///
    /// ## Example
    /// ```ignore
    /// let state: OrderState = fsm.query_state().await?;
    /// ```
    pub async fn query_state<S: DeserializeOwned>(&self) -> Result<S, FsmError> {
        self.query_state_with_timeout(DEFAULT_FSM_TIMEOUT).await
    }

    /// Query the current FSM state with custom timeout.
    pub async fn query_state_with_timeout<S: DeserializeOwned>(
        &self,
        timeout: Duration,
    ) -> Result<S, FsmError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: "fsm_query:state".to_string(),
            payload: vec![],
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| FsmError::Query(e.to_string()))?;

        serde_json::from_slice(&response.payload)
            .map_err(|e| FsmError::Serialization(format!("Failed to deserialize state: {}", e)))
    }

    /// Query FSM with a named query.
    ///
    /// ## Example
    /// ```ignore
    /// let history: Vec<StateChange> = fsm.query("history").await?;
    /// ```
    pub async fn query<O: DeserializeOwned>(&self, name: &str) -> Result<O, FsmError> {
        self.query_with_timeout(name, DEFAULT_FSM_TIMEOUT).await
    }

    /// Query FSM with custom timeout.
    pub async fn query_with_timeout<O: DeserializeOwned>(
        &self,
        name: &str,
        timeout: Duration,
    ) -> Result<O, FsmError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("fsm_query:{}", name),
            payload: vec![],
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| FsmError::Query(e.to_string()))?;

        serde_json::from_slice(&response.payload)
            .map_err(|e| FsmError::Serialization(format!("Failed to deserialize response: {}", e)))
    }
}

// ============================================================================
// EventRef - High-level GenEvent actor handle
// ============================================================================

/// High-level GenEvent actor handle for fire-and-forget event emission.
///
/// Provides a clean API for sending events to GenEvent actors without
/// manual Message construction.
///
/// ## Design Philosophy
/// - **Fire-and-forget**: Events are processed asynchronously
/// - **Typed API**: Automatic serialization of event data
/// - **No timeouts**: Events are sent to mailbox and return immediately
///
/// ## Example
/// ```ignore
/// use plexspaces_actor::EventRef;
///
/// // Create from ActorRef
/// let logger = EventRef::new(actor_ref);
///
/// // Emit events (fire-and-forget)
/// logger.emit("user_login", &LoginEvent { user_id: "alice".into() }).await?;
/// logger.emit("document_created", &DocEvent { doc_id: "doc-123".into() }).await?;
/// ```
pub struct EventRef {
    inner: ActorRef,
}

impl EventRef {
    /// Create a new EventRef from an ActorRef.
    pub fn new(actor_ref: ActorRef) -> Self {
        Self { inner: actor_ref }
    }

    /// Get the event actor ID.
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Get the underlying ActorRef for advanced use cases.
    pub fn actor_ref(&self) -> &ActorRef {
        &self.inner
    }

    /// Emit an event (fire-and-forget).
    ///
    /// Events are processed asynchronously - this method returns immediately
    /// after sending the event to the actor's mailbox.
    ///
    /// ## Example
    /// ```ignore
    /// logger.emit("user_login", &LoginEvent { user_id: "alice".into() }).await?;
    /// ```
    pub async fn emit<T: Serialize>(&self, event_type: &str, data: &T) -> Result<(), EventError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: event_type.to_string(),
            payload: serde_json::to_vec(data)
                .map_err(|e| EventError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        self.inner
            .tell(msg)
            .await
            .map_err(|e| EventError::Emit(e.to_string()))
    }

    /// Emit an event with raw payload.
    ///
    /// Use this when you already have serialized data.
    pub async fn emit_raw(&self, event_type: &str, payload: Vec<u8>) -> Result<(), EventError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: event_type.to_string(),
            payload,
            ..Default::default()
        };

        self.inner
            .tell(msg)
            .await
            .map_err(|e| EventError::Emit(e.to_string()))
    }
}

// ============================================================================
// WorkflowRef - High-level workflow actor handle
// ============================================================================

/// High-level workflow handle with typed signal/query methods.
///
/// Inspired by Temporal's WorkflowHandle and Cloudflare's Durable Object stubs.
/// Eliminates manual Message construction and message_type strings.
///
/// ## Design Philosophy (Temporal-inspired)
/// - **Workflow lifetime**: Workflows can run for days, months, or years
/// - **Operation timeouts**: Per-call timeouts with sensible defaults
/// - **No global timeout**: The handle itself has no timeout - only operations do
///
/// ## Key Features
/// - **Typed API**: Generic methods for run/signal/query with automatic serialization
/// - **Clean Syntax**: `workflow.signal("approve", &data)` instead of manual Message construction
/// - **Per-operation timeouts**: Each method has a `*_with_timeout` variant for custom timeouts
///
/// ## Example
/// ```ignore
/// use plexspaces_actor::WorkflowRef;
///
/// // Create workflow ref from ActorRef
/// let workflow = WorkflowRef::new(actor_ref);
///
/// // Start workflow with typed input (uses DEFAULT_RUN_TIMEOUT)
/// let result: ApprovalResult = workflow.run(&approval_request).await?;
///
/// // Send typed signal (fire-and-forget, no timeout)
/// workflow.signal("approve", &ApprovePayload { approver_id: "alice".into() }).await?;
///
/// // Query workflow state (uses DEFAULT_OPERATION_TIMEOUT)
/// let status: WorkflowStatus = workflow.query("status").await?;
/// ```
pub struct WorkflowRef {
    inner: ActorRef,
}

impl WorkflowRef {
    /// Create a new WorkflowRef from an ActorRef.
    ///
    /// ## Note
    /// The WorkflowRef has no global timeout. Instead, each operation
    /// (run, query, signal_and_wait) has its own timeout with sensible defaults.
    /// Use `*_with_timeout` variants for custom timeouts.
    pub fn new(actor_ref: ActorRef) -> Self {
        Self { inner: actor_ref }
    }

    /// Get the workflow ID.
    pub fn id(&self) -> &str {
        self.inner.id()
    }

    /// Get the underlying ActorRef for advanced use cases.
    pub fn actor_ref(&self) -> &ActorRef {
        &self.inner
    }

    /// Run the workflow with typed input and output.
    ///
    /// Uses `DEFAULT_RUN_TIMEOUT` (5 minutes). For longer workflows,
    /// use `run_with_timeout()`.
    ///
    /// This is the main entry point - equivalent to Temporal's workflow execution.
    /// The workflow's `run()` method will be invoked.
    ///
    /// ## Example
    /// ```ignore
    /// let result: ApprovalResult = workflow.run(&ApprovalRequest {
    ///     document_id: "doc-123".into(),
    ///     approvers: vec!["alice", "bob"],
    /// }).await?;
    /// ```
    pub async fn run<I, O>(&self, input: &I) -> Result<O, WorkflowRefError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        self.run_with_timeout(input, DEFAULT_RUN_TIMEOUT).await
    }

    /// Run the workflow with a custom timeout.
    ///
    /// Use this for long-running workflows that may take hours or days.
    ///
    /// ## Example
    /// ```ignore
    /// let result: BatchResult = workflow.run_with_timeout(
    ///     &BatchRequest { items: large_dataset },
    ///     Duration::from_secs(3600), // 1 hour timeout
    /// ).await?;
    /// ```
    pub async fn run_with_timeout<I, O>(
        &self,
        input: &I,
        timeout: Duration,
    ) -> Result<O, WorkflowRefError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: "workflow_run".to_string(),
            payload: serde_json::to_vec(input)
                .map_err(|e| WorkflowRefError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| WorkflowRefError::Execution(e.to_string()))?;

        serde_json::from_slice(&response.payload).map_err(|e| {
            WorkflowRefError::Serialization(format!("Failed to deserialize response: {}", e))
        })
    }

    /// Send a signal to the workflow (fire-and-forget).
    ///
    /// Signals modify workflow state but don't return a value.
    /// This is a fire-and-forget operation with no timeout.
    ///
    /// Equivalent to Temporal's `workflowHandle.signal()`.
    ///
    /// ## Example
    /// ```ignore
    /// workflow.signal("approve", &ApprovePayload {
    ///     approver_id: "alice".into(),
    ///     comment: Some("Looks good!".into()),
    /// }).await?;
    /// ```
    pub async fn signal<T: Serialize>(&self, name: &str, data: &T) -> Result<(), WorkflowRefError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("workflow_signal:{}", name),
            payload: serde_json::to_vec(data)
                .map_err(|e| WorkflowRefError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        self.inner
            .tell(msg)
            .await
            .map_err(|e| WorkflowRefError::Execution(e.to_string()))
    }

    /// Query the workflow state (read-only).
    ///
    /// Uses `DEFAULT_OPERATION_TIMEOUT` (30 seconds). For slow queries,
    /// use `query_with_timeout()`.
    ///
    /// Queries don't modify state and return a typed result.
    /// Equivalent to Temporal's `workflowHandle.query()`.
    ///
    /// ## Example
    /// ```ignore
    /// let status: WorkflowStatus = workflow.query("status").await?;
    /// ```
    pub async fn query<O: DeserializeOwned>(&self, name: &str) -> Result<O, WorkflowRefError> {
        self.query_with_params_and_timeout(name, &(), DEFAULT_OPERATION_TIMEOUT)
            .await
    }

    /// Query the workflow state with a custom timeout.
    pub async fn query_with_timeout<O: DeserializeOwned>(
        &self,
        name: &str,
        timeout: Duration,
    ) -> Result<O, WorkflowRefError> {
        self.query_with_params_and_timeout(name, &(), timeout).await
    }

    /// Query the workflow state with parameters.
    ///
    /// Uses `DEFAULT_OPERATION_TIMEOUT` (30 seconds).
    pub async fn query_with_params<I, O>(
        &self,
        name: &str,
        params: &I,
    ) -> Result<O, WorkflowRefError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        self.query_with_params_and_timeout(name, params, DEFAULT_OPERATION_TIMEOUT)
            .await
    }

    /// Query the workflow state with parameters and custom timeout.
    pub async fn query_with_params_and_timeout<I, O>(
        &self,
        name: &str,
        params: &I,
        timeout: Duration,
    ) -> Result<O, WorkflowRefError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("workflow_query:{}", name),
            payload: serde_json::to_vec(params)
                .map_err(|e| WorkflowRefError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        let response = self
            .inner
            .ask(msg, timeout)
            .await
            .map_err(|e| WorkflowRefError::Execution(e.to_string()))?;

        serde_json::from_slice(&response.payload).map_err(|e| {
            WorkflowRefError::Serialization(format!("Failed to deserialize response: {}", e))
        })
    }

    /// Signal and wait for acknowledgment.
    ///
    /// Uses `DEFAULT_OPERATION_TIMEOUT` (30 seconds).
    /// Unlike `signal()`, this waits for the signal to be processed.
    pub async fn signal_and_wait<T: Serialize>(
        &self,
        name: &str,
        data: &T,
    ) -> Result<(), WorkflowRefError> {
        self.signal_and_wait_with_timeout(name, data, DEFAULT_OPERATION_TIMEOUT)
            .await
    }

    /// Signal and wait for acknowledgment with custom timeout.
    pub async fn signal_and_wait_with_timeout<T: Serialize>(
        &self,
        name: &str,
        data: &T,
        timeout: Duration,
    ) -> Result<(), WorkflowRefError> {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            receiver_id: self.inner.id().to_string(),
            message_type: format!("workflow_signal:{}", name),
            payload: serde_json::to_vec(data)
                .map_err(|e| WorkflowRefError::Serialization(e.to_string()))?,
            ..Default::default()
        };

        // Use ask instead of tell to wait for processing
        self.inner
            .ask(msg, timeout)
            .await
            .map_err(|e| WorkflowRefError::Execution(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_timeouts() {
        assert_eq!(DEFAULT_CALL_TIMEOUT, Duration::from_secs(30));
        assert_eq!(DEFAULT_FSM_TIMEOUT, Duration::from_secs(30));
        assert_eq!(DEFAULT_OPERATION_TIMEOUT, Duration::from_secs(30));
        assert_eq!(DEFAULT_RUN_TIMEOUT, Duration::from_secs(300));
    }

    #[test]
    fn test_error_display() {
        let gen_err = GenServerError::Call("test".to_string());
        assert!(gen_err.to_string().contains("call error"));

        let fsm_err = FsmError::Transition("test".to_string());
        assert!(fsm_err.to_string().contains("transition error"));

        let event_err = EventError::Emit("test".to_string());
        assert!(event_err.to_string().contains("emit error"));

        let workflow_err = WorkflowRefError::Execution("test".to_string());
        assert!(workflow_err.to_string().contains("execution error"));
    }
}
