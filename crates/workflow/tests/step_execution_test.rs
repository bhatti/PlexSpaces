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

//! Step execution tracking tests
//!
//! ## Purpose
//! Tests for tracking individual step executions within workflows.
//! Following TDD principles - these tests are written FIRST.

use plexspaces_workflow::*;
use serde_json::json;

/// Test recording step execution start
///
/// ## TDD Test
/// This test will FAIL until we implement create_step_execution()
#[tokio::test]
async fn test_record_step_execution_start() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow definition
    let definition = WorkflowDefinition {
        id: "step-test".to_string(),
        name: "Step Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "step-1".to_string(),
            name: "First Step".to_string(),
            step_type: StepType::Task,
            config: json!({}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution
    let execution_id = storage
        .create_execution(
            "step-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Record step execution start
    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({"input": "test"}))
        .await?;

    // Verify step execution was created
    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.step_id, "step-1");
    assert_eq!(step_exec.execution_id, execution_id);
    assert_eq!(step_exec.status, StepExecutionStatus::Running);
    assert_eq!(step_exec.attempt, 1);

    Ok(())
}

/// Test recording step execution completion
///
/// ## TDD Test
/// Tests updating step execution to completed status
#[tokio::test]
async fn test_record_step_execution_completion() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "completion-test".to_string(),
        name: "Completion Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "step-1".to_string(),
            name: "Step 1".to_string(),
            step_type: StepType::Task,
            config: json!({}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "completion-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Create and complete step execution
    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;

    storage
        .complete_step_execution(
            &step_exec_id,
            StepExecutionStatus::Completed,
            Some(json!({"result": "success"})),
            None,
        )
        .await?;

    // Verify completion
    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.status, StepExecutionStatus::Completed);
    assert!(step_exec.output.is_some());
    assert_eq!(step_exec.output.unwrap()["result"], "success");

    Ok(())
}

/// Test recording step execution failure
///
/// ## TDD Test
/// Tests recording failures with error messages
#[tokio::test]
async fn test_record_step_execution_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "failure-test".to_string(),
        name: "Failure Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "step-1".to_string(),
            name: "Step 1".to_string(),
            step_type: StepType::Task,
            config: json!({}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "failure-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Create and fail step execution
    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;

    storage
        .complete_step_execution(
            &step_exec_id,
            StepExecutionStatus::Failed,
            None,
            Some("Network timeout".to_string()),
        )
        .await?;

    // Verify failure recorded
    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.status, StepExecutionStatus::Failed);
    assert!(step_exec.error.is_some());
    assert_eq!(step_exec.error.unwrap(), "Network timeout");

    Ok(())
}

/// Test retry tracking with attempt numbers
///
/// ## TDD Test
/// Tests that retry attempts are tracked correctly
#[tokio::test]
async fn test_step_execution_retry_tracking() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "retry-test".to_string(),
        name: "Retry Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "step-1".to_string(),
            name: "Step 1".to_string(),
            step_type: StepType::Task,
            config: json!({}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "retry-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // First attempt
    let step_exec_id_1 = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;
    let step_exec_1 = storage.get_step_execution(&step_exec_id_1).await?;
    assert_eq!(step_exec_1.attempt, 1);

    storage
        .complete_step_execution(
            &step_exec_id_1,
            StepExecutionStatus::Failed,
            None,
            Some("Temporary failure".to_string()),
        )
        .await?;

    // Second attempt (retry)
    let step_exec_id_2 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 2)
        .await?;
    let step_exec_2 = storage.get_step_execution(&step_exec_id_2).await?;
    assert_eq!(step_exec_2.attempt, 2);

    Ok(())
}

/// Test retrieving step execution history
///
/// ## TDD Test
/// Tests querying all step executions for a workflow execution
#[tokio::test]
async fn test_get_step_execution_history() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "history-test".to_string(),
        name: "History Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "step-1".to_string(),
                name: "Step 1".to_string(),
                step_type: StepType::Task,
                config: json!({}),
                ..Default::default()
            },
            Step {
                id: "step-2".to_string(),
                name: "Step 2".to_string(),
                step_type: StepType::Task,
                config: json!({}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "history-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Execute two steps
    let step1_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;
    storage
        .complete_step_execution(
            &step1_id,
            StepExecutionStatus::Completed,
            Some(json!({})),
            None,
        )
        .await?;

    let step2_id = storage
        .create_step_execution(&execution_id, "step-2", json!({}))
        .await?;
    storage
        .complete_step_execution(
            &step2_id,
            StepExecutionStatus::Completed,
            Some(json!({})),
            None,
        )
        .await?;

    // Get history
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].step_id, "step-1");
    assert_eq!(history[1].step_id, "step-2");

    Ok(())
}
