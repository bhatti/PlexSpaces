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

//! Edge case tests for Actor lifecycle hooks (production-grade coverage)
//!
//! Tests cover:
//! - Concurrent init() calls
//! - Multiple terminate() calls
//! - Error handling edge cases
//! - State transition edge cases
//! - Resource cleanup edge cases

use plexspaces_actor::Actor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorError, ActorId, BehaviorError, ExitAction, ExitReason, Message};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use async_trait::async_trait;
use tokio::time::{sleep, Duration};
use ulid::Ulid;

/// Test actor for edge cases
struct EdgeCaseActor {
    init_called: Arc<AtomicBool>,
    terminate_called: Arc<AtomicBool>,
    terminate_count: Arc<AtomicU32>,
}

impl EdgeCaseActor {
    fn new() -> Self {
        Self {
            init_called: Arc::new(AtomicBool::new(false)),
            terminate_called: Arc::new(AtomicBool::new(false)),
            terminate_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl plexspaces_core::Actor for EdgeCaseActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        // Simulate init that might be called multiple times (shouldn't happen, but test it)
        if self.init_called.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // Already called - this shouldn't happen in practice
            return Err(ActorError::InvalidState("init() called multiple times".to_string()));
        }
        Ok(())
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    async fn terminate(
        &mut self,
        _ctx: &ActorContext,
        _reason: &ExitReason,
    ) -> Result<(), ActorError> {
        self.terminate_called.store(true, Ordering::SeqCst);
        self.terminate_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_stop_idempotent_multiple_calls() {
    // Test that stop() can be called multiple times safely
    let actor_impl = EdgeCaseActor::new();
    let terminate_count = actor_impl.terminate_count.clone();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;
    
    // Call stop() multiple times
    let _ = actor.stop().await;
    let _ = actor.stop().await;
    let _ = actor.stop().await;
    
    // terminate() should only be called once
    assert_eq!(terminate_count.load(Ordering::SeqCst), 1, "terminate() should be called exactly once even with multiple stop() calls");
    
    let _ = handle.await;
}

#[tokio::test]
async fn test_stop_when_already_stopped() {
    // Test that stop() is safe when actor is already stopped
    let actor_impl = EdgeCaseActor::new();
    let terminate_count = actor_impl.terminate_count.clone();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;
    
    // Stop actor
    let _ = actor.stop().await;
    let _ = handle.await;
    
    // Try to stop again (should be safe)
    let _ = actor.stop().await;
    
    // terminate() should still only be called once
    assert_eq!(terminate_count.load(Ordering::SeqCst), 1, "terminate() should not be called again on second stop()");
}

#[tokio::test]
async fn test_stop_when_not_started() {
    // Test that stop() is safe when actor was never started
    let actor_impl = EdgeCaseActor::new();
    let terminate_count = actor_impl.terminate_count.clone();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    // Stop without starting (should be safe)
    let _ = actor.stop().await;
    
    // terminate() should not be called (actor never started)
    assert_eq!(terminate_count.load(Ordering::SeqCst), 0, "terminate() should not be called if actor never started");
}

#[tokio::test]
async fn test_start_after_stop_fails() {
    // Test that starting an actor after it's been stopped fails
    let actor_impl = EdgeCaseActor::new();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;
    
    // Stop actor
    let _ = actor.stop().await;
    let _ = handle.await;
    
    // Try to start again (should fail)
    let result = actor.start().await;
    assert!(result.is_err(), "Starting actor after stop should fail");
}

#[tokio::test]
async fn test_init_error_properly_handled() {
    // Test that init() errors are properly handled and actor doesn't start
    struct FailingInitActor;
    
    #[async_trait]
    impl plexspaces_core::Actor for FailingInitActor {
        async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
            Err(ActorError::InvalidState("init failed: database connection error".to_string()))
        }

        async fn handle_message(
            &mut self,
            _ctx: &ActorContext,
            _msg: Message,
        ) -> Result<(), BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(FailingInitActor),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    // Start should fail
    let result = actor.start().await;
    assert!(result.is_err(), "Actor start should fail if init() fails");
    
    // Actor should be in Failed state
    let state = actor.state().await;
    assert!(matches!(state, plexspaces_actor::ActorState::Failed(_)), "Actor should be in Failed state after init() failure");
}

#[tokio::test]
async fn test_terminate_error_doesnt_prevent_shutdown() {
    // Test that terminate() errors don't prevent shutdown
    struct FailingTerminateActor;
    
    #[async_trait]
    impl plexspaces_core::Actor for FailingTerminateActor {
        async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
            Ok(())
        }

        async fn handle_message(
            &mut self,
            _ctx: &ActorContext,
            _msg: Message,
        ) -> Result<(), BehaviorError> {
            Ok(())
        }

        async fn terminate(
            &mut self,
            _ctx: &ActorContext,
            _reason: &ExitReason,
        ) -> Result<(), ActorError> {
            Err(ActorError::InvalidState("terminate failed".to_string()))
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(FailingTerminateActor),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;
    
    // Stop should succeed even if terminate() fails
    let result = actor.stop().await;
    assert!(result.is_ok(), "stop() should succeed even if terminate() fails");
    
    let _ = handle.await;
}




