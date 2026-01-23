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

//! Integration workflow execution tests
//!
//! ## Purpose
//! Tests for complex workflows combining multiple step types.
//! Tests cross-feature interactions.

use plexspaces_workflow::*;
use serde_json::json;

/// Test workflow combining Choice and Parallel steps
///
/// ## Integration Test
/// Choice step routes to different parallel execution branches
#[tokio::test]
async fn test_choice_to_parallel_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-parallel".to_string(),
        name: "Choice to Parallel".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "classify".to_string(),
                name: "Classify Input".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"priority": "high"}
                }),
                ..Default::default()
            },
            Step {
                id: "route".to_string(),
                name: "Route by Priority".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.priority",
                            "operator": "equals",
                            "value": "high",
                            "next": "high-priority-tasks"
                        },
                        {
                            "variable": "$.priority",
                            "operator": "equals",
                            "value": "low",
                            "next": "low-priority-task"
                        }
                    ],
                    "default": "normal-priority-task"
                }),
                ..Default::default()
            },
            Step {
                id: "high-priority-tasks".to_string(),
                name: "High Priority Parallel Tasks".to_string(),
                step_type: StepType::Parallel,
                config: json!({
                    "branches": [
                        {"id": "notify-urgent", "action": "succeed"},
                        {"id": "escalate", "action": "succeed"},
                        {"id": "log-high", "action": "succeed"}
                    ]
                }),
                ..Default::default()
            },
            Step {
                id: "low-priority-task".to_string(),
                name: "Low Priority Task".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "normal-priority-task".to_string(),
                name: "Normal Priority Task".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-parallel", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify choice routed to high-priority-tasks and parallel branches executed
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();

    assert!(step_ids.contains(&"classify".to_string()));
    assert!(step_ids.contains(&"route".to_string()));
    assert!(step_ids.contains(&"high-priority-tasks".to_string()));

    // Should NOT execute low or normal priority
    assert!(!step_ids.contains(&"low-priority-task".to_string()));
    assert!(!step_ids.contains(&"normal-priority-task".to_string()));

    // Parallel branches should have executed
    assert!(step_ids.contains(&"notify-urgent".to_string()));
    assert!(step_ids.contains(&"escalate".to_string()));
    assert!(step_ids.contains(&"log-high".to_string()));

    Ok(())
}

/// Test workflow combining Map and Choice steps
///
/// ## Integration Test
/// Map processes items, then choice routes based on aggregated result
#[tokio::test]
async fn test_map_to_choice_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "map-choice".to_string(),
        name: "Map to Choice".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "process-items".to_string(),
                name: "Process Items".to_string(),
                step_type: StepType::Map,
                config: json!({
                    "items": [1, 2, 3],
                    "iterator": {
                        "action": "multiply",
                        "factor": 2
                    }
                }),
                ..Default::default()
            },
            Step {
                id: "count-results".to_string(),
                name: "Count Results".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"count": 3}
                }),
                ..Default::default()
            },
            Step {
                id: "check-count".to_string(),
                name: "Check Result Count".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.count",
                            "operator": "greater_than",
                            "value": 2,
                            "next": "many-results"
                        }
                    ],
                    "default": "few-results"
                }),
                ..Default::default()
            },
            Step {
                id: "many-results".to_string(),
                name: "Handle Many Results".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "few-results".to_string(),
                name: "Handle Few Results".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-choice", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();

    assert!(step_ids.contains(&"process-items".to_string()));
    assert!(step_ids.contains(&"count-results".to_string()));
    assert!(step_ids.contains(&"check-count".to_string()));
    assert!(step_ids.contains(&"many-results".to_string()));
    assert!(!step_ids.contains(&"few-results".to_string()));

    Ok(())
}

/// Test complex workflow: Sequential → Choice → Parallel → Map
///
/// ## Integration Test
/// Full integration of all step types in one workflow
#[tokio::test]
async fn test_complex_multi_step_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "complex".to_string(),
        name: "Complex Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "init".to_string(),
                name: "Initialize".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"mode": "fast"}
                }),
                next: Some("decide-mode".to_string()),
                ..Default::default()
            },
            Step {
                id: "decide-mode".to_string(),
                name: "Decide Processing Mode".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.mode",
                            "operator": "equals",
                            "value": "fast",
                            "next": "parallel-process"
                        }
                    ],
                    "default": "sequential-process"
                }),
                ..Default::default()
            },
            Step {
                id: "parallel-process".to_string(),
                name: "Parallel Processing".to_string(),
                step_type: StepType::Parallel,
                config: json!({
                    "branches": [
                        {"id": "task-a", "action": "succeed"},
                        {"id": "task-b", "action": "succeed"}
                    ]
                }),
                // No next - ends after choice-selected parallel
                ..Default::default()
            },
            Step {
                id: "sequential-process".to_string(),
                name: "Sequential Processing".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "complex", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();

    // Verify execution path: init → decide-mode → parallel-process (ends)
    assert!(step_ids.contains(&"init".to_string()));
    assert!(step_ids.contains(&"decide-mode".to_string()));
    assert!(step_ids.contains(&"parallel-process".to_string()));

    // Should NOT execute sequential path (wrong choice branch)
    assert!(!step_ids.contains(&"sequential-process".to_string()));

    Ok(())
}
