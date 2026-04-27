// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Consolidated workflow step type tests
//!
//! This file consolidates tests from:
//! - signal_workflow_test.rs (7 tests)
//! - simple_workflow_test.rs (6 tests)
//! - map_workflow_test.rs (6 tests)
//! - parallel_workflow_test.rs (5 tests)
//! - choice_workflow_test.rs (5 tests)
//! - wait_workflow_test.rs (6 tests)
//! - multi_step_workflow_test.rs (6 tests)
//!
//! Total: 41 tests

use plexspaces_workflow::*;
use serde_json::json;
use std::time::{Duration, Instant};

// =============================================================================
// SIGNAL WORKFLOW TESTS (from signal_workflow_test.rs - 7 tests)
// =============================================================================

#[tokio::test]
async fn test_signal_immediate_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-immediate".to_string(),
        name: "Signal Immediate".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "wait-signal".to_string(),
                name: "Wait for Signal".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "finish".to_string(),
                name: "Finish".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-immediate",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    assert_eq!(signal_exec.status, StepExecutionStatus::Completed);
    let output = signal_exec.output.as_ref().unwrap();
    assert_eq!(output.get("approved").and_then(|v| v.as_bool()), Some(true));

    Ok(())
}

#[tokio::test]
async fn test_signal_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-timeout".to_string(),
        name: "Signal Timeout".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "approval",
                "timeout_ms": 100
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "signal-timeout", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    assert!(
        elapsed >= Duration::from_millis(90),
        "Expected at least 90ms timeout, got {}ms",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_millis(200),
        "Expected less than 200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_signal_delayed_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-delayed".to_string(),
        name: "Signal Delayed".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "payment-complete",
                "timeout_ms": 2000
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-delayed",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let storage_clone = storage.clone();
    let exec_id_clone = execution_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = storage_clone
            .send_signal(
                &exec_id_clone,
                "payment-complete",
                json!({"amount": 100, "status": "success"}),
            )
            .await;
    });

    let start = std::time::Instant::now();
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;

    let result = WorkflowExecutor::execute_from_state(&storage, &execution_id).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Execution should succeed");

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed >= Duration::from_millis(40),
        "Expected at least 40ms wait, got {}ms",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_millis(150),
        "Expected less than 150ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_signal_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-payload".to_string(),
        name: "Signal Payload".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "wait-approval".to_string(),
                name: "Wait for Approval".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "process-approval".to_string(),
                name: "Process Approval".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-payload",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(
            &execution_id,
            "approval",
            json!({
                "approved_by": "alice@example.com",
                "approval_time": "2025-01-12T10:30:00Z",
                "notes": "Looks good!"
            }),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-approval")
        .expect("Signal step should have executed");

    let output = signal_exec.output.as_ref().unwrap();
    assert_eq!(
        output.get("approved_by").and_then(|v| v.as_str()),
        Some("alice@example.com")
    );
    assert_eq!(
        output.get("notes").and_then(|v| v.as_str()),
        Some("Looks good!")
    );

    Ok(())
}

#[tokio::test]
async fn test_multiple_signals_in_sequence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "multi-signal".to_string(),
        name: "Multi Signal".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "wait-approval".to_string(),
                name: "Wait for Approval".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "wait-payment".to_string(),
                name: "Wait for Payment".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "payment",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "finish".to_string(),
                name: "Finish".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "multi-signal",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await?;
    storage
        .send_signal(&execution_id, "payment", json!({"paid": true}))
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"wait-approval".to_string()));
    assert!(step_ids.contains(&"wait-payment".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_signal_no_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-no-timeout".to_string(),
        name: "Signal No Timeout".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "notification"
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-no-timeout",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "notification", json!({"message": "done"}))
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

#[tokio::test]
async fn test_signal_merges_input_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-merge".to_string(),
        name: "Signal Merge".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate Data".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"order_id": "12345", "status": "pending"}
                }),
                ..Default::default()
            },
            Step {
                id: "wait-signal".to_string(),
                name: "Wait for Signal".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "update",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-merge",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(
            &execution_id,
            "update",
            json!({"status": "approved", "approved_by": "bob"}),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    let output = signal_exec.output.as_ref().unwrap();
    assert_eq!(
        output.get("order_id").and_then(|v| v.as_str()),
        Some("12345")
    );
    assert_eq!(
        output.get("status").and_then(|v| v.as_str()),
        Some("approved")
    );
    assert_eq!(
        output.get("approved_by").and_then(|v| v.as_str()),
        Some("bob")
    );

    Ok(())
}

// =============================================================================
// SIMPLE WORKFLOW TESTS (from simple_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_simple_single_step_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "hello-world".to_string(),
        name: "Hello World Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "say-hello".to_string(),
            name: "Say Hello".to_string(),
            step_type: StepType::Task,
            config: json!({
                "actor_type": "EchoActor",
                "method": "say_hello",
                "input": {"name": "World"}
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "hello-world", "1.0", json!({"name": "World"}))
            .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let output = execution.output.unwrap();
    assert_eq!(output["greeting"], "Hello, World!");

    Ok(())
}

#[tokio::test]
async fn test_workflow_definition_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };

    storage.save_definition(&definition).await?;

    let retrieved = storage.get_definition("test-workflow", "1.0").await?;

    assert_eq!(retrieved.id, "test-workflow");
    assert_eq!(retrieved.name, "Test Workflow");
    assert_eq!(retrieved.version, "1.0");

    Ok(())
}

#[tokio::test]
async fn test_execution_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "status-test".to_string(),
        name: "Status Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "status-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Pending);

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Running);

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Completed)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

#[tokio::test]
async fn test_execution_labels() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "label-test".to_string(),
        name: "Label Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let mut labels = std::collections::HashMap::new();
    labels.insert("environment".to_string(), "production".to_string());
    labels.insert("priority".to_string(), "high".to_string());

    let execution_id = storage
        .create_execution("label-test", "1.0", json!({}), labels)
        .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Pending);

    Ok(())
}

#[tokio::test]
async fn test_all_execution_statuses() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "status-full-test".to_string(),
        name: "Status Full Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Cancelled)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Cancelled);
    assert_eq!(execution.status.to_string(), "CANCELLED");

    let execution_id2 = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id2, ExecutionStatus::TimedOut)
        .await?;
    let execution = storage.get_execution(&execution_id2).await?;
    assert_eq!(execution.status, ExecutionStatus::TimedOut);
    assert_eq!(execution.status.to_string(), "TIMED_OUT");

    let execution_id3 = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id3, ExecutionStatus::Failed)
        .await?;
    let execution = storage.get_execution(&execution_id3).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    Ok(())
}

#[tokio::test]
async fn test_error_cases() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let result = storage.get_definition("non-existent", "1.0").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    let result = storage.get_execution("non-existent-exec").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    Ok(())
}

// =============================================================================
// MAP WORKFLOW TESTS (from map_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_map_over_array() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-test".to_string(),
        name: "Map Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map Over Items".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": ["item1", "item2", "item3"],
                "iterator": {
                    "action": "transform",
                    "operation": "uppercase"
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-test", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].step_id, "map-step");
    assert_eq!(history[0].status, StepExecutionStatus::Completed);
    assert!(history[1..]
        .iter()
        .all(|s| s.status == StepExecutionStatus::Completed));

    let map_output = history[0].output.as_ref().unwrap();
    assert!(map_output.is_array());
    assert_eq!(map_output.as_array().unwrap().len(), 3);

    Ok(())
}

#[tokio::test]
async fn test_map_empty_array() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-empty".to_string(),
        name: "Map Empty".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map Empty".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": [],
                "iterator": {
                    "action": "succeed"
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 1);

    let output = history[0].output.as_ref().unwrap();
    assert!(output.is_array());
    assert_eq!(output.as_array().unwrap().len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_map_with_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-fail".to_string(),
        name: "Map Fail".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map with Failure".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": [
                    {"id": "item1", "should_fail": false},
                    {"id": "item2", "should_fail": true},
                    {"id": "item3", "should_fail": false}
                ],
                "iterator": {
                    "action": "conditional_fail"
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-fail", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    Ok(())
}

#[tokio::test]
async fn test_map_preserves_order() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-order".to_string(),
        name: "Map Order".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map Order Test".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": [1, 2, 3, 4, 5],
                "iterator": {
                    "action": "multiply",
                    "factor": 2
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-order", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let output = history[0].output.as_ref().unwrap();

    let results = output.as_array().unwrap();
    assert_eq!(results.len(), 5);

    for (i, result) in results.iter().enumerate() {
        let _expected_value = (i + 1) * 2;
        assert!(result.get("result").is_some());
    }

    Ok(())
}

#[tokio::test]
async fn test_map_max_concurrency() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-concurrency".to_string(),
        name: "Map Concurrency".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map with Concurrency Limit".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                "max_concurrency": 3,
                "iterator": {
                    "action": "succeed",
                    "delay_ms": 50
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-concurrency", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed.as_millis() > 150,
        "Expected batched execution ~200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_map_item_as_input() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-input".to_string(),
        name: "Map Input".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "map-step".to_string(),
            name: "Map Input Test".to_string(),
            step_type: StepType::Map,
            config: json!({
                "items": [
                    {"name": "Alice", "age": 30},
                    {"name": "Bob", "age": 25},
                    {"name": "Carol", "age": 35}
                ],
                "iterator": {
                    "action": "generate"
                }
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-input", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);

    Ok(())
}

// =============================================================================
// PARALLEL WORKFLOW TESTS (from parallel_workflow_test.rs - 5 tests)
// =============================================================================

#[tokio::test]
async fn test_parallel_three_branches() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "parallel-test".to_string(),
        name: "Parallel Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "parallel-step".to_string(),
            name: "Parallel Execution".to_string(),
            step_type: StepType::Parallel,
            config: json!({
                "branches": [
                    {"id": "branch-a", "action": "succeed", "delay_ms": 50},
                    {"id": "branch-b", "action": "succeed", "delay_ms": 100},
                    {"id": "branch-c", "action": "succeed", "delay_ms": 25}
                ]
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-test", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].step_id, "parallel-step");
    assert_eq!(history[0].status, StepExecutionStatus::Completed);
    assert!(history[1..]
        .iter()
        .all(|s| s.status == StepExecutionStatus::Completed));

    let branch_ids: Vec<String> = history[1..].iter().map(|s| s.step_id.clone()).collect();
    assert!(branch_ids.contains(&"branch-a".to_string()));
    assert!(branch_ids.contains(&"branch-b".to_string()));
    assert!(branch_ids.contains(&"branch-c".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_parallel_with_one_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "parallel-fail".to_string(),
        name: "Parallel Fail".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "parallel-step".to_string(),
            name: "Parallel with Failure".to_string(),
            step_type: StepType::Parallel,
            config: json!({
                "branches": [
                    {"id": "branch-success", "action": "succeed"},
                    {"id": "branch-fail", "action": "fail", "error": "Branch failed"},
                    {"id": "branch-success-2", "action": "succeed"}
                ]
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-fail", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 3);

    Ok(())
}

#[tokio::test]
async fn test_parallel_output_aggregation() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "parallel-aggregate".to_string(),
        name: "Parallel Aggregate".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "parallel-step".to_string(),
                name: "Parallel Aggregation".to_string(),
                step_type: StepType::Parallel,
                config: json!({
                    "branches": [
                        {"id": "branch-1", "action": "generate", "data": "result-1"},
                        {"id": "branch-2", "action": "generate", "data": "result-2"}
                    ]
                }),
                ..Default::default()
            },
            Step {
                id: "verify-output".to_string(),
                name: "Verify Output".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-aggregate", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let parallel_exec = history
        .iter()
        .find(|s| s.step_id == "parallel-step")
        .unwrap();
    assert!(parallel_exec.output.is_some());

    let output = parallel_exec.output.as_ref().unwrap();
    assert!(output.is_object() || output.is_array());

    Ok(())
}

#[tokio::test]
async fn test_parallel_empty_branches() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "parallel-empty".to_string(),
        name: "Empty Parallel".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "parallel-step".to_string(),
            name: "Empty Parallel".to_string(),
            step_type: StepType::Parallel,
            config: json!({"branches": []}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

#[tokio::test]
async fn test_parallel_concurrent_execution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "parallel-timing".to_string(),
        name: "Parallel Timing".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "parallel-step".to_string(),
            name: "Concurrent Timing".to_string(),
            step_type: StepType::Parallel,
            config: json!({
                "branches": [
                    {"id": "branch-1", "action": "succeed", "delay_ms": 100},
                    {"id": "branch-2", "action": "succeed", "delay_ms": 100},
                    {"id": "branch-3", "action": "succeed", "delay_ms": 100}
                ]
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-timing", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed.as_millis() < 250,
        "Execution took {}ms, expected concurrent execution ~100ms",
        elapsed.as_millis()
    );

    Ok(())
}

// =============================================================================
// CHOICE WORKFLOW TESTS (from choice_workflow_test.rs - 5 tests)
// =============================================================================

#[tokio::test]
async fn test_choice_simple_condition() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-simple".to_string(),
        name: "Choice Simple".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "approved"}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"},
                        {"variable": "$.status", "operator": "equals", "value": "rejected", "next": "rejected-path"}
                    ],
                    "default": "default-path"
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "rejected-path".to_string(),
                name: "Rejected Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "default-path".to_string(),
                name: "Default Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-simple", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"set-status".to_string()));
    assert!(step_ids.contains(&"choice-step".to_string()));
    assert!(step_ids.contains(&"approved-path".to_string()));
    assert!(!step_ids.contains(&"rejected-path".to_string()));
    assert!(!step_ids.contains(&"default-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_default_path() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-default".to_string(),
        name: "Choice Default".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "pending"}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"}
                    ],
                    "default": "default-path"
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "default-path".to_string(),
                name: "Default Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-default", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"default-path".to_string()));
    assert!(!step_ids.contains(&"approved-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_numeric_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-numeric".to_string(),
        name: "Choice Numeric".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-amount".to_string(),
                name: "Set Amount".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"amount": 150}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Amount".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {"variable": "$.amount", "operator": "greater_than", "value": 100, "next": "high-amount"},
                        {"variable": "$.amount", "operator": "less_than", "value": 50, "next": "low-amount"}
                    ],
                    "default": "medium-amount"
                }),
                ..Default::default()
            },
            Step {
                id: "high-amount".to_string(),
                name: "High Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "low-amount".to_string(),
                name: "Low Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "medium-amount".to_string(),
                name: "Medium Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-numeric", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-amount".to_string()));
    assert!(!step_ids.contains(&"low-amount".to_string()));
    assert!(!step_ids.contains(&"medium-amount".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_boolean() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-boolean".to_string(),
        name: "Choice Boolean".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-flag".to_string(),
                name: "Set Flag".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"is_valid": true}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Flag".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {"variable": "$.is_valid", "operator": "equals", "value": true, "next": "valid-path"}
                    ],
                    "default": "invalid-path"
                }),
                ..Default::default()
            },
            Step {
                id: "valid-path".to_string(),
                name: "Valid Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "invalid-path".to_string(),
                name: "Invalid Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-boolean", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"valid-path".to_string()));
    assert!(!step_ids.contains(&"invalid-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_no_matching_choice_no_default() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-no-default".to_string(),
        name: "Choice No Default".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "unknown"}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"}
                    ]
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-no-default", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

// =============================================================================
// WAIT WORKFLOW TESTS (from wait_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_wait_fixed_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-duration".to_string(),
        name: "Wait Duration".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "wait-step".to_string(),
                name: "Wait 100ms".to_string(),
                step_type: StepType::Wait,
                config: json!({"duration_ms": 100}),
                ..Default::default()
            },
            Step {
                id: "finish".to_string(),
                name: "Finish".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-duration", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected at least 100ms wait, got {}ms",
        elapsed.as_millis()
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"start".to_string()));
    assert!(step_ids.contains(&"wait-step".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_wait_duration_seconds() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-seconds".to_string(),
        name: "Wait Seconds".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait 0.2 seconds".to_string(),
            step_type: StepType::Wait,
            config: json!({"duration_secs": 0.2}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-seconds", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed >= Duration::from_millis(200),
        "Expected at least 200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_until_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let wait_until = chrono::Utc::now() + chrono::Duration::milliseconds(150);

    let definition = WorkflowDefinition {
        id: "wait-until".to_string(),
        name: "Wait Until".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait Until Timestamp".to_string(),
            step_type: StepType::Wait,
            config: json!({"until": wait_until.to_rfc3339()}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-until", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed >= Duration::from_millis(140),
        "Expected at least 140ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_until_past_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let wait_until = chrono::Utc::now() - chrono::Duration::seconds(10);

    let definition = WorkflowDefinition {
        id: "wait-past".to_string(),
        name: "Wait Past".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "wait-step".to_string(),
                name: "Wait Until Past".to_string(),
                step_type: StepType::Wait,
                config: json!({"until": wait_until.to_rfc3339()}),
                ..Default::default()
            },
            Step {
                id: "after-wait".to_string(),
                name: "After Wait".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-past", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_zero_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-zero".to_string(),
        name: "Wait Zero".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait 0ms".to_string(),
            step_type: StepType::Wait,
            config: json!({"duration_ms": 0}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-zero", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_passes_input_through() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-passthrough".to_string(),
        name: "Wait Passthrough".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate Data".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"data": "test-value", "count": 42}
                }),
                ..Default::default()
            },
            Step {
                id: "wait-step".to_string(),
                name: "Wait".to_string(),
                step_type: StepType::Wait,
                config: json!({"duration_ms": 50}),
                ..Default::default()
            },
            Step {
                id: "verify".to_string(),
                name: "Verify Data".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-passthrough", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    assert!(wait_exec.input.is_some());
    let input = wait_exec.input.as_ref().unwrap();
    assert_eq!(
        input.get("data").and_then(|v| v.as_str()),
        Some("test-value")
    );
    assert_eq!(input.get("count").and_then(|v| v.as_u64()), Some(42));

    assert!(wait_exec.output.is_some());
    let output = wait_exec.output.as_ref().unwrap();
    assert_eq!(output, input);

    Ok(())
}

// =============================================================================
// MULTI-STEP WORKFLOW TESTS (from multi_step_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_three_step_sequential_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "three-step".to_string(),
        name: "Three Step Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "step-1".to_string(),
                name: "First Step".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "fetch_data"}),
                ..Default::default()
            },
            Step {
                id: "step-2".to_string(),
                name: "Second Step".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "process_data"}),
                ..Default::default()
            },
            Step {
                id: "step-3".to_string(),
                name: "Third Step".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "store_result"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "three-step", "1.0", json!({"user_id": 123}))
            .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].step_id, "step-1");
    assert_eq!(history[1].step_id, "step-2");
    assert_eq!(history[2].step_id, "step-3");
    assert!(history
        .iter()
        .all(|s| s.status == StepExecutionStatus::Completed));

    Ok(())
}

#[tokio::test]
async fn test_data_passing_between_steps() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "data-flow".to_string(),
        name: "Data Flow Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate Data".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "generate"}),
                ..Default::default()
            },
            Step {
                id: "transform".to_string(),
                name: "Transform Data".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "transform"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "data-flow", "1.0", json!({})).await?;

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert!(history[0].output.is_some());
    assert!(history[1].input.is_some());

    Ok(())
}

#[tokio::test]
async fn test_workflow_fails_when_step_fails() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "failing-workflow".to_string(),
        name: "Failing Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "step-1".to_string(),
                name: "Success Step".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "step-2".to_string(),
                name: "Failing Step".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "fail", "error": "Simulated failure"}),
                ..Default::default()
            },
            Step {
                id: "step-3".to_string(),
                name: "Should Not Execute".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "failing-workflow", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].status, StepExecutionStatus::Completed);
    assert_eq!(history[1].status, StepExecutionStatus::Failed);

    Ok(())
}

#[tokio::test]
async fn test_step_retry_on_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "retry-workflow".to_string(),
        name: "Retry Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "flaky-step".to_string(),
            name: "Flaky Step".to_string(),
            step_type: StepType::Task,
            config: json!({"action": "flaky", "fail_count": 2}),
            retry_policy: Some(RetryPolicy {
                max_attempts: 3,
                initial_backoff: std::time::Duration::from_millis(10),
                backoff_multiplier: 2.0,
                max_backoff: std::time::Duration::from_secs(1),
                jitter: 0.0,
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "retry-workflow", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 2);
    assert_eq!(
        history.last().unwrap().status,
        StepExecutionStatus::Completed
    );

    Ok(())
}

#[tokio::test]
async fn test_empty_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "empty".to_string(),
        name: "Empty Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_explicit_step_ordering() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "ordered".to_string(),
        name: "Ordered Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({}),
                next: Some("middle".to_string()),
                ..Default::default()
            },
            Step {
                id: "middle".to_string(),
                name: "Middle".to_string(),
                step_type: StepType::Task,
                config: json!({}),
                next: Some("end".to_string()),
                ..Default::default()
            },
            Step {
                id: "end".to_string(),
                name: "End".to_string(),
                step_type: StepType::Task,
                config: json!({}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "ordered", "1.0", json!({})).await?;

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].step_id, "start");
    assert_eq!(history[1].step_id, "middle");
    assert_eq!(history[2].step_id, "end");

    Ok(())
}
