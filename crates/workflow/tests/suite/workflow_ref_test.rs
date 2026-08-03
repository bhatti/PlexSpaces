// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// WorkflowRef Integration Tests
//
// Tests the high-level WorkflowRef API with a real workflow actor.

use async_trait::async_trait;
use plexspaces_actor::behavior::Workflow;
use plexspaces_actor::ActorBuilder;
use plexspaces_actor::{
    Actor, ActorContext, BehaviorError, BehaviorType, Message, RequestContext, RequestContextExt,
};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_workflow::WorkflowRef;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// Test Workflow Types
// =============================================================================

/// Simple approval request for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovalRequest {
    document_id: String,
    approvers: Vec<String>,
}

/// Workflow state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ApprovalState {
    document_id: String,
    status: String,
    current_approver_index: usize,
    approvals: Vec<String>,
}

/// Status query response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusResponse {
    document_id: String,
    status: String,
    approvals_completed: usize,
    approvals_total: usize,
}

/// Approval signal payload
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovePayload {
    approver_id: String,
}

// =============================================================================
// Test Workflow Actor
// =============================================================================

/// Simple test workflow for WorkflowRef integration tests
struct TestApprovalWorkflow {
    state: ApprovalState,
    request: Option<ApprovalRequest>,
}

impl TestApprovalWorkflow {
    fn new() -> Self {
        Self {
            state: ApprovalState::default(),
            request: None,
        }
    }
}

#[async_trait]
impl Actor for TestApprovalWorkflow {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Route to Workflow trait methods
        self.route_workflow_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait]
impl Workflow for TestApprovalWorkflow {
    async fn run(&mut self, _ctx: &ActorContext, input: Message) -> Result<Message, BehaviorError> {
        // Parse request
        let request: ApprovalRequest = serde_json::from_slice(&input.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;

        self.request = Some(request.clone());
        self.state.document_id = request.document_id.clone();
        self.state.status = "pending".to_string();

        // Return initial state
        let payload = serde_json::to_vec(&self.state)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }

    async fn signal(
        &mut self,
        _ctx: &ActorContext,
        name: String,
        data: Message,
    ) -> Result<(), BehaviorError> {
        match name.as_str() {
            "approve" => {
                let payload: ApprovePayload =
                    serde_json::from_slice(&data.payload).map_err(|e| {
                        BehaviorError::ProcessingError(format!("Invalid payload: {}", e))
                    })?;

                self.state.approvals.push(payload.approver_id);
                self.state.current_approver_index += 1;

                if let Some(ref request) = self.request {
                    if self.state.current_approver_index >= request.approvers.len() {
                        self.state.status = "approved".to_string();
                    }
                }

                Ok(())
            }
            "reject" => {
                self.state.status = "rejected".to_string();
                Ok(())
            }
            _ => Err(BehaviorError::UnsupportedMessage),
        }
    }

    async fn query(
        &self,
        _ctx: &ActorContext,
        name: String,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        match name.as_str() {
            "status" => {
                let total = self
                    .request
                    .as_ref()
                    .map(|r| r.approvers.len())
                    .unwrap_or(0);
                let response = StatusResponse {
                    document_id: self.state.document_id.clone(),
                    status: self.state.status.clone(),
                    approvals_completed: self.state.current_approver_index,
                    approvals_total: total,
                };

                let payload = serde_json::to_vec(&response)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                Ok(Message {
                    id: ulid::Ulid::new().to_string(),
                    payload,
                    ..Default::default()
                })
            }
            _ => Err(BehaviorError::UnsupportedMessage),
        }
    }
}

// =============================================================================
// Integration Tests
// =============================================================================

/// Test WorkflowRef::run() with typed input/output
#[tokio::test]
async fn test_workflow_ref_run() {
    // Create node
    let node: Node = NodeBuilder::new("test-node")
        .with_clustering_enabled(false)
        .build()
        .await;
    let node: Arc<Node> = Arc::new(node);

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Spawn workflow actor
    let ctx =
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

    let actor_ref = ActorBuilder::new(Box::new(TestApprovalWorkflow::new()))
        .with_name("test-workflow")
        .with_namespace("test")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("Failed to spawn workflow");

    // Create WorkflowRef
    let workflow = WorkflowRef::new(actor_ref);

    // Run workflow with typed input
    let request = ApprovalRequest {
        document_id: "DOC-001".to_string(),
        approvers: vec!["alice".to_string(), "bob".to_string()],
    };

    let result: ApprovalState = workflow
        .run(&ctx, &request)
        .await
        .expect("Failed to run workflow");

    assert_eq!(result.document_id, "DOC-001");
    assert_eq!(result.status, "pending");

    // Cleanup
    node.shutdown(Duration::from_secs(1)).await.ok();
}

/// Test WorkflowRef::signal() and query()
#[tokio::test]
async fn test_workflow_ref_signal_and_query() {
    // Create node
    let node: Node = NodeBuilder::new("test-node-2")
        .with_clustering_enabled(false)
        .build()
        .await;
    let node: Arc<Node> = Arc::new(node);

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Spawn workflow actor
    let ctx =
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

    let actor_ref = ActorBuilder::new(Box::new(TestApprovalWorkflow::new()))
        .with_name("test-workflow-2")
        .with_namespace("test")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("Failed to spawn workflow");

    // Create WorkflowRef
    let workflow = WorkflowRef::new(actor_ref);

    // Run workflow first
    let request = ApprovalRequest {
        document_id: "DOC-002".to_string(),
        approvers: vec!["alice".to_string(), "bob".to_string()],
    };

    let _: ApprovalState = workflow
        .run(&ctx, &request)
        .await
        .expect("Failed to run workflow");

    // Send approval signal
    workflow
        .signal(
            &ctx,
            "approve",
            &ApprovePayload {
                approver_id: "alice".to_string(),
            },
        )
        .await
        .expect("Failed to send signal");

    // Wait for signal to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Query status
    let status: StatusResponse = workflow
        .query(&ctx, "status")
        .await
        .expect("Failed to query status");

    assert_eq!(status.document_id, "DOC-002");
    assert_eq!(status.approvals_completed, 1);
    assert_eq!(status.approvals_total, 2);

    // Send second approval
    workflow
        .signal(
            &ctx,
            "approve",
            &ApprovePayload {
                approver_id: "bob".to_string(),
            },
        )
        .await
        .expect("Failed to send signal");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Query final status
    let final_status: StatusResponse = workflow
        .query(&ctx, "status")
        .await
        .expect("Failed to query status");

    assert_eq!(final_status.status, "approved");
    assert_eq!(final_status.approvals_completed, 2);

    // Cleanup
    node.shutdown(Duration::from_secs(1)).await.ok();
}

/// Test WorkflowRef with per-operation custom timeout
#[tokio::test]
async fn test_workflow_ref_with_timeout() {
    // Create node
    let node: Node = NodeBuilder::new("test-node-3")
        .with_clustering_enabled(false)
        .build()
        .await;
    let node: Arc<Node> = Arc::new(node);

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Spawn workflow actor
    let ctx =
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

    let actor_ref = ActorBuilder::new(Box::new(TestApprovalWorkflow::new()))
        .with_name("test-workflow-3")
        .with_namespace("test")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("Failed to spawn workflow");

    // Create WorkflowRef (no global timeout - per-operation timeouts instead)
    let workflow = WorkflowRef::new(actor_ref);

    let workflow_id = plexspaces_actor::ActorId::from_canonical(workflow.id()).unwrap();
    assert_eq!(workflow_id.name(), "test-workflow-3");
    // namespace comes from RequestContext, not with_namespace() which is overridden by spawn()
    assert_eq!(workflow_id.namespace(), "test-namespace");
    assert_eq!(workflow_id.node_id(), "test-node-3");

    // Run workflow with custom timeout (for long-running workflows)
    let request = ApprovalRequest {
        document_id: "DOC-003".to_string(),
        approvers: vec!["alice".to_string()],
    };

    // Use run_with_timeout for custom timeout
    let result: ApprovalState = workflow
        .run_with_timeout(&ctx, &request, Duration::from_secs(60))
        .await
        .expect("Failed to run workflow");

    assert_eq!(result.document_id, "DOC-003");

    // Query with custom timeout
    let status: StatusResponse = workflow
        .query_with_timeout(&ctx, "status", Duration::from_secs(10))
        .await
        .expect("Failed to query status");

    assert_eq!(status.document_id, "DOC-003");

    // Cleanup
    node.shutdown(Duration::from_secs(1)).await.ok();
}

/// Test WorkflowRef error handling
#[tokio::test]
async fn test_workflow_ref_error_handling() {
    // Create node
    let node: Node = NodeBuilder::new("test-node-4")
        .with_clustering_enabled(false)
        .build()
        .await;
    let node: Arc<Node> = Arc::new(node);

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Spawn workflow actor
    let ctx =
        RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());

    let actor_ref = ActorBuilder::new(Box::new(TestApprovalWorkflow::new()))
        .with_name("test-workflow-4")
        .with_namespace("test")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("Failed to spawn workflow");

    let workflow = WorkflowRef::new(actor_ref);

    // Query without running first should work but return empty state
    let status: StatusResponse = workflow
        .query(&ctx, "status")
        .await
        .expect("Query should work");

    assert_eq!(status.document_id, "");
    assert_eq!(status.status, "");

    // Cleanup
    node.shutdown(Duration::from_secs(1)).await.ok();
}
