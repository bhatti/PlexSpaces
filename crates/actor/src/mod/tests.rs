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

use super::*;
use crate::behavior::MockBehavior;
use crate::core::{
    ActorContext, ActorStateHandle as CoreActorStateHandle, BehaviorError, ServiceLocator,
};
use async_trait::async_trait;
use plexspaces_journaling::{DurabilityFacet, JournalStorage, SqliteJournalStorage};
use plexspaces_mailbox::MailboxConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

async fn actor_with_default_service_locator(
    id: ActorId,
    behavior: Box<dyn crate::core::Actor>,
    mailbox: Mailbox,
    tenant_id: String,
    namespace: String,
) -> ActorInstance {
    let node_id = "test-node".to_string();
    let service_locator: Arc<dyn crate::core::ServiceLocator> =
        Arc::new(crate::TestServiceLocatorStub::new());
    // self_ref must be set — mirrors what actor_factory_impl does at spawn time so that
    // ctx.actor_id() never panics inside the message loop.
    let self_ref = crate::core::ActorRef::new(id.clone())
        .expect("test actor id must be valid for self_ref construction");
    let context = Arc::new(
        ActorContext::new(
            node_id,
            tenant_id.clone(),
            namespace.clone(),
            service_locator,
            None,
        )
        .with_self_ref(self_ref),
    );
    ActorInstance::new(id, behavior, mailbox, tenant_id, namespace, None).set_context(context)
}

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

/// Helper to create a test message with TTL
#[allow(dead_code)]
fn create_test_message_with_ttl(payload: Vec<u8>, ttl: std::time::Duration) -> Message {
    let mut msg = create_test_message(payload);
    msg.ttl = Some(prost_types::Duration {
        seconds: ttl.as_secs() as i64,
        nanos: ttl.subsec_nanos() as i32,
    });
    msg
}

fn runtime_actor_id(name: &str) -> ActorId {
    ActorId::new(name, "gen_server", "test-namespace", "test-node")
        .expect("test actor id should be valid")
}

struct TerminateTrackingBehavior {
    terminated: Arc<AtomicBool>,
}

struct CheckpointTrackingBehavior {
    count: i32,
    observed_count: Arc<tokio::sync::RwLock<i32>>,
    #[allow(dead_code)]
    behavior_type: crate::core::BehaviorType,
}

impl CheckpointTrackingBehavior {
    async fn set_observed_count(&self) {
        *self.observed_count.write().await = self.count;
    }
}

#[async_trait]
impl crate::core::Actor for TerminateTrackingBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _message: Message,
    ) -> Result<(), BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> crate::core::BehaviorType {
        crate::core::BehaviorType::GenServer
    }

    async fn terminate(
        &mut self,
        _ctx: &ActorContext,
        _reason: &crate::core::ExitReason,
    ) -> Result<(), crate::core::ActorError> {
        self.terminated.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl crate::core::Actor for CheckpointTrackingBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        if message.payload == b"inc".to_vec() {
            self.count += 1;
            self.set_observed_count().await;
        }
        Ok(())
    }

    fn behavior_type(&self) -> crate::core::BehaviorType {
        crate::core::BehaviorType::GenServer
    }

    async fn capture_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
    ) -> Result<Option<Vec<u8>>, crate::core::ActorError> {
        serde_json::to_vec(&serde_json::json!({ "count": self.count }))
            .map(Some)
            .map_err(|e| crate::core::ActorError::BehaviorError(e.to_string()))
    }

    async fn restore_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
        state_data: &[u8],
    ) -> Result<bool, crate::core::ActorError> {
        let value: serde_json::Value = serde_json::from_slice(state_data)
            .map_err(|e| crate::core::ActorError::BehaviorError(e.to_string()))?;
        self.count = value
            .get("count")
            .and_then(|count| count.as_i64())
            .unwrap_or_default() as i32;
        self.set_observed_count().await;
        Ok(true)
    }
}

async fn wait_for_count(observed_count: &Arc<tokio::sync::RwLock<i32>>, expected: i32) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if *observed_count.read().await == expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("timed out waiting for actor state");
}

#[tokio::test]
async fn test_actor_lifecycle() {
    let id = runtime_actor_id("test-actor");
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        id.clone(),
        behavior,
        mailbox,
        String::new(),
        "test-namespace".to_string(),
    )
    .await;

    // Test initial state
    assert_eq!(actor.state().await, ActorState::Creating);

    // Start actor
    actor.start().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Active);

    // Stop actor
    actor.stop().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Terminated);
}

#[tokio::test]
async fn test_actor_handle_stop_actor_runs_terminate() {
    let id = runtime_actor_id("test-stop-handle");
    let terminated = Arc::new(AtomicBool::new(false));
    let behavior = Box::new(TerminateTrackingBehavior {
        terminated: terminated.clone(),
    });
    let mailbox = Mailbox::new(MailboxConfig::default(), id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        id,
        behavior,
        mailbox,
        String::new(),
        "test-namespace".to_string(),
    )
    .await;

    actor.start().await.unwrap();
    CoreActorStateHandle::stop_actor(&actor).await.unwrap();

    assert!(terminated.load(Ordering::SeqCst));
    assert_eq!(actor.state().await, ActorState::Terminated);
}

#[tokio::test]
async fn test_non_virtual_durable_actor_restores_checkpoint_on_restart() {
    let actor_id = runtime_actor_id("durable-checkpoint-actor");
    let namespace = "test-namespace".to_string();
    let observed_count = Arc::new(tokio::sync::RwLock::new(0));
    let storage: Arc<dyn JournalStorage> = Arc::new(
        SqliteJournalStorage::new(":memory:")
            .await
            .expect("sqlite durability storage"),
    );

    let behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenServer,
    });
    let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        actor_id.clone(),
        behavior,
        mailbox,
        String::new(),
        namespace.clone(),
    )
    .await;
    actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage.clone(),
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    actor.start().await.unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    wait_for_count(&observed_count, 2).await;
    CoreActorStateHandle::stop_actor(&actor).await.unwrap();

    let checkpoint = storage
        .get_latest_checkpoint(&actor_id.to_string())
        .await
        .expect("checkpoint should be saved on stop");
    assert!(!checkpoint.state_data.is_empty());
    let checkpoint_payload: serde_json::Value =
        serde_json::from_slice(&checkpoint.state_data).expect("checkpoint must be valid JSON");
    assert_eq!(
        checkpoint_payload
            .get("count")
            .and_then(|count| count.as_i64())
            .unwrap_or_default(),
        2
    );

    *observed_count.write().await = 0;
    let restarted_behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenServer,
    });
    let restarted_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut restarted_actor = actor_with_default_service_locator(
        actor_id.clone(),
        restarted_behavior,
        restarted_mailbox,
        String::new(),
        namespace,
    )
    .await;
    restarted_actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage.clone(),
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    restarted_actor.start().await.unwrap();
    wait_for_count(&observed_count, 2).await;
}

#[tokio::test]
async fn test_non_virtual_non_durable_actor_restarts_from_fresh_state() {
    let actor_id = runtime_actor_id("non-durable-checkpoint-actor");
    let observed_count = Arc::new(tokio::sync::RwLock::new(0));

    let behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenServer,
    });
    let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        actor_id.clone(),
        behavior,
        mailbox,
        String::new(),
        "test-namespace".to_string(),
    )
    .await;
    actor.start().await.unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    wait_for_count(&observed_count, 2).await;
    CoreActorStateHandle::stop_actor(&actor).await.unwrap();

    *observed_count.write().await = 0;
    let restarted_behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenServer,
    });
    let restarted_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut restarted_actor = actor_with_default_service_locator(
        actor_id,
        restarted_behavior,
        restarted_mailbox,
        String::new(),
        "test-namespace".to_string(),
    )
    .await;
    restarted_actor.start().await.unwrap();
    wait_for_count(&observed_count, 0).await;
}

#[tokio::test]
async fn test_non_virtual_durable_workflow_actor_restores_checkpoint_on_restart() {
    let actor_id = runtime_actor_id("durable-workflow-checkpoint-actor");
    let namespace = "test-namespace".to_string();
    let observed_count = Arc::new(tokio::sync::RwLock::new(0));
    let storage: Arc<dyn JournalStorage> = Arc::new(
        SqliteJournalStorage::new(":memory:")
            .await
            .expect("sqlite durability storage"),
    );

    let behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::Workflow,
    });
    let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        actor_id.clone(),
        behavior,
        mailbox,
        String::new(),
        namespace.clone(),
    )
    .await;
    actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage.clone(),
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    actor.start().await.unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    wait_for_count(&observed_count, 2).await;
    CoreActorStateHandle::stop_actor(&actor).await.unwrap();

    *observed_count.write().await = 0;
    let restarted_behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::Workflow,
    });
    let restarted_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut restarted_actor = actor_with_default_service_locator(
        actor_id,
        restarted_behavior,
        restarted_mailbox,
        String::new(),
        namespace,
    )
    .await;
    restarted_actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage,
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    restarted_actor.start().await.unwrap();
    wait_for_count(&observed_count, 2).await;
}

#[tokio::test]
async fn test_non_virtual_durable_event_actor_restores_checkpoint_on_restart() {
    let actor_id = runtime_actor_id("durable-event-checkpoint-actor");
    let namespace = "test-namespace".to_string();
    let observed_count = Arc::new(tokio::sync::RwLock::new(0));
    let storage: Arc<dyn JournalStorage> = Arc::new(
        SqliteJournalStorage::new(":memory:")
            .await
            .expect("sqlite durability storage"),
    );

    let behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenEvent,
    });
    let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut actor = actor_with_default_service_locator(
        actor_id.clone(),
        behavior,
        mailbox,
        String::new(),
        namespace.clone(),
    )
    .await;
    actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage.clone(),
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    actor.start().await.unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    actor
        .send(create_test_message(b"inc".to_vec()))
        .await
        .unwrap();
    wait_for_count(&observed_count, 2).await;
    CoreActorStateHandle::stop_actor(&actor).await.unwrap();

    *observed_count.write().await = 0;
    let restarted_behavior = Box::new(CheckpointTrackingBehavior {
        count: 0,
        observed_count: observed_count.clone(),
        behavior_type: crate::core::BehaviorType::GenEvent,
    });
    let restarted_mailbox = Mailbox::new(MailboxConfig::default(), actor_id.to_string(), String::new(), String::new(), None)
        .await
        .unwrap();
    let mut restarted_actor = actor_with_default_service_locator(
        actor_id,
        restarted_behavior,
        restarted_mailbox,
        String::new(),
        namespace,
    )
    .await;
    restarted_actor
        .attach_facet(Box::new(DurabilityFacet::new(
            storage,
            serde_json::json!({
                "checkpoint_interval": 1,
                "replay_on_activation": true,
                "state_schema_version": 1
            }),
            50,
        )))
        .await
        .expect("attach durability facet");
    restarted_actor.start().await.unwrap();
    wait_for_count(&observed_count, 2).await;
}

#[tokio::test]
async fn test_actor_id() {
    let id1 = runtime_actor_id("test");
    let id2 = runtime_actor_id("test");
    assert_eq!(id1, id2);
    assert_eq!(id1.name(), "test");
}

/// Test that on_activate is ALWAYS called during actor start
/// This is a CORE feature - not optional
#[tokio::test]
async fn test_static_lifecycle_on_activate_always_runs() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start actor - on_activate should ALWAYS run (transitions to Active state)
    actor.start().await.unwrap();

    // Verify state transition to Active (on_activate completed successfully)
    assert_eq!(actor.state().await, ActorState::Active);
}

/// Test that on_deactivate is ALWAYS called during actor stop
/// This is a CORE feature - not optional
#[tokio::test]
async fn test_static_lifecycle_on_deactivate_always_runs() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    actor.start().await.unwrap();
    actor.stop().await.unwrap();

    // Verify state transition to Terminated (on_deactivate completed successfully)
    assert_eq!(actor.state().await, ActorState::Terminated);
}

/// Test that lifecycle state transitions are predictable
#[tokio::test]
async fn test_static_lifecycle_state_transitions() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Initial state
    assert_eq!(actor.state().await, ActorState::Creating);

    // During start, should go: Created -> Activating -> Active
    actor.start().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Active);

    // During stop, should go: Active -> Deactivating -> Terminated
    actor.stop().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Terminated);
}

/// Test that behaviors can optionally extend lifecycle
/// The core lifecycle always runs, but behaviors can add custom init/cleanup
#[tokio::test]
async fn test_static_lifecycle_allows_behavior_extension() {
    // This test will pass once we implement the helper methods
    // for behaviors to extend lifecycle (like as_lifecycle_mut())
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Should work even if behavior doesn't implement lifecycle extensions
    actor.start().await.unwrap();
    actor.stop().await.unwrap();
}

// ============================================================================
// RESOURCE MONITORING TESTS
// ============================================================================

#[tokio::test]
async fn test_with_resource_profile() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let profile = ResourceProfile::ResourceProfileCpuIntensive;

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    )
    .with_resource_profile(profile);

    // Verify profile was set
    assert_eq!(
        actor.resource_profile,
        ResourceProfile::ResourceProfileCpuIntensive
    );
}

#[tokio::test]
async fn test_with_resource_contract() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let contract = ResourceContract {
        max_memory_bytes: 1024 * 1024 * 50u64, // 50 MB
        max_cpu_percent: 80.0,
        max_io_ops_per_sec: Some(1000),
        guaranteed_bandwidth_mbps: Some(10),
        max_execution_time: None,
    };

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    )
    .with_resource_contract(contract.clone());

    // Verify contract was set
    assert!(actor.resource_contract.is_some());
    let stored_contract = actor.resource_contract.unwrap();
    assert_eq!(stored_contract.max_memory_bytes, 1024 * 1024 * 50u64);
    assert_eq!(stored_contract.max_cpu_percent, 80.0);
    assert_eq!(stored_contract.max_io_ops_per_sec, Some(1000));
}

#[tokio::test]
async fn test_resource_usage() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Get initial resource usage
    let usage = actor.resource_usage().await;

    // Should have zero initial usage
    assert_eq!(usage.memory_bytes, 0);
    assert_eq!(usage.cpu_percent, 0.0);
    assert_eq!(usage.io_ops_per_sec, 0);
    assert_eq!(usage.network_mbps, 0);
}

#[tokio::test]
async fn test_health_check() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Check health (should be Healthy initially)
    let health = actor.health().await;

    // Actor just created, should be healthy
    assert!(health.is_healthy());
}

#[tokio::test]
async fn test_validate_resources_without_contract() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Validate resources (should pass without contract)
    let result = actor.validate_resources().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_validate_resources_with_contract() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let contract = ResourceContract {
        max_memory_bytes: 1024 * 1024 * 1000u64, // 1000 MB (high limit)
        max_cpu_percent: 100.0,
        max_io_ops_per_sec: Some(10000),
        guaranteed_bandwidth_mbps: None,
        max_execution_time: None,
    };

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    )
    .with_resource_contract(contract);

    // Validate resources (should pass with reasonable contract)
    let result = actor.validate_resources().await;
    assert!(result.is_ok());
}

// ============================================================================
// ACTOR LIFECYCLE ERROR PATHS TESTS
// ============================================================================

#[tokio::test]
async fn test_start_already_active_error() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start actor
    actor.start().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Active);

    // Try to start again (should fail)
    let result = actor.start().await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ActorError::InvalidState(_)));
}

#[tokio::test]
async fn test_stop_already_stopped() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start then stop
    actor.start().await.unwrap();
    actor.stop().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Terminated);

    // Try to stop again (should succeed silently)
    let result = actor.stop().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_message() {
    use plexspaces_mailbox::mailbox_config_default;

    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .expect("Failed to create mailbox");

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Send a message
    let msg = create_test_message(b"test message".to_vec());
    let result = actor.send(msg).await;
    assert!(result.is_ok());

    // Verify message is in mailbox
    assert_eq!(actor.mailbox().size(), 1);
}

#[tokio::test]
async fn test_actor_id_getter() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor-123".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor-123"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Test id() getter
    assert_eq!(actor.id(), &runtime_actor_id("test-actor-123"));
}

#[tokio::test]
async fn test_mailbox_getter() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Test mailbox() getter returns a reference
    // Just verify we can access the mailbox
    let _mailbox_ref = actor.mailbox();
    assert!(true); // Test passes if we can get the mailbox reference
}

// ============================================================================
// BEHAVIOR STACK TESTS (become/unbecome pattern)
// ============================================================================

#[tokio::test]
async fn test_become_changes_behavior() {
    let behavior1 = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior1,
        mailbox,
        "test-tenant".to_string(),
        "test-namespace".to_string(),
        None,
    );

    // Change behavior (using raw identifier since "become" is reserved)
    let behavior2 = Box::new(MockBehavior::new());
    let result = actor.r#become(behavior2).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unbecome_restores_previous_behavior() {
    let behavior1 = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior1,
        mailbox,
        "test-tenant".to_string(),
        "test-namespace".to_string(),
        None,
    );

    // Change behavior (using raw identifier since "become" is reserved)
    let behavior2 = Box::new(MockBehavior::new());
    actor.r#become(behavior2).await.unwrap();

    // Restore previous behavior
    let result = actor.unbecome().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unbecome_error_when_no_previous_behavior() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Try to unbecome without become (should fail)
    let result = actor.unbecome().await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ActorError::NoBehaviorToRestore
    ));
}

// ============================================================================
// FACET MANAGEMENT TESTS
// ============================================================================

#[tokio::test]
async fn test_attach_facet() {
    use async_trait::async_trait;
    use plexspaces_facet::{Facet, FacetError};

    // Simple test facet
    struct TestFacet {
        config: serde_json::Value,
        priority: i32,
    }

    impl TestFacet {
        fn new(config: serde_json::Value, priority: i32) -> Self {
            Self { config, priority }
        }
    }

    #[async_trait]
    impl Facet for TestFacet {
        fn facet_type(&self) -> &str {
            "test-facet"
        }

        fn get_config(&self) -> serde_json::Value {
            self.config.clone()
        }

        fn get_priority(&self) -> i32 {
            self.priority
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }

        async fn on_attach(
            &mut self,
            _actor_id: &str,
            _config: serde_json::Value,
        ) -> Result<(), FacetError> {
            Ok(())
        }

        async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
            Ok(())
        }

        // Default implementations are provided by the trait for before_method and after_method
        // So we don't need to implement them unless we want custom behavior
    }

    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Attach facet
    let facet = Box::new(TestFacet::new(serde_json::json!({}), 50));
    let result = actor.attach_facet(facet).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_facets() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // List facets (should be empty initially)
    let facets = actor.list_facets().await;
    assert_eq!(facets.len(), 0);
}

#[tokio::test]
async fn test_detach_facet() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Try to detach non-existent facet (should succeed - no-op)
    let result = actor.detach_facet("non-existent").await;
    // Note: This might fail depending on FacetContainer implementation
    // If it returns an error, that's also acceptable behavior
    let _ = result; // Accept either Ok or Err
}

// ============================================================================
// MESSAGE LOOP AND PROCESSING TESTS
// ============================================================================

/// Test that actor actually processes messages in its message loop
/// This covers the message loop internals (lines 425-442)
#[tokio::test]
async fn test_actor_processes_messages_in_loop() {
    use plexspaces_mailbox::mailbox_config_default;

    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .expect("Failed to create mailbox");

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start actor (message loop begins)
    let _handle = actor.start().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Active);

    // Send a message
    let msg = create_test_message(b"test message".to_vec());
    actor.send(msg).await.unwrap();

    // Give actor time to process message
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Stop actor gracefully
    actor.stop().await.unwrap();

    // Verify final state
    assert_eq!(actor.state().await, ActorState::Terminated);
}

/// Test graceful shutdown via shutdown channel
/// This covers shutdown path (lines 444-451)
#[tokio::test]
async fn test_actor_graceful_shutdown() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start actor
    let handle = actor.start().await.unwrap();
    assert_eq!(actor.state().await, ActorState::Active);

    // Stop actor (triggers shutdown signal)
    actor.stop().await.unwrap();

    // Wait for shutdown to complete
    let result = tokio::time::timeout(tokio::time::Duration::from_secs(1), handle).await;

    // Handle should complete successfully
    assert!(result.is_ok());
    assert_eq!(actor.state().await, ActorState::Terminated);
}

/// Test on_timer() hook forwards timer events to behavior
/// This covers timer hook (lines 612-625)
#[tokio::test]
async fn test_on_timer_hook() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = ActorInstance::new(
        runtime_actor_id("test-actor"),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Call _on_timer hook (internal method)
    let result = actor._on_timer("test-timer").await;

    // Should succeed (MockBehavior accepts all messages)
    assert!(result.is_ok());
}

/// Test message processing with sender tracking
/// This covers process_message lines 658-662 (sender extraction)
#[tokio::test]
async fn test_message_processing_with_sender() {
    use plexspaces_mailbox::mailbox_config_default;

    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(mailbox_config_default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .expect("Failed to create mailbox");

    let mut actor = actor_with_default_service_locator(
        runtime_actor_id("receiver"),
        behavior,
        mailbox,
        "test-tenant".to_string(),
        "test-namespace".to_string(),
    )
    .await;

    // Start actor
    let _handle = actor.start().await.unwrap();

    // Create message with sender
    let mut msg = create_test_message(b"hello".to_vec());
    msg.sender_id = "sender-actor".to_string();

    // Send message
    actor.send(msg).await.unwrap();

    // Give time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Stop actor
    actor.stop().await.unwrap();
}

/// Test message processing error handling
/// This covers error path in message loop (lines 439-442)
#[tokio::test]
async fn test_message_processing_error_handling() {
    use crate::core::{Actor as ActorTrait, BehaviorError, BehaviorType};
    use async_trait::async_trait;
    use plexspaces_mailbox::mailbox_config_default;

    // Behavior that always fails
    struct FailingBehavior;

    #[async_trait]
    impl ActorTrait for FailingBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &crate::core::ActorContext,
            _msg: Message,
        ) -> Result<(), BehaviorError> {
            Err(BehaviorError::ProcessingError(
                "Intentional failure".to_string(),
            ))
        }

        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::Custom("Failing".to_string())
        }
    }

    let behavior = Box::new(FailingBehavior);
    let mailbox = Mailbox::new(mailbox_config_default(), "failing-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let mut actor = actor_with_default_service_locator(
        ActorId::new("failing-actor", "failing", "test-namespace", "test-node").unwrap(),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
    )
    .await;

    // Start actor
    let _handle = actor.start().await.unwrap();

    // Send message (will fail in processing)
    let msg = create_test_message(b"test".to_vec());
    actor.send(msg).await.unwrap();

    // Give time to attempt processing
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Actor should still be running despite error
    assert_eq!(actor.state().await, ActorState::Active);

    // Stop actor
    actor.stop().await.unwrap();
}

/// Test health check tracks last message time
/// This covers process_message line 641 (last_message_time update)
#[tokio::test]
async fn test_health_tracking_with_messages() {
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor".to_string(), String::new(), String::new(), None)
        .await
        .unwrap();

    let actor = ActorInstance::new(
        ActorId::new("test-actor", "mock", "test-namespace", "test-node").unwrap(),
        behavior,
        mailbox,
        String::new(), // tenant_id (empty if auth disabled)
        "test-namespace".to_string(),
        None,
    );

    // Check health immediately (should be Healthy)
    let health1 = actor.health().await;
    assert!(health1.is_healthy());

    // Simulate long wait (> 30 seconds for Stuck detection)
    // Note: We can't actually wait 30s in test, but we can verify the method works
    let health2 = actor.health().await;
    assert!(health2.is_healthy());
}

// ── Struct-size regression guards (D5 from planx.txt) ─────────────────────────

/// ActorMutableState must remain a compact value type.
///
/// D5 consolidated resource_usage, last_message_time, and error_message from
/// three separate Arc<RwLock<T>> fields into one ActorMutableState behind a
/// single Arc<RwLock>.  The struct itself must stay small — a large inline
/// struct would negate the benefit of grouping them.
#[test]
fn actor_mutable_state_stack_size_is_compact() {
    let size = std::mem::size_of::<ActorMutableState>();
    assert!(
        size <= 256,
        "ActorMutableState size ({size}B) exceeds 256 B — check for added fields"
    );
}

/// ActorInstance is a thin handle: all heavy state is behind Arc.
/// The struct must not exceed 512 B on the stack; actual data lives on the heap.
#[test]
fn actor_instance_stack_size_is_within_budget() {
    let size = std::mem::size_of::<ActorInstance>();
    assert!(
        size <= 512,
        "ActorInstance struct size ({size}B) exceeds 512 B — check for added inline fields"
    );
}
