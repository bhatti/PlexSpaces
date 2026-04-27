// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for distributed supervision
//!
//! Tests validate Node.monitor() functionality following Erlang's location-transparent
//! monitoring philosophy.  DOWN notifications are delivered as `__DOWN__` messages into
//! the supervisor actor's mailbox — no separate notification channel.

use plexspaces_core::{ExitReason, ServiceLocator};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{Node, NodeBuilder};
use std::sync::Arc;
use std::time::Duration;

use super::test_helpers::{register_actor_with_message_sender, test_runtime_actor_id};

// ─── helpers ─────────────────────────────────────────────────────────────────

async fn node_request_context(node: &Node) -> plexspaces_core::RequestContext {
    node.service_locator()
        .request_context_for_system_operations()
        .await
}

/// Register a supervisor actor that has its own mailbox so DOWN messages can land.
async fn register_supervisor(
    node: &Node,
    supervisor_id: &plexspaces_core::ActorId,
) -> Arc<Mailbox> {
    let mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), supervisor_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(node, supervisor_id, mailbox.clone()).await;
    mailbox
}

/// Wait up to `deadline` for a `__DOWN__` message to appear in `mailbox`.
async fn wait_for_down(
    mailbox: &Mailbox,
    deadline: Duration,
) -> Option<plexspaces_proto::common::v1::Message> {
    let start = tokio::time::Instant::now();
    loop {
        let remaining = deadline.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return None;
        }
        let poll_timeout = remaining.min(Duration::from_millis(50));
        if let Some(msg) = mailbox.dequeue_with_timeout(Some(poll_timeout)).await {
            if msg.message_type == "__DOWN__"
                || msg.headers.get("type").map_or(false, |v| v == "__DOWN__")
            {
                return Some(msg);
            }
        }
        if start.elapsed() >= deadline {
            return None;
        }
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// Test 1: Monitor local actor — `monitor_ref` is returned without error.
#[tokio::test]
async fn test_monitor_local_actor() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    // Register worker so monitoring can proceed.
    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;

    let ctx = node_request_context(&node).await;
    let monitor_ref = node.monitor(&worker_id, &supervisor_id, &ctx).await;
    assert!(monitor_ref.is_ok(), "Monitoring local actor should succeed");
    assert!(
        !monitor_ref.unwrap().is_empty(),
        "Monitor ref should not be empty"
    );
}

/// Test 2: Monitor non-existent actor — should fail.
#[tokio::test]
async fn test_monitor_nonexistent_actor() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let ctx = node_request_context(&node).await;
    let result = node
        .monitor(
            &test_runtime_actor_id("nonexistent", "node1"),
            &test_runtime_actor_id("supervisor", "node1"),
            &ctx,
        )
        .await;

    assert!(result.is_err(), "Monitoring non-existent actor should fail");
}

/// Test 3: Local actor terminates → supervisor receives `__DOWN__` in mailbox.
#[tokio::test]
async fn test_local_actor_termination_down_message() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    // Register both actors.
    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    let supervisor_mailbox = register_supervisor(&node, &supervisor_id).await;

    // Establish monitor.
    let ctx = node_request_context(&node).await;
    node.monitor(&worker_id, &supervisor_id, &ctx)
        .await
        .unwrap();

    // Terminate the worker.
    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(&worker_id, ExitReason::Normal)
        .await;

    // Supervisor's mailbox should receive a __DOWN__ message.
    let down = wait_for_down(&supervisor_mailbox, Duration::from_millis(500)).await;
    assert!(down.is_some(), "Supervisor must receive __DOWN__ message");

    let msg = down.unwrap();
    assert_eq!(
        msg.headers.get("down_from").map(|s| s.as_str()),
        Some(worker_id.to_string().as_str()),
        "down_from header must match terminated actor"
    );
    assert_eq!(
        msg.headers.get("down_reason").map(|s| s.as_str()),
        Some("normal"),
        "down_reason header must be 'normal'"
    );
}

/// Test 4: Multiple supervisors monitoring the same actor — both get __DOWN__.
#[tokio::test]
async fn test_multiple_monitors_same_actor() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let sup1_id = test_runtime_actor_id("supervisor1", "node1");
    let sup2_id = test_runtime_actor_id("supervisor2", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    let sup1_mailbox = register_supervisor(&node, &sup1_id).await;
    let sup2_mailbox = register_supervisor(&node, &sup2_id).await;

    let ctx = node_request_context(&node).await;
    node.monitor(&worker_id, &sup1_id, &ctx).await.unwrap();
    node.monitor(&worker_id, &sup2_id, &ctx).await.unwrap();

    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(&worker_id, ExitReason::Error("crash".to_string()))
        .await;

    let down1 = wait_for_down(&sup1_mailbox, Duration::from_millis(500)).await;
    let down2 = wait_for_down(&sup2_mailbox, Duration::from_millis(500)).await;

    assert!(down1.is_some(), "Supervisor 1 must receive __DOWN__");
    assert!(down2.is_some(), "Supervisor 2 must receive __DOWN__");

    assert_eq!(
        down1
            .unwrap()
            .headers
            .get("down_reason")
            .map(|s| s.as_str()),
        Some("crash")
    );
    assert_eq!(
        down2
            .unwrap()
            .headers
            .get("down_reason")
            .map(|s| s.as_str()),
        Some("crash")
    );
}

/// Test 5: Monitor refs are unique per monitor() call.
#[tokio::test]
async fn test_monitor_ref_uniqueness() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let sup1_id = test_runtime_actor_id("supervisor1", "node1");
    let sup2_id = test_runtime_actor_id("supervisor2", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    register_supervisor(&node, &sup1_id).await;
    register_supervisor(&node, &sup2_id).await;

    let ctx = node_request_context(&node).await;
    let mon1 = node.monitor(&worker_id, &sup1_id, &ctx).await.unwrap();
    let mon2 = node.monitor(&worker_id, &sup2_id, &ctx).await.unwrap();

    assert_ne!(mon1, mon2, "Monitor refs should be unique");
}

/// Test 6: Crash reason is propagated verbatim in `down_reason` header.
#[tokio::test]
async fn test_actor_crash_reason_propagation() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    let supervisor_mailbox = register_supervisor(&node, &supervisor_id).await;

    let ctx = node_request_context(&node).await;
    node.monitor(&worker_id, &supervisor_id, &ctx)
        .await
        .unwrap();

    let crash_reason = "panic: index out of bounds at line 42";
    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(&worker_id, ExitReason::Error(crash_reason.to_string()))
        .await;

    let down = wait_for_down(&supervisor_mailbox, Duration::from_millis(500)).await;
    assert!(down.is_some(), "Should receive __DOWN__");

    let reason = down
        .unwrap()
        .headers
        .get("down_reason")
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        reason, crash_reason,
        "Crash reason must be propagated exactly"
    );
}

/// Test 7: demonitor cancels the watch — no __DOWN__ after demonitor.
#[tokio::test]
async fn test_demonitor_cancels_down_notification() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    let supervisor_mailbox = register_supervisor(&node, &supervisor_id).await;

    // Establish and then immediately cancel the monitor.
    let ctx = node_request_context(&node).await;
    let monitor_ref = node
        .monitor(&worker_id, &supervisor_id, &ctx)
        .await
        .unwrap();

    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .demonitor(&worker_id, &supervisor_id, &monitor_ref)
        .await
        .unwrap();

    // Terminate the worker — no DOWN should arrive because we demonitored.
    actor_registry
        .handle_actor_termination(&worker_id, ExitReason::Normal)
        .await;

    let down = wait_for_down(&supervisor_mailbox, Duration::from_millis(200)).await;
    assert!(
        down.is_none(),
        "demonitor must prevent __DOWN__ delivery after cancellation"
    );
}

/// Test 8: monitor — __DOWN__ fires on Shutdown exit (all exits trigger DOWN).
#[tokio::test]
async fn test_monitor_down_on_shutdown() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    let supervisor_mailbox = register_supervisor(&node, &supervisor_id).await;

    let ctx = node_request_context(&node).await;
    node.monitor(&worker_id, &supervisor_id, &ctx)
        .await
        .unwrap();

    // Terminate with Shutdown — monitors receive __DOWN__ for all exit kinds.
    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    actor_registry
        .handle_actor_termination(&worker_id, ExitReason::Shutdown)
        .await;

    let down = wait_for_down(&supervisor_mailbox, Duration::from_millis(500)).await;
    assert!(
        down.is_some(),
        "__DOWN__ must be delivered even on Shutdown exit"
    );
    assert_eq!(
        down.unwrap().headers.get("down_reason").map(|s| s.as_str()),
        Some("shutdown"),
    );
}

/// Test 9: Multiple demonitor calls for the same ref are idempotent.
#[tokio::test]
async fn test_demonitor_idempotent() {
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    let worker_id = test_runtime_actor_id("worker", "node1");
    let supervisor_id = test_runtime_actor_id("supervisor", "node1");

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node, &worker_id, worker_mailbox.clone()).await;
    register_supervisor(&node, &supervisor_id).await;

    let ctx = node_request_context(&node).await;
    let monitor_ref = node
        .monitor(&worker_id, &supervisor_id, &ctx)
        .await
        .unwrap();

    let actor_registry = node.service_locator().actor_registry().await.unwrap();
    // First demonitor — should succeed.
    actor_registry
        .demonitor(&worker_id, &supervisor_id, &monitor_ref)
        .await
        .unwrap();
    // Second demonitor — must also succeed (idempotent, no panic/error).
    actor_registry
        .demonitor(&worker_id, &supervisor_id, &monitor_ref)
        .await
        .unwrap();
}
