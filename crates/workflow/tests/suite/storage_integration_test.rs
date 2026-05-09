// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration tests for WorkflowStorage
//!
//! Tests all SQL operations with SQLite to ensure:
//! - All queries work correctly
//! - Optimistic locking prevents race conditions
//! - Node ownership transfer works
//! - Health monitoring (heartbeat) works
//! - Stale workflow detection works

use plexspaces_workflow::*;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

#[tokio::test]
async fn test_create_and_get_execution() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution(
            "test-workflow",
            "1.0",
            json!({"input": "test"}),
            HashMap::new(),
        )
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();

    assert_eq!(execution.execution_id, execution_id);
    assert_eq!(execution.definition_id, "test-workflow");
    assert_eq!(execution.execution_status(), ExecutionStatus::ExecutionStatusPending);
}

#[tokio::test]
async fn test_create_execution_with_node() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({"input": "test"}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();

    assert_eq!(execution.node_id, "node-1");
}

#[tokio::test]
async fn test_update_execution_status() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.execution_status(), ExecutionStatus::ExecutionStatusRunning);
}

#[tokio::test]
async fn test_optimistic_locking_version_check() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Update with correct version (version=1) should succeed
    storage
        .update_execution_status_with_version(
            &execution_id,
            ExecutionStatus::ExecutionStatusRunning,
            Some(1),
        )
        .await
        .unwrap();

    // Update with wrong version (still 1 but now version is 2) should fail
    let result = storage
        .update_execution_status_with_version(
            &execution_id,
            ExecutionStatus::ExecutionStatusCompleted,
            Some(1),
        )
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_transfer_ownership() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, "node-1");

    // Transfer to node-2 (version=1)
    storage
        .transfer_ownership(&execution_id, "node-2", 1)
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, "node-2");
}

#[tokio::test]
async fn test_transfer_ownership_optimistic_locking() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    // Transfer with version=1 (should succeed)
    storage
        .transfer_ownership(&execution_id, "node-2", 1)
        .await
        .unwrap();

    // Transfer with stale version=1 (should fail, current version is now 2)
    let result = storage
        .transfer_ownership(&execution_id, "node-3", 1)
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_update_heartbeat() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Update heartbeat — should succeed without error
    storage
        .update_heartbeat(&execution_id, "node-1")
        .await
        .unwrap();

    // Verify execution still exists and is owned by node-1
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, "node-1");
}

#[tokio::test]
async fn test_list_executions_by_status() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let def1 = make_workflow_definition("workflow-1", "Workflow 1", "1.0", vec![]);
    storage.save_definition(&def1).await.unwrap();

    let def2 = make_workflow_definition("workflow-2", "Workflow 2", "1.0", vec![]);
    storage.save_definition(&def2).await.unwrap();

    let exec1 = storage
        .create_execution_with_node(
            "workflow-1",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    let exec2 = storage
        .create_execution_with_node(
            "workflow-2",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    storage
        .update_execution_status(&exec1, ExecutionStatus::ExecutionStatusRunning)
        .await
        .unwrap();

    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::ExecutionStatusRunning], None)
        .await
        .unwrap();

    assert_eq!(running.len(), 1);
    assert_eq!(running[0].execution_id, exec1);

    let pending = storage
        .list_executions_by_status(vec![ExecutionStatus::ExecutionStatusPending], None)
        .await
        .unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].execution_id, exec2);
}

#[tokio::test]
async fn test_list_executions_by_node() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let def1 = make_workflow_definition("workflow-1", "Workflow 1", "1.0", vec![]);
    storage.save_definition(&def1).await.unwrap();

    let def2 = make_workflow_definition("workflow-2", "Workflow 2", "1.0", vec![]);
    storage.save_definition(&def2).await.unwrap();

    let exec1 = storage
        .create_execution_with_node(
            "workflow-1",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    let _exec2 = storage
        .create_execution_with_node(
            "workflow-2",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-2"),
        )
        .await
        .unwrap();

    let node1_execs = storage
        .list_executions_by_status(vec![ExecutionStatus::ExecutionStatusPending], Some("node-1"))
        .await
        .unwrap();

    assert_eq!(node1_execs.len(), 1);
    assert_eq!(node1_execs[0].execution_id, exec1);
}

#[tokio::test]
async fn test_list_stale_executions() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("workflow-1", "Workflow 1", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let exec1 = storage
        .create_execution_with_node(
            "workflow-1",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    storage
        .update_execution_status(&exec1, ExecutionStatus::ExecutionStatusRunning)
        .await
        .unwrap();

    // Should not be stale immediately
    let stale = storage
        .list_stale_executions(300, vec![ExecutionStatus::ExecutionStatusRunning])
        .await
        .unwrap();

    assert_eq!(stale.len(), 0);
}

#[tokio::test]
async fn test_save_and_get_definition() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);

    storage.save_definition(&definition).await.unwrap();

    let retrieved = storage
        .get_definition("test-workflow", "1.0")
        .await
        .unwrap();

    assert_eq!(retrieved.id, "test-workflow");
    assert_eq!(retrieved.name, "Test Workflow");
}

#[tokio::test]
async fn test_send_and_check_signal() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await
        .unwrap();

    let signal = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();

    assert!(signal.is_some());
    assert_eq!(signal.unwrap()["approved"], true);

    // Signal should be consumed (removed)
    let signal2 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();

    assert!(signal2.is_none());
}

#[tokio::test]
async fn test_step_execution_lifecycle() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({"input": "test"}))
        .await
        .unwrap();

    storage
        .complete_step_execution(
            &step_exec_id,
            StepStatus::StepStatusCompleted,
            Some(json!({"output": "result"})),
            None,
        )
        .await
        .unwrap();

    let step_exec = storage.get_step_execution(&step_exec_id).await.unwrap();

    assert_eq!(step_exec.step_status(), StepStatus::StepStatusCompleted);
    assert_eq!(step_exec.attempt, 1);
}

#[tokio::test]
async fn test_step_execution_with_retry() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let step_exec_id1 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 1)
        .await
        .unwrap();

    storage
        .complete_step_execution(
            &step_exec_id1,
            StepStatus::StepStatusFailed,
            None,
            Some("Error".to_string()),
        )
        .await
        .unwrap();

    let step_exec_id2 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 2)
        .await
        .unwrap();

    storage
        .complete_step_execution(
            &step_exec_id2,
            StepStatus::StepStatusCompleted,
            Some(json!({"result": "success"})),
            None,
        )
        .await
        .unwrap();

    let history = storage
        .get_step_execution_history(&execution_id)
        .await
        .unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].attempt, 1);
    assert_eq!(history[0].step_status(), StepStatus::StepStatusFailed);
    assert_eq!(history[1].attempt, 2);
    assert_eq!(history[1].step_status(), StepStatus::StepStatusCompleted);
}

#[tokio::test]
async fn test_update_execution_output() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    storage
        .update_execution_output(&execution_id, json!({"result": "success"}))
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    let output = execution.output_value().expect("Execution should have output");
    assert_eq!(output["result"], "success");
}

#[tokio::test]
async fn test_concurrent_update_detection() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Node A updates with version 1 (should succeed)
    storage
        .update_execution_status_with_version(
            &execution_id,
            ExecutionStatus::ExecutionStatusRunning,
            Some(1),
        )
        .await
        .unwrap();

    // Node A tries to update again with stale version (should fail)
    let result = storage
        .update_execution_status_with_version(
            &execution_id,
            ExecutionStatus::ExecutionStatusCompleted,
            Some(1),
        )
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_update_execution_output_with_version() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Update output with correct version (should succeed)
    storage
        .update_execution_output_with_version(
            &execution_id,
            json!({"result": "success"}),
            Some(1),
        )
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    let output = execution.output_value().expect("Execution should have output");
    assert_eq!(output["result"], "success");

    // Update with wrong version (should fail)
    let result = storage
        .update_execution_output_with_version(
            &execution_id,
            json!({"result": "fail"}),
            Some(1),
        )
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_recovery_scenario_node_crash() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await
        .unwrap();

    // Find RUNNING executions on node-1
    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::ExecutionStatusRunning], Some("node-1"))
        .await
        .unwrap();

    assert_eq!(running.len(), 1);
    assert_eq!(running[0].execution_id, execution_id);

    // Node-2 transfers ownership (recovery) with version=2 (after update_execution_status)
    storage
        .transfer_ownership(&execution_id, "node-2", 2)
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, "node-2");
}

#[tokio::test]
async fn test_recovery_scenario_concurrent_ownership_transfer() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    // Node-2 transfers ownership with version=1 (should succeed)
    storage
        .transfer_ownership(&execution_id, "node-2", 1)
        .await
        .unwrap();

    // Node-3 tries to transfer with stale version=1 (should fail)
    let result = storage
        .transfer_ownership(&execution_id, "node-3", 1)
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_heartbeat_updates_health_monitoring() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution_with_node(
            "test-workflow",
            "1.0",
            json!({}),
            HashMap::new(),
            Some("node-1"),
        )
        .await
        .unwrap();

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await
        .unwrap();

    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        storage
            .update_heartbeat(&execution_id, "node-1")
            .await
            .unwrap();
    }

    // Verify execution is still running
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.execution_status(), ExecutionStatus::ExecutionStatusRunning);
}

#[tokio::test]
async fn test_get_nonexistent_execution() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let result = storage.get_execution("nonexistent-id").await;
    assert!(matches!(result, Err(WorkflowError::NotFound(_))));
}

#[tokio::test]
async fn test_get_nonexistent_definition() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let result = storage.get_definition("nonexistent", "1.0").await;
    assert!(matches!(result, Err(WorkflowError::NotFound(_))));
}

#[tokio::test]
async fn test_get_nonexistent_step_execution() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let result = storage.get_step_execution("nonexistent-id").await;
    assert!(matches!(result, Err(WorkflowError::NotFound(_))));
}

#[tokio::test]
async fn test_multiple_signals_same_name() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    storage
        .send_signal(&execution_id, "approval", json!({"id": 1}))
        .await
        .unwrap();
    storage
        .send_signal(&execution_id, "approval", json!({"id": 2}))
        .await
        .unwrap();
    storage
        .send_signal(&execution_id, "approval", json!({"id": 3}))
        .await
        .unwrap();

    let signal1 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();
    assert_eq!(signal1.unwrap()["id"], 1);

    let signal2 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();
    assert_eq!(signal2.unwrap()["id"], 2);

    let signal3 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();
    assert_eq!(signal3.unwrap()["id"], 3);

    let signal4 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();
    assert!(signal4.is_none());
}

#[tokio::test]
async fn test_list_executions_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::ExecutionStatusRunning], None)
        .await
        .unwrap();

    assert_eq!(running.len(), 0);
}

#[tokio::test]
async fn test_list_stale_executions_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let stale = storage
        .list_stale_executions(300, vec![ExecutionStatus::ExecutionStatusRunning])
        .await
        .unwrap();

    assert_eq!(stale.len(), 0);
}

#[tokio::test]
async fn test_step_execution_history_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);
    storage.save_definition(&definition).await.unwrap();

    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let history = storage
        .get_step_execution_history(&execution_id)
        .await
        .unwrap();

    assert_eq!(history.len(), 0);
}
