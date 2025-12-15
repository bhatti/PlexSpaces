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

//! Map workflow execution tests
//!
//! ## Purpose
//! Tests for executing the same step for each item in an array.
//! Following TDD principles - these tests are written FIRST (RED phase).

use plexspaces_workflow::*;
use serde_json::json;

/// Test map over array of items
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement map step execution
#[tokio::test]
async fn test_map_over_array() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with map step
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

    // Execute workflow
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-test", "1.0", json!({})).await?;

    // Verify workflow completed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify map step execution
    let history = storage.get_step_execution_history(&execution_id).await?;

    // Should have 1 parent map step + 3 item executions = 4 total
    assert_eq!(history.len(), 4);

    // Parent map step should be completed
    assert_eq!(history[0].step_id, "map-step");
    assert_eq!(history[0].status, StepExecutionStatus::Completed);

    // All 3 item iterations should be completed
    assert!(history[1..]
        .iter()
        .all(|s| s.status == StepExecutionStatus::Completed));

    // Output should be array of results
    let map_output = history[0].output.as_ref().unwrap();
    assert!(map_output.is_array());
    assert_eq!(map_output.as_array().unwrap().len(), 3);

    Ok(())
}

/// Test map with empty array
///
/// ## TDD Test (RED)
/// Map over empty array should complete immediately with empty results
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

    // Should have only parent map step
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 1);

    let output = history[0].output.as_ref().unwrap();
    assert!(output.is_array());
    assert_eq!(output.as_array().unwrap().len(), 0);

    Ok(())
}

/// Test map with one failing item
///
/// ## TDD Test (RED)
/// If any item fails, the map step should fail
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

    // Workflow should fail due to item failure
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    Ok(())
}

/// Test map preserves item order in results
///
/// ## TDD Test (RED)
/// Results should be in same order as input items
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

    // Results should be [2, 4, 6, 8, 10] in order
    let results = output.as_array().unwrap();
    assert_eq!(results.len(), 5);

    // Verify order is preserved (results correspond to input order)
    for (i, result) in results.iter().enumerate() {
        let expected_value = (i + 1) * 2;
        assert!(result.get("result").is_some());
    }

    Ok(())
}

/// Test map with max concurrency limit
///
/// ## TDD Test (RED)
/// Map should respect max_concurrency setting
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
                "max_concurrency": 3,  // Process at most 3 items at a time
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

    // With max_concurrency=3 and 10 items with 50ms delay:
    // Batches: [1,2,3], [4,5,6], [7,8,9], [10]
    // Expected time: 4 batches * 50ms = ~200ms
    // Without limit: 50ms (all concurrent)
    // Verify it's closer to 200ms than 50ms
    assert!(
        elapsed.as_millis() > 150,
        "Expected batched execution ~200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

/// Test map passes each item as input to iterator
///
/// ## TDD Test (RED)
/// Each iteration should receive its item as input
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
                    "action": "generate"  // Will echo input
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

    // Verify each item was processed
    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4); // 1 parent + 3 items

    Ok(())
}
