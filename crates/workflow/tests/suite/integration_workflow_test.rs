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

//! Integration workflow execution tests
//!
//! ## Purpose
//! Tests for complex workflows combining multiple step types.
//! Tests cross-feature interactions.

use plexspaces_workflow::*;
use serde_json::json;

/// Test workflow combining Choice and Parallel steps
#[tokio::test]
async fn test_choice_to_parallel_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = make_workflow_definition(
        "choice-parallel",
        "Choice to Parallel",
        "1.0",
        vec![
            make_step(
                "classify",
                "Classify Input",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"priority": "high"}}),
                None,
                None,
                None,
            ),
            make_step(
                "route",
                "Route by Priority",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.priority", "operator": "equals", "value": "high", "next": "high-priority-tasks"}, {"variable": "$.priority", "operator": "equals", "value": "low", "next": "low-priority-task"}], "default": "normal-priority-task"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-priority-tasks",
                "High Priority Parallel Tasks",
                StepType::StepTypeParallel,
                json!({"branches": [{"id": "notify-urgent", "action": "succeed"}, {"id": "escalate", "action": "succeed"}, {"id": "log-high", "action": "succeed"}]}),
                None,
                None,
                None,
            ),
            make_step(
                "low-priority-task",
                "Low Priority Task",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "normal-priority-task",
                "Normal Priority Task",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-parallel", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();

    assert!(step_ids.contains(&"classify".to_string()));
    assert!(step_ids.contains(&"route".to_string()));
    assert!(step_ids.contains(&"high-priority-tasks".to_string()));

    assert!(!step_ids.contains(&"low-priority-task".to_string()));
    assert!(!step_ids.contains(&"normal-priority-task".to_string()));

    assert!(step_ids.contains(&"notify-urgent".to_string()));
    assert!(step_ids.contains(&"escalate".to_string()));
    assert!(step_ids.contains(&"log-high".to_string()));

    Ok(())
}

/// Test workflow combining Map and Choice steps
#[tokio::test]
async fn test_map_to_choice_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = make_workflow_definition(
        "map-choice",
        "Map to Choice",
        "1.0",
        vec![
            make_step(
                "process-items",
                "Process Items",
                StepType::StepTypeMap,
                json!({"items": [1, 2, 3], "iterator": {"action": "multiply", "factor": 2}}),
                None,
                None,
                None,
            ),
            make_step(
                "count-results",
                "Count Results",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"count": 3}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-count",
                "Check Result Count",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.count", "operator": "greater_than", "value": 2, "next": "many-results"}], "default": "few-results"}),
                None,
                None,
                None,
            ),
            make_step(
                "many-results",
                "Handle Many Results",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "few-results",
                "Handle Few Results",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "map-choice", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

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
#[tokio::test]
async fn test_complex_multi_step_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = make_workflow_definition(
        "complex",
        "Complex Workflow",
        "1.0",
        vec![
            make_step(
                "init",
                "Initialize",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"mode": "fast"}}),
                Some("decide-mode".to_string()),
                None,
                None,
            ),
            make_step(
                "decide-mode",
                "Decide Processing Mode",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.mode", "operator": "equals", "value": "fast", "next": "parallel-process"}], "default": "sequential-process"}),
                None,
                None,
                None,
            ),
            make_step(
                "parallel-process",
                "Parallel Processing",
                StepType::StepTypeParallel,
                json!({"branches": [{"id": "task-a", "action": "succeed"}, {"id": "task-b", "action": "succeed"}]}),
                None,
                None,
                None,
            ),
            make_step(
                "sequential-process",
                "Sequential Processing",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "complex", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();

    assert!(step_ids.contains(&"init".to_string()));
    assert!(step_ids.contains(&"decide-mode".to_string()));
    assert!(step_ids.contains(&"parallel-process".to_string()));

    assert!(!step_ids.contains(&"sequential-process".to_string()));

    Ok(())
}
