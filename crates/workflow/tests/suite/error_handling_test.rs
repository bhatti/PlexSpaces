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

//! Error handling tests for workflow engine
//!
//! ## Purpose
//! Tests error paths and edge cases to achieve 95%+ coverage.
//! These tests target uncovered lines identified by tarpaulin.

use plexspaces_workflow::*;
use serde_json::json;

/// Test Choice step with greater_than operator
///
/// ## Coverage Target
/// Lines 1070-1072 in executor.rs (greater_than operator)
#[tokio::test]
async fn test_choice_greater_than_operator() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-greater-than".to_string(),
        name: "Choice Greater Than".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 150}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "high-value"
                        }
                    ],
                    "default": "low-value"
                }),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-greater-than", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify high-value branch was taken
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-value".to_string()));
    assert!(!step_ids.contains(&"low-value".to_string()));

    Ok(())
}

/// Test Choice step with less_than operator
///
/// ## Coverage Target
/// Lines 1081-1083 in executor.rs (less_than operator)
#[tokio::test]
async fn test_choice_less_than_operator() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-less-than".to_string(),
        name: "Choice Less Than".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 50}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "less_than",
                            "value": 100,
                            "next": "low-value"
                        }
                    ],
                    "default": "high-value"
                }),
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-less-than", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify low-value branch was taken
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Wait step with invalid timestamp format
///
/// ## Coverage Target
/// Lines 1142-1154 in executor.rs (invalid timestamp error path)
#[tokio::test]
async fn test_wait_invalid_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-invalid-timestamp".to_string(),
        name: "Wait Invalid Timestamp".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait Invalid".to_string(),
            step_type: StepType::Wait,
            config: json!({
                "until": "not-a-valid-timestamp"
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-invalid-timestamp", "1.0", json!({}))
            .await?;

    // Workflow should fail due to invalid timestamp
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    // Verify error message
    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    assert_eq!(wait_exec.status, StepExecutionStatus::Failed);
    assert!(wait_exec.error.is_some());
    let error = wait_exec.error.as_ref().unwrap();
    assert!(error.contains("Invalid timestamp format"));

    Ok(())
}

/// Test Signal step with missing signal_name config
///
/// ## Coverage Target
/// Lines 1216-1220 in executor.rs (missing signal_name error)
#[tokio::test]
async fn test_signal_missing_config() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-missing-config".to_string(),
        name: "Signal Missing Config".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                // Missing "signal_name" field
                "timeout_ms": 1000
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "signal-missing-config", "1.0", json!({}))
            .await?;

    // Workflow should fail due to invalid config
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    Ok(())
}

/// Test Wait step with missing duration config
///
/// ## Coverage Target
/// Lines 1158-1169 in executor.rs (missing duration error)
#[tokio::test]
async fn test_wait_missing_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-missing-duration".to_string(),
        name: "Wait Missing Duration".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait Missing Duration".to_string(),
            step_type: StepType::Wait,
            config: json!({
                // Missing duration_ms, duration_secs, and until
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-missing-duration", "1.0", json!({}))
            .await?;

    // Workflow should fail
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    // Verify error message
    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    assert_eq!(wait_exec.status, StepExecutionStatus::Failed);
    assert!(wait_exec.error.is_some());
    let error = wait_exec.error.as_ref().unwrap();
    assert!(error.contains("Wait step requires"));

    Ok(())
}

/// Test Choice step with non-numeric values for numeric operators
///
/// ## Coverage Target
/// Lines 1078, 1085-1087 (comparison with non-numeric values)
#[tokio::test]
async fn test_choice_non_numeric_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-non-numeric".to_string(),
        name: "Choice Non-Numeric".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": "not-a-number"}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "high-value"
                        }
                    ],
                    "default": "low-value"
                }),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-non-numeric", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should take default branch (comparison returns false for non-numeric)
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Choice step with float comparison (greater_than)
///
/// ## Coverage Target
/// Lines 1073-1075 in executor.rs (float greater_than)
#[tokio::test]
async fn test_choice_float_greater_than() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-float-gt".to_string(),
        name: "Choice Float GT".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 150.5}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100.0,
                            "next": "high-value"
                        }
                    ],
                    "default": "low-value"
                }),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-float-gt", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Choice step with float comparison (less_than)
///
/// ## Coverage Target
/// Lines 1084-1086 in executor.rs (float less_than)
#[tokio::test]
async fn test_choice_float_less_than() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-float-lt".to_string(),
        name: "Choice Float LT".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 50.5}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "less_than",
                            "value": 100.0,
                            "next": "low-value"
                        }
                    ],
                    "default": "high-value"
                }),
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-float-lt", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));

    Ok(())
}

/// Test execute_from_state with empty workflow
///
/// ## Coverage Target
/// Lines 118-121 in executor.rs (empty workflow in execute_from_state)
#[tokio::test]
async fn test_execute_from_state_empty_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create empty workflow definition
    let definition = WorkflowDefinition {
        id: "empty-workflow".to_string(),
        name: "Empty Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![], // No steps
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution manually (bypassing start_execution)
    let execution_id = storage
        .create_execution(
            "empty-workflow",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Set to RUNNING state
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;

    // Execute from state (this should complete immediately for empty workflow)
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    // Verify completed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

/// Test Choice step pointing to non-existent next step
///
/// ## Coverage Target
/// Line 212 in executor.rs (choice next step doesn't exist)
#[tokio::test]
async fn test_choice_invalid_next_step() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-invalid-next".to_string(),
        name: "Choice Invalid Next".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 150}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "non-existent-step"  // This step doesn't exist
                        }
                    ]
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-invalid-next", "1.0", json!({}))
            .await?;

    // Workflow should complete (ends when next step doesn't exist)
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

/// Test Choice step with no matching choices and no default
///
/// ## Coverage Target
/// Line 226 in executor.rs (no valid choice - workflow ends)
#[tokio::test]
async fn test_choice_no_match_no_default() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-no-match".to_string(),
        name: "Choice No Match".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 50}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "high-value"
                        }
                    ]
                    // No default specified
                }),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-no-match", "1.0", json!({})).await?;

    // Workflow should complete (no match, no default = workflow ends)
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify high-value step was NOT executed
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test choice-selected step with invalid next
///
/// ## Coverage Target
/// Lines 230-234 in executor.rs (choice path with invalid next)
#[tokio::test]
async fn test_choice_path_invalid_next() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-path-invalid".to_string(),
        name: "Choice Path Invalid".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"value": 150}}),
                ..Default::default()
            },
            Step {
                id: "check-value".to_string(),
                name: "Check Value".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.value",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "high-value"
                        }
                    ],
                    "default": "low-value"
                }),
                ..Default::default()
            },
            Step {
                id: "high-value".to_string(),
                name: "High Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                next: Some("non-existent-step".to_string()), // Invalid next
                ..Default::default()
            },
            Step {
                id: "low-value".to_string(),
                name: "Low Value".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-path-invalid", "1.0", json!({}))
            .await?;

    // Workflow should complete (ends when choice-selected step has invalid next)
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}
