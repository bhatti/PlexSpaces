// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

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

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({"input": "test"}), HashMap::new())
        .await
        .unwrap();

    // Get execution
    let execution = storage.get_execution(&execution_id).await.unwrap();

    assert_eq!(execution.execution_id, execution_id);
    assert_eq!(execution.definition_id, "test-workflow");
    assert_eq!(execution.status, ExecutionStatus::Pending);
    assert_eq!(execution.version, 1);
}

#[tokio::test]
async fn test_create_execution_with_node() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution with node ownership
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

    // Get execution
    let execution = storage.get_execution(&execution_id).await.unwrap();

    assert_eq!(execution.node_id, Some("node-1".to_string()));
    assert!(execution.last_heartbeat.is_some());
}

#[tokio::test]
async fn test_update_execution_status() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Update status
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await
        .unwrap();

    // Verify
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.status, ExecutionStatus::Running);
    assert_eq!(execution.version, 2); // Version incremented
}

#[tokio::test]
async fn test_optimistic_locking_version_check() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    let version = execution.version;

    // Update with correct version (should succeed)
    storage
        .update_execution_status_with_version(&execution_id, ExecutionStatus::Running, Some(version))
        .await
        .unwrap();

    // Update with wrong version (should fail)
    let result = storage
        .update_execution_status_with_version(&execution_id, ExecutionStatus::Completed, Some(version))
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_transfer_ownership() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution with node-1
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
    assert_eq!(execution.node_id, Some("node-1".to_string()));

    // Transfer to node-2
    storage
        .transfer_ownership(&execution_id, "node-2", execution.version)
        .await
        .unwrap();

    // Verify
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, Some("node-2".to_string()));
    assert_eq!(execution.version, 2); // Version incremented
}

#[tokio::test]
async fn test_transfer_ownership_optimistic_locking() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
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
    let version = execution.version;

    // Transfer with correct version (should succeed)
    storage
        .transfer_ownership(&execution_id, "node-2", version)
        .await
        .unwrap();

    // Transfer with wrong version (should fail)
    let result = storage
        .transfer_ownership(&execution_id, "node-3", version)
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_update_heartbeat() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution with node
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

    let execution1 = storage.get_execution(&execution_id).await.unwrap();
    let heartbeat1 = execution1.last_heartbeat;

    // Wait a bit to ensure timestamp difference
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Update heartbeat
    storage
        .update_heartbeat(&execution_id, "node-1")
        .await
        .unwrap();

    // Verify heartbeat updated
    let execution2 = storage.get_execution(&execution_id).await.unwrap();
    assert!(execution2.last_heartbeat.is_some());
    if let (Some(hb1), Some(hb2)) = (heartbeat1, execution2.last_heartbeat) {
        // Allow for small timing differences, but hb2 should be >= hb1
        assert!(hb2 >= hb1);
    } else {
        // If heartbeat1 was None, then heartbeat2 should be Some
        assert!(execution2.last_heartbeat.is_some());
    }
}

#[tokio::test]
async fn test_list_executions_by_status() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definitions first (required by foreign key)
    let def1 = WorkflowDefinition {
        id: "workflow-1".to_string(),
        name: "Workflow 1".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&def1).await.unwrap();

    let def2 = WorkflowDefinition {
        id: "workflow-2".to_string(),
        name: "Workflow 2".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&def2).await.unwrap();

    // Create multiple executions
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

    // Update one to RUNNING
    storage
        .update_execution_status(&exec1, ExecutionStatus::Running)
        .await
        .unwrap();

    // List RUNNING executions
    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::Running], None)
        .await
        .unwrap();

    assert_eq!(running.len(), 1);
    assert_eq!(running[0].execution_id, exec1);

    // List PENDING executions
    let pending = storage
        .list_executions_by_status(vec![ExecutionStatus::Pending], None)
        .await
        .unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].execution_id, exec2);
}

#[tokio::test]
async fn test_list_executions_by_node() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definitions first (required by foreign key)
    let def1 = WorkflowDefinition {
        id: "workflow-1".to_string(),
        name: "Workflow 1".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&def1).await.unwrap();

    let def2 = WorkflowDefinition {
        id: "workflow-2".to_string(),
        name: "Workflow 2".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&def2).await.unwrap();

    // Create executions for different nodes
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

    // List executions for node-1
    let node1_execs = storage
        .list_executions_by_status(vec![ExecutionStatus::Pending], Some("node-1"))
        .await
        .unwrap();

    assert_eq!(node1_execs.len(), 1);
    assert_eq!(node1_execs[0].execution_id, exec1);
}

#[tokio::test]
async fn test_list_stale_executions() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "workflow-1".to_string(),
        name: "Workflow 1".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution and mark as RUNNING
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
        .update_execution_status(&exec1, ExecutionStatus::Running)
        .await
        .unwrap();

    // Should not be stale immediately
    let stale = storage
        .list_stale_executions(300, vec![ExecutionStatus::Running])
        .await
        .unwrap();

    assert_eq!(stale.len(), 0);

    // Note: Testing actual stale detection would require manipulating timestamps
    // or waiting 300+ seconds, which is impractical in unit tests
    // This test verifies the query works correctly
}

#[tokio::test]
async fn test_save_and_get_definition() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };

    // Save definition
    storage.save_definition(&definition).await.unwrap();

    // Get definition
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

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Send signal
    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await
        .unwrap();

    // Check signal
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

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Create step execution
    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({"input": "test"}))
        .await
        .unwrap();

    // Complete step execution
    storage
        .complete_step_execution(
            &step_exec_id,
            StepExecutionStatus::Completed,
            Some(json!({"output": "result"})),
            None,
        )
        .await
        .unwrap();

    // Get step execution
    let step_exec = storage.get_step_execution(&step_exec_id).await.unwrap();

    assert_eq!(step_exec.status, StepExecutionStatus::Completed);
    assert_eq!(step_exec.attempt, 1);
}

#[tokio::test]
async fn test_step_execution_with_retry() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Create step execution with attempt 1
    let step_exec_id1 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 1)
        .await
        .unwrap();

    // Fail attempt 1
    storage
        .complete_step_execution(
            &step_exec_id1,
            StepExecutionStatus::Failed,
            None,
            Some("Error".to_string()),
        )
        .await
        .unwrap();

    // Create step execution with attempt 2
    let step_exec_id2 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 2)
        .await
        .unwrap();

    // Succeed attempt 2
    storage
        .complete_step_execution(
            &step_exec_id2,
            StepExecutionStatus::Completed,
            Some(json!({"result": "success"})),
            None,
        )
        .await
        .unwrap();

    // Get step execution history
    let history = storage
        .get_step_execution_history(&execution_id)
        .await
        .unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].attempt, 1);
    assert_eq!(history[0].status, StepExecutionStatus::Failed);
    assert_eq!(history[1].attempt, 2);
    assert_eq!(history[1].status, StepExecutionStatus::Completed);
}

#[tokio::test]
async fn test_update_execution_output() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Update output
    storage
        .update_execution_output(&execution_id, json!({"result": "success"}))
        .await
        .unwrap();

    // Verify
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.output, Some(json!({"result": "success"})));
    assert_eq!(execution.version, 2); // Version incremented
}

#[tokio::test]
async fn test_concurrent_update_detection() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    let version = execution.version;

    // Simulate concurrent update: Node A reads version 1
    // Node B updates to version 2
    storage
        .update_execution_status_with_version(&execution_id, ExecutionStatus::Running, Some(version))
        .await
        .unwrap();

    // Node A tries to update with stale version (should fail)
    let result = storage
        .update_execution_status_with_version(&execution_id, ExecutionStatus::Completed, Some(version))
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_update_execution_output_with_version() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition first (required by foreign key)
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    let execution = storage.get_execution(&execution_id).await.unwrap();
    let version = execution.version;

    // Update output with correct version (should succeed)
    storage
        .update_execution_output_with_version(&execution_id, json!({"result": "success"}), Some(version))
        .await
        .unwrap();

    // Verify
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.output, Some(json!({"result": "success"})));
    assert_eq!(execution.version, 2); // Version incremented

    // Update with wrong version (should fail)
    let result = storage
        .update_execution_output_with_version(&execution_id, json!({"result": "fail"}), Some(version))
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_recovery_scenario_node_crash() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution on node-1 and mark as RUNNING
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
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await
        .unwrap();

    // Simulate node-1 crash: node-2 tries to recover
    // First, find RUNNING executions
    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::Running], Some("node-1"))
        .await
        .unwrap();

    assert_eq!(running.len(), 1);
    assert_eq!(running[0].execution_id, execution_id);

    // Node-2 transfers ownership (recovery)
    let execution = storage.get_execution(&execution_id).await.unwrap();
    let version_before_transfer = execution.version;
    storage
        .transfer_ownership(&execution_id, "node-2", execution.version)
        .await
        .unwrap();

    // Verify ownership transferred
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert_eq!(execution.node_id, Some("node-2".to_string()));
    assert_eq!(execution.version, version_before_transfer + 1); // Version incremented
}

#[tokio::test]
async fn test_recovery_scenario_concurrent_ownership_transfer() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution on node-1
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
    let version = execution.version;

    // Node-2 transfers ownership (should succeed)
    storage
        .transfer_ownership(&execution_id, "node-2", version)
        .await
        .unwrap();

    // Node-3 tries to transfer with stale version (should fail)
    let result = storage
        .transfer_ownership(&execution_id, "node-3", version)
        .await;

    assert!(matches!(result, Err(WorkflowError::ConcurrentUpdate(_))));
}

#[tokio::test]
async fn test_heartbeat_updates_health_monitoring() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution on node-1 and mark as RUNNING
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
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await
        .unwrap();

    // Update heartbeat multiple times
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        storage
            .update_heartbeat(&execution_id, "node-1")
            .await
            .unwrap();
    }

    // Verify heartbeat is updated
    let execution = storage.get_execution(&execution_id).await.unwrap();
    assert!(execution.last_heartbeat.is_some());
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

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Send multiple signals with same name
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

    // Check signals (should get first one, FIFO)
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

    // No more signals
    let signal4 = storage
        .check_signal(&execution_id, "approval")
        .await
        .unwrap();
    assert!(signal4.is_none());
}

#[tokio::test]
async fn test_list_executions_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // List executions when none exist
    let running = storage
        .list_executions_by_status(vec![ExecutionStatus::Running], None)
        .await
        .unwrap();

    assert_eq!(running.len(), 0);
}

#[tokio::test]
async fn test_list_stale_executions_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // List stale executions when none exist
    let stale = storage
        .list_stale_executions(300, vec![ExecutionStatus::Running])
        .await
        .unwrap();

    assert_eq!(stale.len(), 0);
}

#[tokio::test]
async fn test_step_execution_history_empty() {
    let storage = WorkflowStorage::new_in_memory().await.unwrap();

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await.unwrap();

    // Create execution
    let execution_id = storage
        .create_execution("test-workflow", "1.0", json!({}), HashMap::new())
        .await
        .unwrap();

    // Get step execution history (should be empty)
    let history = storage
        .get_step_execution_history(&execution_id)
        .await
        .unwrap();

    assert_eq!(history.len(), 0);
}

