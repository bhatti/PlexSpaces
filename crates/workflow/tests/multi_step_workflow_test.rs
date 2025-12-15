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

//! Multi-step workflow orchestration tests
//!
//! ## Purpose
//! Tests for executing workflows with multiple sequential steps.
//! Following TDD principles - these tests are written FIRST.

use plexspaces_workflow::*;
use serde_json::json;

/// Test sequential execution of 3 steps
///
/// ## TDD Test
/// This test will FAIL until we implement multi-step orchestration
#[tokio::test]
async fn test_three_step_sequential_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with 3 sequential steps
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

    // Execute workflow
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "three-step", "1.0", json!({"user_id": 123}))
            .await?;

    // Verify workflow completed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify all 3 steps were executed
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

/// Test data passing between steps
///
/// ## TDD Test
/// Output of step N should be available as input to step N+1
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

    // Verify data was passed through
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);

    // Step 1 should have output
    assert!(history[0].output.is_some());

    // Step 2 should have received step 1's output as input
    // (This is a simplified assertion - real implementation would verify actual data flow)
    assert!(history[1].input.is_some());

    Ok(())
}

/// Test workflow failure when a step fails
///
/// ## TDD Test
/// If step 2 of 3 fails, workflow should fail and step 3 should not execute
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

    // Verify workflow failed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    // Verify only 2 steps executed (step 3 should not have run)
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].status, StepExecutionStatus::Completed);
    assert_eq!(history[1].status, StepExecutionStatus::Failed);

    Ok(())
}

/// Test workflow execution with step retry
///
/// ## TDD Test
/// Step with retry policy should retry on failure before workflow fails
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
                jitter: 0.0, // No jitter for deterministic testing
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "retry-workflow", "1.0", json!({})).await?;

    // Verify workflow eventually succeeded after retries
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify multiple attempts were made
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 2); // At least 2 attempts (could be 3)

    // Last attempt should be successful
    assert_eq!(
        history.last().unwrap().status,
        StepExecutionStatus::Completed
    );

    Ok(())
}

/// Test empty workflow (no steps)
///
/// ## TDD Test
/// Workflow with no steps should complete immediately
#[tokio::test]
async fn test_empty_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "empty".to_string(),
        name: "Empty Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![], // No steps
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "empty", "1.0", json!({})).await?;

    // Should complete immediately
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // No step executions
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 0);

    Ok(())
}

/// Test workflow with explicit step ordering
///
/// ## TDD Test
/// Steps with 'next' field should execute in specified order
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

    // Verify correct execution order
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].step_id, "start");
    assert_eq!(history[1].step_id, "middle");
    assert_eq!(history[2].step_id, "end");

    Ok(())
}
