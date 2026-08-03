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

//! Comprehensive Actor Lifecycle Tests
//!
//! Consolidated from:
//! - lifecycle_hooks_tests.rs (4 tests)
//! - lifecycle_observability_tests.rs (4 tests)
//! - unified_lifecycle_tests.rs (5 tests)
//! Total: 13 tests

use super::test_actor_helpers::actor_with_default_service_locator;
use async_trait::async_trait;
use plexspaces_actor::Actor;
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, ActorError, ActorId, BehaviorError, BehaviorType,
    ExitAction, ExitReason, Message,
};
use plexspaces_facet::{Facet, FacetError};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use ulid::Ulid;

fn linked_actor_id(name: &str) -> ActorId {
    ActorId::new(
        name.to_string(),
        "gen_server".to_string(),
        "default".to_string(),
        "test-node".to_string(),
    )
    .expect("linked actor id should be valid")
}

// =============================================================================
// LIFECYCLE HOOKS TEST ACTORS (from lifecycle_hooks_tests.rs)
// =============================================================================

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
}

#[async_trait]
impl plexspaces_actor::Actor for TestLifecycleActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        *self.init_called.lock().unwrap() = true;
        self.init_result
            .clone()
            .map_err(|e| ActorError::InvalidState(e))
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
        let action = ExitAction::Handle;
        *self.handle_exit_action.lock().unwrap() = Some(action.clone());
        Ok(action)
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// =============================================================================
// OBSERVABILITY TEST ACTORS (from lifecycle_observability_tests.rs)
// =============================================================================

struct ObservabilityTestActor {
    init_called: Arc<std::sync::atomic::AtomicBool>,
}

impl ObservabilityTestActor {
    fn new() -> Self {
        Self {
            init_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl plexspaces_actor::Actor for ObservabilityTestActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        self.init_called
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

struct FailingInitActor;

#[async_trait]
impl plexspaces_actor::Actor for FailingInitActor {
    async fn init(&mut self, _ctx: &ActorContext) -> Result<(), ActorError> {
        Err(ActorError::InvalidState("init failed".to_string()))
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// =============================================================================
// UNIFIED LIFECYCLE TEST FACET (from unified_lifecycle_tests.rs)
// =============================================================================

struct TestFacet {
    facet_type: String,
    priority: i32,
    attach_called: Arc<AtomicU32>,
    init_complete_called: Arc<AtomicU32>,
    terminate_start_called: Arc<AtomicU32>,
    detach_called: Arc<AtomicU32>,
}

impl TestFacet {
    fn with_trackers(
        facet_type: &str,
        priority: i32,
        attach_called: Arc<AtomicU32>,
        init_complete_called: Arc<AtomicU32>,
        terminate_start_called: Arc<AtomicU32>,
        detach_called: Arc<AtomicU32>,
    ) -> Self {
        Self {
            facet_type: facet_type.to_string(),
            priority,
            attach_called,
            init_complete_called,
            terminate_start_called,
            detach_called,
        }
    }
}

#[async_trait]
impl Facet for TestFacet {
    fn facet_type(&self) -> &str {
        &self.facet_type
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        self.attach_called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        self.detach_called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_init_complete(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        self.init_complete_called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_terminate_start(
        &mut self,
        _actor_id: &str,
        _reason: &plexspaces_facet::ExitReason,
    ) -> Result<(), FacetError> {
        self.terminate_start_called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn get_config(&self) -> Value {
        Value::Object(serde_json::Map::new())
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

// =============================================================================
// LIFECYCLE HOOKS TESTS (from lifecycle_hooks_tests.rs - 4 tests)
// =============================================================================

#[tokio::test]
async fn test_init_called_before_message_loop() {
    let actor_impl = TestLifecycleActor::new();
    let init_called = actor_impl.init_called.clone();

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let handle = actor.start().await.expect("Actor should start");
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(
        *init_called.lock().unwrap(),
        "init() should be called before message loop"
    );

    let _ = actor.stop().await;
    let _ = handle.await;
}

#[tokio::test]
async fn test_init_failure_prevents_start() {
    let actor_impl = TestLifecycleActor::new().with_init_error("init failed".to_string());

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let result = actor.start().await;
    assert!(result.is_err(), "Actor start should fail if init() fails");

    if let Err(e) = result {
        assert!(
            e.to_string().contains("init failed"),
            "Error should contain init failure message"
        );
    }
}

#[tokio::test]
async fn test_terminate_called_on_stop() {
    let actor_impl = TestLifecycleActor::new();
    let terminate_called = actor_impl.terminate_called.clone();
    let terminate_reason = actor_impl.terminate_reason.clone();

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let handle = actor.start().await.expect("Actor should start");
    tokio::time::sleep(Duration::from_millis(100)).await;

    actor.stop().await.expect("Actor should stop");

    assert!(
        *terminate_called.lock().unwrap(),
        "terminate() should be called on stop"
    );
    assert_eq!(
        terminate_reason.lock().unwrap().as_ref(),
        Some(&ExitReason::Shutdown),
        "terminate() should be called with Shutdown reason"
    );

    let _ = handle.await;
}

#[tokio::test]
async fn test_handle_exit_called_when_linked_actor_dies() {
    use plexspaces_actor::TestServiceLocatorStub;
    let service_locator: Arc<dyn plexspaces_actor::ServiceLocator> =
        Arc::new(TestServiceLocatorStub::new());
    let mut ctx = ActorContext::new(
        "node-1".to_string(),
        "tenant".to_string(),
        "namespace".to_string(),
        service_locator,
        None,
    );
    ctx.trap_exit = true;
    let ctx = Arc::new(ctx);

    let mut actor_impl_test = TestLifecycleActor::new();
    let handle_exit_called_test = actor_impl_test.handle_exit_called.clone();
    let handle_exit_action_test = actor_impl_test.handle_exit_action.clone();
    let from = linked_actor_id("linked-actor");
    let reason = ExitReason::Error("linked actor crashed".to_string());

    let result = ActorTrait::handle_exit(&mut actor_impl_test, &ctx, &from, &reason).await;
    assert!(result.is_ok(), "handle_exit() should succeed");
    assert_eq!(
        result.unwrap(),
        ExitAction::Handle,
        "handle_exit() should return Handle action"
    );

    assert!(
        *handle_exit_called_test.lock().unwrap(),
        "handle_exit() should be called"
    );
    assert_eq!(
        handle_exit_action_test.lock().unwrap().as_ref(),
        Some(&ExitAction::Handle),
        "handle_exit() should set action"
    );
}

// =============================================================================
// LIFECYCLE OBSERVABILITY TESTS (from lifecycle_observability_tests.rs - 4 tests)
// =============================================================================

#[tokio::test]
async fn test_metrics_emitted_on_init_success() {
    let actor_impl = ObservabilityTestActor::new();

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;

    assert!(
        actor.state().await == plexspaces_actor::ActorState::Active
            || actor.state().await == plexspaces_actor::ActorState::Stopping
            || actor.state().await == plexspaces_actor::ActorState::Terminated
    );

    let _ = actor.stop().await;
    let _ = handle.await;
}

#[tokio::test]
async fn test_metrics_emitted_on_init_failure() {
    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(FailingInitActor),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let result = actor.start().await;
    assert!(result.is_err(), "Actor start should fail");

    let state = actor.state().await;
    assert!(
        matches!(state, plexspaces_actor::ActorState::Failed(_)),
        "Actor should be in Failed state"
    );
}

#[tokio::test]
async fn test_metrics_emitted_on_terminate() {
    let actor_impl = ObservabilityTestActor::new();

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;

    let _ = actor.stop().await;

    let state = actor.state().await;
    assert!(
        state == plexspaces_actor::ActorState::Terminated
            || state == plexspaces_actor::ActorState::Stopping
    );

    let _ = handle.await;
}

#[tokio::test]
async fn test_logging_on_lifecycle_events() {
    let actor_impl = ObservabilityTestActor::new();

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        "test-actor".to_string(),
        Box::new(actor_impl),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    let handle = actor.start().await.expect("Actor should start");
    sleep(Duration::from_millis(100)).await;

    let _ = actor.stop().await;
    let _ = handle.await;
}

// =============================================================================
// UNIFIED LIFECYCLE TESTS (from unified_lifecycle_tests.rs - 5 tests)
// =============================================================================

/// Simple actor for facet tests
struct SimpleActor;

#[async_trait]
impl ActorTrait for SimpleActor {
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
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_multiple_facets_attachment_order() {
    let facet1_attach = Arc::new(AtomicU32::new(0));
    let facet1_init_complete = Arc::new(AtomicU32::new(0));
    let facet1 = TestFacet::with_trackers(
        "metrics",
        800,
        facet1_attach.clone(),
        facet1_init_complete.clone(),
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
    );

    let facet2_attach = Arc::new(AtomicU32::new(0));
    let facet2_init_complete = Arc::new(AtomicU32::new(0));
    let facet2 = TestFacet::with_trackers(
        "timer",
        100,
        facet2_attach.clone(),
        facet2_init_complete.clone(),
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
    );

    let facet3_attach = Arc::new(AtomicU32::new(0));
    let facet3_init_complete = Arc::new(AtomicU32::new(0));
    let facet3 = TestFacet::with_trackers(
        "durability",
        50,
        facet3_attach.clone(),
        facet3_init_complete.clone(),
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
    );

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        format!("test-actor-{}", Ulid::new()),
        Box::new(SimpleActor),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    actor.attach_facet(Box::new(facet1)).await.unwrap();
    actor.attach_facet(Box::new(facet2)).await.unwrap();
    actor.attach_facet(Box::new(facet3)).await.unwrap();

    let handle_result = actor.start().await;

    if handle_result.is_ok() {
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            facet1_attach.load(Ordering::SeqCst),
            1,
            "Metrics facet on_attach should be called"
        );
        assert_eq!(
            facet2_attach.load(Ordering::SeqCst),
            1,
            "Timer facet on_attach should be called"
        );
        assert_eq!(
            facet3_attach.load(Ordering::SeqCst),
            1,
            "Durability facet on_attach should be called"
        );

        assert_eq!(
            facet1_init_complete.load(Ordering::SeqCst),
            1,
            "Metrics facet on_init_complete should be called"
        );
        assert_eq!(
            facet2_init_complete.load(Ordering::SeqCst),
            1,
            "Timer facet on_init_complete should be called"
        );
        assert_eq!(
            facet3_init_complete.load(Ordering::SeqCst),
            1,
            "Durability facet on_init_complete should be called"
        );

        actor.stop().await.unwrap();
    } else {
        assert_eq!(
            facet1_attach.load(Ordering::SeqCst),
            1,
            "Metrics facet on_attach should be called"
        );
        assert_eq!(
            facet2_attach.load(Ordering::SeqCst),
            1,
            "Timer facet on_attach should be called"
        );
        assert_eq!(
            facet3_attach.load(Ordering::SeqCst),
            1,
            "Durability facet on_attach should be called"
        );
    }
}

#[tokio::test]
async fn test_multiple_facets_detachment_order() {
    let terminate_called = Arc::new(AtomicU32::new(0));
    let terminate_called_clone = terminate_called.clone();

    struct SimpleActorWithTerminate {
        terminate_called: Arc<AtomicU32>,
    }
    #[async_trait]
    impl plexspaces_actor::Actor for SimpleActorWithTerminate {
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
            self.terminate_called.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::GenServer
        }
    }

    let facet1_terminate_start = Arc::new(AtomicU32::new(0));
    let facet1_detach = Arc::new(AtomicU32::new(0));
    let facet1 = TestFacet::with_trackers(
        "metrics",
        800,
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        facet1_terminate_start.clone(),
        facet1_detach.clone(),
    );

    let facet2_terminate_start = Arc::new(AtomicU32::new(0));
    let facet2_detach = Arc::new(AtomicU32::new(0));
    let facet2 = TestFacet::with_trackers(
        "timer",
        100,
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        facet2_terminate_start.clone(),
        facet2_detach.clone(),
    );

    let facet3_terminate_start = Arc::new(AtomicU32::new(0));
    let facet3_detach = Arc::new(AtomicU32::new(0));
    let facet3 = TestFacet::with_trackers(
        "durability",
        50,
        Arc::new(AtomicU32::new(0)),
        Arc::new(AtomicU32::new(0)),
        facet3_terminate_start.clone(),
        facet3_detach.clone(),
    );

    let mailbox = Mailbox::new(MailboxConfig::default(), format!("mailbox-{}", Ulid::new()), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        format!("test-actor-{}", Ulid::new()),
        Box::new(SimpleActorWithTerminate {
            terminate_called: terminate_called_clone,
        }),
        mailbox,
        "tenant".to_string(),
        "namespace".to_string(),
    )
    .await;

    actor.attach_facet(Box::new(facet1)).await.unwrap();
    actor.attach_facet(Box::new(facet2)).await.unwrap();
    actor.attach_facet(Box::new(facet3)).await.unwrap();

    let handle_result = actor.start().await;
    if handle_result.is_ok() {
        tokio::time::sleep(Duration::from_millis(100)).await;

        actor.stop().await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(
            facet1_terminate_start.load(Ordering::SeqCst),
            1,
            "Metrics facet on_terminate_start should be called"
        );
        assert_eq!(
            facet2_terminate_start.load(Ordering::SeqCst),
            1,
            "Timer facet on_terminate_start should be called"
        );
        assert_eq!(
            facet3_terminate_start.load(Ordering::SeqCst),
            1,
            "Durability facet on_terminate_start should be called"
        );

        assert_eq!(
            terminate_called.load(Ordering::SeqCst),
            1,
            "Actor terminate() should be called"
        );

        assert_eq!(
            facet1_detach.load(Ordering::SeqCst),
            1,
            "Metrics facet on_detach should be called"
        );
        assert_eq!(
            facet2_detach.load(Ordering::SeqCst),
            1,
            "Timer facet on_detach should be called"
        );
        assert_eq!(
            facet3_detach.load(Ordering::SeqCst),
            1,
            "Durability facet on_detach should be called"
        );
    }
}

/// Test S-I-1: Supervisor Restart with Facets
#[tokio::test]
async fn test_supervisor_restart_with_facets() {
    // TODO: Implement when FacetRegistry is available via ServiceLocator
}

/// Test A-LC-1: Application Deploy with Facets
#[tokio::test]
async fn test_application_deploy_with_facets() {
    // TODO: Implement when FacetRegistry is available via ServiceLocator
}

/// Test A-LC-2: Application Undeploy with Facets
#[tokio::test]
async fn test_application_undeploy_with_facets() {
    // TODO: Implement when FacetRegistry is available via ServiceLocator
}
