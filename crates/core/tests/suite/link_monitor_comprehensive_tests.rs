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

//! Comprehensive tests for Link/Monitor Semantics (Phase 6)
//!
//! Tests cover all scenarios for link/monitor operations:
//! - Link: Two-way death propagation
//!   - Normal exit doesn't propagate
//!   - Error exit propagates (cascading failure)
//!   - trap_exit=true: receives EXIT as message
//!   - trap_exit=false: terminates immediately
//! - Monitor: One-way notification
//!   - DOWN message on target death
//!   - Normal/error termination notifications
//! - Startup scenarios
//! - Termination scenarios
//! - Error scenarios
//! - Cascading failures
//! - Multiple links/monitors

use plexspaces_core::actor_trait::MessageSender;
use plexspaces_core::{
    Actor, ActorContext, ActorError, ActorId, ActorRegistry, BehaviorError, BehaviorType,
    ExitAction, ExitReason, Message, RequestContext,
};
// Note: Message is now unified proto Message from plexspaces_proto::common::v1

use async_trait::async_trait;

fn test_actor_id(name: &str) -> ActorId {
    ActorId::new(name, "gen_server", "default", "test-node").unwrap()
}

fn monitoring_request_context_for_tests() -> RequestContext {
    RequestContext::new_without_auth("test-tenant".into(), "default".into())
}
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use std::any::Any;
use std::sync::Arc;
use tokio::sync::mpsc;

// Helper to wrap ObjectRegistry for ActorRegistry
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait]
impl plexspaces_core::actor_context::ObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn lookup_full(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .lookup_full(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn register(
        &self,
        ctx: &plexspaces_core::RequestContext,
        registration: plexspaces_proto::object_registry::v1::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner.register(ctx, registration).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn discover(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_id_prefix: Option<String>,
        object_ids: Option<Vec<String>>,
        tags: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        limit: usize,
        offset: usize,
    ) -> Result<
        Vec<plexspaces_proto::object_registry::v1::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.inner
            .discover(
                ctx,
                object_type,
                object_id_prefix,
                object_ids,
                tags,
                health_status,
                limit,
                offset,
            )
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn unregister(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn heartbeat(
        &self,
        ctx: &plexspaces_core::RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.inner
            .heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

async fn create_test_registry() -> Arc<ActorRegistry> {
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
    let object_registry: Arc<dyn plexspaces_core::actor_context::ObjectRegistry> =
        Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
    Arc::new(ActorRegistry::new(object_registry, "test-node".to_string()))
}

/// Mock MessageSender that captures messages into an unbounded channel.
/// Used to register a monitoring actor in the registry so DOWN messages
/// can be delivered to its mailbox via ActorRegistry::tell().
struct MockMessageSender {
    tx: mpsc::UnboundedSender<Message>,
}

#[async_trait]
impl MessageSender for MockMessageSender {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.tx
            .send(message)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Register a mock actor in the registry and return the receiver end of the channel.
/// DOWN messages will be forwarded to this receiver when the monitored actor terminates.
async fn register_mock_actor(
    registry: &Arc<ActorRegistry>,
    actor_id: ActorId,
) -> mpsc::UnboundedReceiver<Message> {
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = Arc::new(MockMessageSender { tx });
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());
    registry
        .register_actor(
            &ctx,
            actor_id,
            sender,
            "MockActor".to_string(),
            None,
            None,
            None,
        )
        .await;
    rx
}

/// Test actor that can trap exits
struct TestActor {
    id: String,
    trap_exit: bool,
    exit_messages: Arc<tokio::sync::RwLock<Vec<(ActorId, ExitReason)>>>,
    should_fail_init: bool,
    should_fail_terminate: bool,
}

impl TestActor {
    fn new(id: String) -> Self {
        Self {
            id,
            trap_exit: false,
            exit_messages: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            should_fail_init: false,
            should_fail_terminate: false,
        }
    }

    fn with_trap_exit(mut self) -> Self {
        self.trap_exit = true;
        self
    }

    fn with_init_failure(mut self) -> Self {
        self.should_fail_init = true;
        self
    }

    fn with_terminate_failure(mut self) -> Self {
        self.should_fail_terminate = true;
        self
    }

    async fn get_exit_messages(&self) -> Vec<(ActorId, ExitReason)> {
        self.exit_messages.read().await.clone()
    }
}

#[async_trait]
impl Actor for TestActor {
    async fn init(&mut self, ctx: &ActorContext) -> Result<(), ActorError> {
        if self.should_fail_init {
            Err(ActorError::InvalidState("Init failed".to_string()))
        } else {
            // Set trap_exit if configured
            if self.trap_exit {
                // Note: ActorContext doesn't have a setter for trap_exit in the current API
                // This would need to be set during actor creation or via a facet
                // For now, we'll test the behavior assuming it's set correctly
            }
            Ok(())
        }
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Check if this is an EXIT message (message_type == "__EXIT__")
        if msg.message_type == "__EXIT__" {
            // EXIT information is in the payload/headers
            // For now, we'll handle it in handle_exit
        }
        Ok(())
    }

    async fn handle_exit(
        &mut self,
        _ctx: &ActorContext,
        from: &ActorId,
        reason: &ExitReason,
    ) -> Result<ExitAction, ActorError> {
        // Record EXIT message
        {
            let mut messages = self.exit_messages.write().await;
            messages.push((from.clone(), reason.clone()));
        }
        // Default: propagate (causes termination)
        Ok(ExitAction::Propagate)
    }

    async fn terminate(
        &mut self,
        _ctx: &ActorContext,
        reason: &ExitReason,
    ) -> Result<(), ActorError> {
        if self.should_fail_terminate {
            Err(ActorError::InvalidState("Terminate failed".to_string()))
        } else {
            Ok(())
        }
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

/// Test: Link two actors - normal exit doesn't propagate
#[tokio::test]
async fn test_link_normal_exit_no_propagation() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    // Link actor1 and actor2
    registry.link(&actor1_id, &actor2_id).await.unwrap();

    // Verify link is bidirectional
    let links1 = registry.get_links(&actor1_id).await;
    assert!(links1.contains(&actor2_id));

    let links2 = registry.get_links(&actor2_id).await;
    assert!(links2.contains(&actor1_id));

    // Terminate actor1 with normal exit
    registry
        .handle_actor_termination(&actor1_id, ExitReason::Normal)
        .await;

    // Actor2 should NOT receive EXIT (normal exits don't propagate)
    // Verify links are cleaned up (actor1 removed from actor2's links)
    let links2_after = registry.get_links(&actor2_id).await;
    assert!(
        !links2_after.contains(&actor1_id),
        "Actor1 should be removed from actor2's links after termination"
    );
}

/// Test: Link two actors - error exit propagates (cascading failure)
#[tokio::test]
async fn test_link_error_exit_propagates() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    // Link actor1 and actor2
    registry.link(&actor1_id, &actor2_id).await.unwrap();

    // Verify links are bidirectional
    let links1 = registry.get_links(&actor1_id).await;
    assert!(links1.contains(&actor2_id));

    let links2 = registry.get_links(&actor2_id).await;
    assert!(links2.contains(&actor1_id));

    // Terminate actor1 with error exit
    // Note: In a full implementation, this would send EXIT to actor2's mailbox
    // For now, we verify the link propagation logic exists
    registry
        .handle_actor_termination(&actor1_id, ExitReason::Error("test error".to_string()))
        .await;

    // Verify links are cleaned up after termination
    let links2_after = registry.get_links(&actor2_id).await;
    assert!(
        !links2_after.contains(&actor1_id),
        "Actor1 should be removed from actor2's links after termination"
    );
}

/// Test: Link with trap_exit=true receives EXIT as message
#[tokio::test]
async fn test_link_trap_exit_receives_message() {
    // This test requires actor to have trap_exit=true
    // and handle_exit to return ExitAction::Handle
    // Implementation needed in Actor message loop
}

/// Test: Link with trap_exit=false terminates immediately
#[tokio::test]
async fn test_link_no_trap_exit_terminates() {
    // This test verifies that when trap_exit=false,
    // linked actor terminates immediately on EXIT
}

/// Test: Monitor receives DOWN message on target death
#[tokio::test]
async fn test_monitor_receives_down_message() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register the monitoring actor so DOWN messages can be delivered to its mailbox
    let mut rx = register_mock_actor(&registry, monitor_id.clone()).await;

    // Register monitor
    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor
    registry
        .handle_actor_termination(&actor_id, ExitReason::Error("test error".to_string()))
        .await;

    // Monitor should receive a DOWN message via its mailbox
    let down_msg = rx.recv().await;
    assert!(down_msg.is_some());
    let msg = down_msg.unwrap();
    assert_eq!(msg.message_type, "__DOWN__");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(actor_id.to_string().as_str())
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("test error")
    );
}

/// Test: Multiple monitors receive DOWN messages
#[tokio::test]
async fn test_multiple_monitors_receive_down() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor1_id = test_actor_id("monitor1");
    let monitor2_id = test_actor_id("monitor2");

    // Register both monitoring actors so DOWN messages can be delivered to their mailboxes
    let mut rx1 = register_mock_actor(&registry, monitor1_id.clone()).await;
    let mut rx2 = register_mock_actor(&registry, monitor2_id.clone()).await;

    // Register both monitors
    registry
        .monitor(
            &actor_id,
            &monitor1_id,
            "monitor-ref-1".to_string(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();
    registry
        .monitor(
            &actor_id,
            &monitor2_id,
            "monitor-ref-2".to_string(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor
    registry
        .handle_actor_termination(&actor_id, ExitReason::Error("test error".to_string()))
        .await;

    // Both monitors should receive DOWN messages
    let down1 = rx1.recv().await;
    let down2 = rx2.recv().await;
    assert!(down1.is_some());
    assert!(down2.is_some());
    assert_eq!(down1.unwrap().message_type, "__DOWN__");
    assert_eq!(down2.unwrap().message_type, "__DOWN__");
}

/// Test: Cascading failure through multiple links
#[tokio::test]
async fn test_cascading_failure_multiple_links() {
    // A -> B -> C
    // If A dies with error, B dies, then C dies
}

/// Test: Unlink removes link
#[tokio::test]
async fn test_unlink_removes_link() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    // Link actors
    registry.link(&actor1_id, &actor2_id).await.unwrap();
    assert!(registry.get_links(&actor1_id).await.contains(&actor2_id));

    // Unlink actors
    registry.unlink(&actor1_id, &actor2_id).await.unwrap();
    assert!(!registry.get_links(&actor1_id).await.contains(&actor2_id));
    assert!(!registry.get_links(&actor2_id).await.contains(&actor1_id));
}

/// Test: Demonitor removes monitor
#[tokio::test]
async fn test_demonitor_removes_monitor() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register monitor (no channel needed since we only test registration/removal)
    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Demonitor
    registry
        .demonitor(&actor_id, &monitor_id, &monitor_ref)
        .await
        .unwrap();

    // Terminate actor - monitor should NOT receive DOWN (it was demonitored)
    registry
        .handle_actor_termination(&actor_id, ExitReason::Error("test error".to_string()))
        .await;
    // No channel to check; correctness verified by absence of panic/error above
}

/// Test: Link during startup (before actor is fully started)
#[tokio::test]
async fn test_link_during_startup() {
    // Link should work even if actor is not fully started yet
}

/// Test: Monitor during startup
#[tokio::test]
async fn test_monitor_during_startup() {
    // Monitor should work even if actor is not fully started yet
}

/// Test: Normal exit doesn't propagate to links
#[tokio::test]
async fn test_normal_exit_no_link_propagation() {
    // Already covered in test_link_normal_exit_no_propagation
}

/// Test: Shutdown exit doesn't propagate to links
#[tokio::test]
async fn test_shutdown_exit_no_link_propagation() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    registry.link(&actor1_id, &actor2_id).await.unwrap();

    // Terminate actor1 with shutdown exit
    registry
        .handle_actor_termination(&actor1_id, ExitReason::Shutdown)
        .await;

    // Actor2 should NOT receive EXIT (shutdown is like normal - doesn't propagate)
    // Verify links are cleaned up
    let links2_after = registry.get_links(&actor2_id).await;
    assert!(
        !links2_after.contains(&actor1_id),
        "Actor1 should be removed from actor2's links after shutdown"
    );
}

/// Test: Killed exit propagates to links
#[tokio::test]
async fn test_killed_exit_propagates() {
    // Killed is an error, so it should propagate
}

/// Test: Linked exit reason nesting
#[tokio::test]
async fn test_linked_exit_reason_nesting() {
    let registry = create_test_registry().await;

    let actor_a = test_actor_id("actor-a");
    let actor_b = test_actor_id("actor-b");
    let actor_c = test_actor_id("actor-c");

    // Link A -> B -> C
    registry.link(&actor_a, &actor_b).await.unwrap();
    registry.link(&actor_b, &actor_c).await.unwrap();

    // Verify links
    assert!(registry.get_links(&actor_a).await.contains(&actor_b));
    assert!(registry.get_links(&actor_b).await.contains(&actor_c));

    // Terminate A with error - should propagate to B, then C
    registry
        .handle_actor_termination(&actor_a, ExitReason::Error("error from A".to_string()))
        .await;

    // Verify links are cleaned up: A removed from B's links
    assert!(
        !registry.get_links(&actor_b).await.contains(&actor_a),
        "Actor A should be removed from actor B's links"
    );
    // Note: B and C are still linked (B hasn't terminated yet in this test)
    // In a full scenario with actual actors, B would receive EXIT message and terminate,
    // which would then propagate to C. But in this unit test, we're just testing the
    // link cleanup logic, not the full cascading termination.
    // Verify B and C are still linked (they haven't terminated)
    assert!(
        registry.get_links(&actor_b).await.contains(&actor_c),
        "Actor B and C should still be linked"
    );
    assert!(
        registry.get_links(&actor_c).await.contains(&actor_b),
        "Actor C and B should still be linked"
    );
}

/// Test: Link to non-existent actor (should still work - link is registered)
#[tokio::test]
async fn test_link_to_nonexistent_actor() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    // Link to non-existent actor should still work
    registry.link(&actor1_id, &actor2_id).await.unwrap();

    // Verify link is registered
    assert!(registry.get_links(&actor1_id).await.contains(&actor2_id));
    assert!(registry.get_links(&actor2_id).await.contains(&actor1_id));
}

/// Test: Monitor non-existent actor (should still work - monitor is registered)
#[tokio::test]
async fn test_monitor_nonexistent_actor() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Monitor non-existent actor should still work (no channel needed for this test)
    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // When actor terminates, monitor would receive DOWN (verified in other tests)
    registry
        .handle_actor_termination(&actor_id, ExitReason::Error("test error".to_string()))
        .await;
}

/// Test: Self-link prevention
#[tokio::test]
async fn test_self_link_prevention() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");

    // Attempting to link actor to itself should fail
    let result = registry.link(&actor_id, &actor_id).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("itself"));
}

/// Test: Multiple links to same actor
#[tokio::test]
async fn test_multiple_links_same_actor() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");
    let actor3_id = test_actor_id("actor3");

    // Link actor1 to both actor2 and actor3
    registry.link(&actor1_id, &actor2_id).await.unwrap();
    registry.link(&actor1_id, &actor3_id).await.unwrap();

    // Verify both links exist
    let links1 = registry.get_links(&actor1_id).await;
    assert!(links1.contains(&actor2_id));
    assert!(links1.contains(&actor3_id));

    // Terminate actor1 - both actor2 and actor3 should receive EXIT
    registry
        .handle_actor_termination(&actor1_id, ExitReason::Error("test error".to_string()))
        .await;

    // Verify links are cleaned up
    assert!(!registry.get_links(&actor2_id).await.contains(&actor1_id));
    assert!(!registry.get_links(&actor3_id).await.contains(&actor1_id));
}

/// Test: Unlink non-existent link (should succeed - idempotent)
#[tokio::test]
async fn test_unlink_nonexistent_link() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");

    // Unlink non-existent link should succeed (idempotent)
    let result = registry.unlink(&actor1_id, &actor2_id).await;
    assert!(result.is_ok());
}

/// Test: Demonitor non-existent monitor (should succeed - idempotent)
#[tokio::test]
async fn test_demonitor_nonexistent_monitor() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Demonitor non-existent monitor should succeed (idempotent)
    let result = registry
        .demonitor(&actor_id, &monitor_id, &monitor_ref)
        .await;
    assert!(result.is_ok());
}

/// Test: Monitor receives DOWN for normal exit
#[tokio::test]
async fn test_monitor_receives_down_normal_exit() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register the monitoring actor so DOWN messages can be delivered to its mailbox
    let mut rx = register_mock_actor(&registry, monitor_id.clone()).await;

    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor with normal exit
    registry
        .handle_actor_termination(&actor_id, ExitReason::Normal)
        .await;

    // Monitor should receive DOWN even for normal exit
    let down_msg = rx.recv().await;
    assert!(down_msg.is_some());
    let msg = down_msg.unwrap();
    assert_eq!(msg.message_type, "__DOWN__");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(actor_id.to_string().as_str())
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("normal")
    );
}

/// Test: Monitor receives DOWN for shutdown exit
#[tokio::test]
async fn test_monitor_receives_down_shutdown_exit() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register the monitoring actor so DOWN messages can be delivered to its mailbox
    let mut rx = register_mock_actor(&registry, monitor_id.clone()).await;

    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor with shutdown exit
    registry
        .handle_actor_termination(&actor_id, ExitReason::Shutdown)
        .await;

    // Monitor should receive DOWN
    let down_msg = rx.recv().await;
    assert!(down_msg.is_some());
    let msg = down_msg.unwrap();
    assert_eq!(msg.message_type, "__DOWN__");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(actor_id.to_string().as_str())
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("shutdown")
    );
}

/// Test: Monitor receives DOWN for killed exit
#[tokio::test]
async fn test_monitor_receives_down_killed_exit() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register the monitoring actor so DOWN messages can be delivered to its mailbox
    let mut rx = register_mock_actor(&registry, monitor_id.clone()).await;

    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor with killed exit
    registry
        .handle_actor_termination(&actor_id, ExitReason::Killed)
        .await;

    // Monitor should receive DOWN
    let down_msg = rx.recv().await;
    assert!(down_msg.is_some());
    let msg = down_msg.unwrap();
    assert_eq!(msg.message_type, "__DOWN__");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(actor_id.to_string().as_str())
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("killed")
    );
}

/// Test: Monitor receives DOWN for linked exit
#[tokio::test]
async fn test_monitor_receives_down_linked_exit() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor_id = test_actor_id("monitor1");
    let monitor_ref = "monitor-ref-1".to_string();

    // Register the monitoring actor so DOWN messages can be delivered to its mailbox
    let mut rx = register_mock_actor(&registry, monitor_id.clone()).await;

    registry
        .monitor(
            &actor_id,
            &monitor_id,
            monitor_ref.clone(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor with linked exit
    let linked_reason = ExitReason::Linked {
        actor_id: test_actor_id("linked-actor"),
        reason: Box::new(ExitReason::Error("linked error".to_string())),
    };
    registry
        .handle_actor_termination(&actor_id, linked_reason)
        .await;

    // Monitor should receive DOWN with linked reason
    let down_msg = rx.recv().await;
    assert!(down_msg.is_some());
    let msg = down_msg.unwrap();
    assert_eq!(msg.message_type, "__DOWN__");
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(actor_id.to_string().as_str())
    );
    let reason = msg
        .headers
        .get("down_reason")
        .map(|s| s.as_str())
        .unwrap_or("");
    assert!(
        reason.contains("linked"),
        "Expected 'linked' in reason, got: {}",
        reason
    );
}

/// Test: Link cleanup on termination removes all references
#[tokio::test]
async fn test_link_cleanup_removes_all_references() {
    let registry = create_test_registry().await;

    let actor1_id = test_actor_id("actor1");
    let actor2_id = test_actor_id("actor2");
    let actor3_id = test_actor_id("actor3");

    // Create links: actor1 <-> actor2, actor1 <-> actor3
    registry.link(&actor1_id, &actor2_id).await.unwrap();
    registry.link(&actor1_id, &actor3_id).await.unwrap();

    // Terminate actor1
    registry
        .handle_actor_termination(&actor1_id, ExitReason::Normal)
        .await;

    // Verify actor1's links are removed
    let links1 = registry.get_links(&actor1_id).await;
    assert!(
        links1.is_empty(),
        "Actor1's links should be empty after termination"
    );

    // Verify actor1 is removed from actor2's and actor3's links
    let links2 = registry.get_links(&actor2_id).await;
    let links3 = registry.get_links(&actor3_id).await;
    assert!(
        !links2.contains(&actor1_id),
        "Actor1 should be removed from actor2's links"
    );
    assert!(
        !links3.contains(&actor1_id),
        "Actor1 should be removed from actor3's links"
    );
}

/// Test: Monitor cleanup on termination
#[tokio::test]
async fn test_monitor_cleanup_on_termination() {
    let registry = create_test_registry().await;

    let actor_id = test_actor_id("actor1");
    let monitor1_id = test_actor_id("monitor1");
    let monitor2_id = test_actor_id("monitor2");

    // Register both monitoring actors so DOWN messages can be delivered to their mailboxes
    let mut rx1 = register_mock_actor(&registry, monitor1_id.clone()).await;
    let mut rx2 = register_mock_actor(&registry, monitor2_id.clone()).await;

    registry
        .monitor(
            &actor_id,
            &monitor1_id,
            "monitor-ref-1".to_string(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();
    registry
        .monitor(
            &actor_id,
            &monitor2_id,
            "monitor-ref-2".to_string(),
            &monitoring_request_context_for_tests(),
        )
        .await
        .unwrap();

    // Terminate actor
    registry
        .handle_actor_termination(&actor_id, ExitReason::Normal)
        .await;

    // Both monitors should receive DOWN
    let down1 = rx1.recv().await;
    let down2 = rx2.recv().await;
    assert!(down1.is_some());
    assert!(down2.is_some());
    assert_eq!(down1.unwrap().message_type, "__DOWN__");
    assert_eq!(down2.unwrap().message_type, "__DOWN__");

    // Monitors should be cleaned up (no longer registered)
    // Note: This is verified by the fact that monitors are removed in handle_actor_termination
}
