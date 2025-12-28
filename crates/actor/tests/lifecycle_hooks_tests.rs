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

//! Tests for Actor lifecycle hooks (TDD)
//!
//! Tests for init(), terminate(), and handle_exit() lifecycle hooks

use plexspaces_actor::Actor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorError, ActorId, BehaviorError, ExitAction, ExitReason, Message};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use async_trait::async_trait;
use ulid::Ulid;

/// Test actor that tracks lifecycle hook calls
struct TestLifecycleActor {
    init_called: Arc<std::sync::Mutex<bool>>,
    init_result: Result<(), String>,
    terminate_called: Arc<std::sync::Mutex<bool>>,
    terminate_reason: Arc<std::sync::Mutex<Option<ExitReason>>>,
    handle_exit_called: Arc<std::sync::Mutex<bool>>,
    handle_exit_action: Arc<std::sync::Mutex<Option<ExitAction>>>,
}

impl TestLifecycleActor {
    fn new() -> Self {
        Self {
            init_called: Arc::new(std::sync::Mutex::new(false)),
            init_result: Ok(()),
            terminate_called: Arc::new(std::sync::Mutex::new(false)),
            terminate_reason: Arc::new(std::sync::Mutex::new(None)),
            handle_exit_called: Arc::new(std::sync::Mutex::new(false)),
            handle_exit_action: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn with_init_error(mut self, error: String) -> Self {
        self.init_result = Err(error);
        self
    }

    fn init_was_called(&self) -> bool {
        *self.init_called.lock().unwrap()
    }

    fn terminate_was_called(&self) -> bool {
        *self.terminate_called.lock().unwrap()
    }

    fn terminate_reason(&self) -> Option<ExitReason> {
        self.terminate_reason.lock().unwrap().clone()
    }

    fn handle_exit_was_called(&self) -> bool {
        *self.handle_exit_called.lock().unwrap()
    }

    fn handle_exit_action(&self) -> Option<ExitAction> {
        self.handle_exit_action.lock().unwrap().clone()
    }
}

#[async_trait]
impl plexspaces_core::Actor for TestLifecycleActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        *self.init_called.lock().unwrap() = true;
        self.init_result.clone().map_err(|e| ActorError::InvalidState(e))
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
        reason: &ExitReason,
    ) -> Result<(), ActorError> {
        *self.terminate_called.lock().unwrap() = true;
        *self.terminate_reason.lock().unwrap() = Some(reason.clone());
        Ok(())
    }

    async fn handle_exit(
        &mut self,
        _ctx: &ActorContext,
        _from: &ActorId,
        _reason: &ExitReason,
    ) -> Result<ExitAction, ActorError> {
        *self.handle_exit_called.lock().unwrap() = true;
        let action = ExitAction::Handle; // Default to Handle for testing
        *self.handle_exit_action.lock().unwrap() = Some(action.clone());
        Ok(action)
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_init_called_before_message_loop() {
    // Test that init() is called before actor enters message loop
    let actor_impl = TestLifecycleActor::new();
    let init_called = actor_impl.init_called.clone();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    // Start actor - init() should be called
    let handle = actor.start().await.expect("Actor should start");
    
    // Give it a moment to call init()
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Verify init was called
    assert!(*init_called.lock().unwrap(), "init() should be called before message loop");
    
    // Cleanup
    let _ = actor.stop().await;
    let _ = handle.await;
}

#[tokio::test]
async fn test_init_failure_prevents_start() {
    // Test that if init() fails, actor doesn't start
    let actor_impl = TestLifecycleActor::new().with_init_error("init failed".to_string());
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    // Start should fail
    let result = actor.start().await;
    assert!(result.is_err(), "Actor start should fail if init() fails");
    
    if let Err(e) = result {
        assert!(e.to_string().contains("init failed"), "Error should contain init failure message");
    }
}

#[tokio::test]
async fn test_terminate_called_on_stop() {
    // Test that terminate() is called when actor stops
    let actor_impl = TestLifecycleActor::new();
    let terminate_called = actor_impl.terminate_called.clone();
    let terminate_reason = actor_impl.terminate_reason.clone();
    
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new())).await.unwrap();
    let mut actor = Actor::new(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
        None,
    );

    // Start actor
    let handle = actor.start().await.expect("Actor should start");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Stop actor - terminate() should be called
    actor.stop().await.expect("Actor should stop");
    
    // Verify terminate was called with Shutdown reason
    assert!(*terminate_called.lock().unwrap(), "terminate() should be called on stop");
    assert_eq!(
        terminate_reason.lock().unwrap().as_ref(),
        Some(&ExitReason::Shutdown),
        "terminate() should be called with Shutdown reason"
    );
    
    // Cleanup
    let _ = handle.await;
}

#[tokio::test]
async fn test_handle_exit_called_when_linked_actor_dies() {
    // Test that handle_exit() is called when linked actor dies (if trap_exit=true)
    // This test will be expanded once link/monitor implementation is complete
    // For now, we just verify the method exists and can be called
    
    let actor_impl = TestLifecycleActor::new();
    let handle_exit_called = actor_impl.handle_exit_called.clone();
    let handle_exit_action = actor_impl.handle_exit_action.clone();
    
    // Create actor context with trap_exit=true
    use plexspaces_core::ServiceLocator;
    let service_locator = Arc::new(ServiceLocator::new());
    let mut ctx = ActorContext::new(
        "node-1".to_string(),
        "tenant".to_string(),
        "namespace".to_string(),
        service_locator,
        None,
    );
    ctx.trap_exit = true;
    let ctx = Arc::new(ctx);
    
    // Call handle_exit directly (simulating linked actor death)
    let mut actor_impl_test = TestLifecycleActor::new();
    let handle_exit_called_test = actor_impl_test.handle_exit_called.clone();
    let handle_exit_action_test = actor_impl_test.handle_exit_action.clone();
    let from = ActorId::from("linked-actor");
    let reason = ExitReason::Error("linked actor crashed".to_string());
    
    let result = ActorTrait::handle_exit(&mut actor_impl_test, &ctx, &from, &reason).await;
    assert!(result.is_ok(), "handle_exit() should succeed");
    assert_eq!(result.unwrap(), ExitAction::Handle, "handle_exit() should return Handle action");
    
    // Verify handle_exit was called
    assert!(*handle_exit_called_test.lock().unwrap(), "handle_exit() should be called");
    assert_eq!(
        handle_exit_action_test.lock().unwrap().as_ref(),
        Some(&ExitAction::Handle),
        "handle_exit() should set action"
    );
}




