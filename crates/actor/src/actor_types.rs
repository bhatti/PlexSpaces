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

//! Core actor types: Actor trait, BehaviorContext, BehaviorType, BehaviorError, ActorError.
//!
//! These were previously defined inline in `plexspaces_actor::lib.rs`. Moved here during
//! Phase 9 (merge core into actor).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::actor_context::ActorContext;
use crate::actor_id::ActorId;
use crate::exit_reason::{ExitAction, ExitReason};
use crate::journal_storage::JournalError;
use plexspaces_proto::common::v1::Message;
use plexspaces_service_traits::ActorRef;

/// Context passed to behavior when handling a message
pub struct BehaviorContext {
    /// Reference to actor context
    pub actor_context: Arc<ActorContext>,
    /// Current message being processed
    pub message: Message,
    /// Sender of the message
    pub sender: Option<ActorRef>,
    /// Correlation ID for distributed tracing
    pub correlation_id: Option<String>,
}

/// Actor trait - what you implement to create an actor
#[async_trait]
pub trait Actor: Send + Sync {
    /// Initialize actor before entering message loop
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Ok(())
    }

    /// Handle an incoming message
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError>;

    /// Handle EXIT from linked actor (only if trap_exit = true)
    async fn handle_exit(
        &mut self,
        _ctx: &ActorContext,
        _from: &ActorId,
        _reason: &ExitReason,
    ) -> Result<ExitAction, ActorError> {
        Ok(ExitAction::Propagate)
    }

    /// Cleanup before actor stops
    async fn terminate(
        &mut self,
        _ctx: &ActorContext,
        _reason: &ExitReason,
    ) -> Result<(), ActorError> {
        Ok(())
    }

    /// Capture actor state for durability checkpointing.
    async fn capture_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
    ) -> Result<Option<Vec<u8>>, ActorError> {
        Ok(None)
    }

    /// Restore actor state from a serialized checkpoint.
    async fn restore_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
        _state_data: &[u8],
    ) -> Result<bool, ActorError> {
        Ok(false)
    }

    /// Called after all facets are attached and initialized.
    async fn on_facets_ready(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Ok(())
    }

    /// Called before facets are detached.
    async fn on_facets_detaching(
        &mut self,
        _ctx: &ActorContext,
        _reason: &ExitReason,
    ) -> Result<(), ActorError> {
        Ok(())
    }

    /// Get the behavior type.
    fn behavior_type(&self) -> BehaviorType;

    /// Get the OTP-style behavior kind for logging.
    fn behavior_kind(&self) -> BehaviorType {
        self.behavior_type()
    }
}

/// Types of behaviors (OTP-inspired)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BehaviorType {
    /// GenServer-like request/reply behavior
    GenServer,
    /// GenEvent-like event handling
    GenEvent,
    /// GenStateMachine-like FSM
    GenStateMachine,
    /// Workflow-like orchestration
    Workflow,
    /// Custom behavior type
    Custom(String),
}

impl BehaviorType {
    /// Canonical `actor_type` segment for ActorId and BehaviorRegistry keys.
    pub fn actor_type_slug(&self) -> std::borrow::Cow<'_, str> {
        use std::borrow::Cow;
        match self {
            BehaviorType::GenServer => Cow::Borrowed("gen_server"),
            BehaviorType::GenEvent => Cow::Borrowed("gen_event"),
            BehaviorType::GenStateMachine => Cow::Borrowed("gen_state_machine"),
            BehaviorType::Workflow => Cow::Borrowed("workflow"),
            BehaviorType::Custom(s) => Cow::Borrowed(s.as_str()),
        }
    }
}

/// Behavior errors
#[derive(Debug, thiserror::Error)]
pub enum BehaviorError {
    /// Handler not found
    #[error("Handler not found: {0}")]
    HandlerNotFound(String),
    /// Unsupported message type
    #[error("Unsupported message type")]
    UnsupportedMessage,
    /// State transition failed
    #[error("State transition failed: {0}")]
    TransitionFailed(String),
    /// Processing error
    #[error("Processing error: {0}")]
    ProcessingError(String),
}

impl BehaviorError {
    /// Returns the proto error code corresponding to this error variant.
    pub fn code(&self) -> plexspaces_proto::actor::v1::BehaviorErrorCode {
        use plexspaces_proto::actor::v1::BehaviorErrorCode;
        match self {
            BehaviorError::HandlerNotFound(_) => BehaviorErrorCode::BehaviorErrorHandlerNotFound,
            BehaviorError::UnsupportedMessage => BehaviorErrorCode::BehaviorErrorUnsupportedMessage,
            BehaviorError::TransitionFailed(_) => BehaviorErrorCode::BehaviorErrorTransitionFailed,
            BehaviorError::ProcessingError(_) => BehaviorErrorCode::BehaviorErrorProcessingError,
        }
    }
}

/// Actor errors
#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    /// Mailbox error
    #[error("Mailbox error: {0}")]
    MailboxError(String),
    /// Behavior error
    #[error("Behavior error: {0}")]
    BehaviorError(String),
    /// Journal error
    #[error("Journal error: {0}")]
    JournalError(String),
    /// Actor not found
    #[error("Actor not found: {0}. Hint: Verify the actor ID is correct and the actor has been spawned.")]
    NotFound(String),
    /// Actor already exists
    #[error("Actor already exists: {0}")]
    AlreadyExists(String),
    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),
    /// No behavior to restore
    #[error("No behavior to restore")]
    NoBehaviorToRestore,
    /// Request timeout
    #[error("Request timeout. Hint: The actor may be overloaded or unreachable. Consider increasing the timeout or checking actor health.")]
    Timeout,
    /// Actor terminated
    #[error("Actor terminated")]
    ActorTerminated,
    /// Facet error
    #[error("Facet error: {0}")]
    FacetError(String),
}

impl ActorError {
    /// Returns the proto error code corresponding to this error variant.
    pub fn code(&self) -> plexspaces_proto::actor::v1::ActorErrorCode {
        use plexspaces_proto::actor::v1::ActorErrorCode;
        match self {
            ActorError::MailboxError(_) => ActorErrorCode::ActorErrorMailboxError,
            ActorError::BehaviorError(_) => ActorErrorCode::ActorErrorBehaviorError,
            ActorError::JournalError(_) => ActorErrorCode::ActorErrorJournalError,
            ActorError::NotFound(_) => ActorErrorCode::ActorErrorNotFound,
            ActorError::AlreadyExists(_) => ActorErrorCode::ActorErrorAlreadyExists,
            ActorError::InvalidState(_) => ActorErrorCode::ActorErrorInvalidState,
            ActorError::NoBehaviorToRestore => ActorErrorCode::ActorErrorInvalidState,
            ActorError::Timeout => ActorErrorCode::ActorErrorTimeout,
            ActorError::ActorTerminated => ActorErrorCode::ActorErrorTerminated,
            ActorError::FacetError(_) => ActorErrorCode::ActorErrorFacetError,
        }
    }
}

impl From<JournalError> for ActorError {
    fn from(e: JournalError) -> Self {
        ActorError::JournalError(e.to_string())
    }
}
