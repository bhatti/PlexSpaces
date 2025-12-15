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

//! Parallel workflow execution tests
//!
//! ## Purpose
//! Tests for executing multiple workflow steps concurrently.
//! Following TDD principles - these tests are written FIRST (RED phase).

use plexspaces_workflow::*;
use serde_json::json;

/// Test parallel execution of 3 independent steps
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement parallel step execution
#[tokio::test]
async fn test_parallel_three_branches() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with parallel step containing 3 branches
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
                    {
                        "id": "branch-a",
                        "action": "succeed",
                        "delay_ms": 50
                    },
                    {
                        "id": "branch-b",
                        "action": "succeed",
                        "delay_ms": 100
                    },
                    {
                        "id": "branch-c",
                        "action": "succeed",
                        "delay_ms": 25
                    }
                ]
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Execute workflow
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-test", "1.0", json!({})).await?;

    // Verify workflow completed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify parallel step execution
    let history = storage.get_step_execution_history(&execution_id).await?;

    // Should have 1 parent step + 3 branch executions = 4 total
    assert_eq!(history.len(), 4);

    // Parent parallel step should be completed
    assert_eq!(history[0].step_id, "parallel-step");
    assert_eq!(history[0].status, StepExecutionStatus::Completed);

    // All 3 branches should be completed
    assert!(history[1..]
        .iter()
        .all(|s| s.status == StepExecutionStatus::Completed));

    // Branch IDs should match (order doesn't matter due to concurrency)
    let branch_ids: Vec<String> = history[1..].iter().map(|s| s.step_id.clone()).collect();
    assert!(branch_ids.contains(&"branch-a".to_string()));
    assert!(branch_ids.contains(&"branch-b".to_string()));
    assert!(branch_ids.contains(&"branch-c".to_string()));

    Ok(())
}

/// Test parallel execution with one failing branch
///
/// ## TDD Test (RED)
/// If any branch fails, the parallel step should fail
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
                    {
                        "id": "branch-success",
                        "action": "succeed"
                    },
                    {
                        "id": "branch-fail",
                        "action": "fail",
                        "error": "Branch failed"
                    },
                    {
                        "id": "branch-success-2",
                        "action": "succeed"
                    }
                ]
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "parallel-fail", "1.0", json!({})).await?;

    // Workflow should fail due to branch failure
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    // Verify all branches were attempted
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 3); // At least 3 branches executed

    Ok(())
}

/// Test parallel execution combines branch outputs
///
/// ## TDD Test (RED)
/// Parallel step should aggregate outputs from all branches
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
                        {
                            "id": "branch-1",
                            "action": "generate",
                            "data": "result-1"
                        },
                        {
                            "id": "branch-2",
                            "action": "generate",
                            "data": "result-2"
                        }
                    ]
                }),
                ..Default::default()
            },
            // Next step receives aggregated output
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

    // Verify output aggregation
    let history = storage.get_step_execution_history(&execution_id).await?;

    // Find parallel step execution
    let parallel_exec = history
        .iter()
        .find(|s| s.step_id == "parallel-step")
        .unwrap();
    assert!(parallel_exec.output.is_some());

    // Output should contain results from both branches
    let output = parallel_exec.output.as_ref().unwrap();
    assert!(output.is_object() || output.is_array());

    Ok(())
}

/// Test empty parallel step (no branches)
///
/// ## TDD Test (RED)
/// Parallel step with no branches should complete immediately
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

/// Test parallel execution timing (all branches run concurrently)
///
/// ## TDD Test (RED)
/// Total execution time should be ~max(branch times), not sum(branch times)
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

    // If truly concurrent: ~100ms (max of delays)
    // If sequential: ~300ms (sum of delays)
    // Allow some overhead, but should be closer to 100ms than 300ms
    assert!(
        elapsed.as_millis() < 250,
        "Execution took {}ms, expected concurrent execution ~100ms",
        elapsed.as_millis()
    );

    Ok(())
}
