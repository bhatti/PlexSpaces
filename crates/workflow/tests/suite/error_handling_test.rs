// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Error handling tests for workflow engine
//!
//! ## Purpose
//! Tests error paths and edge cases to achieve 95%+ coverage.
//! These tests target uncovered lines identified by tarpaulin.

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_workflow::*;
use serde_json::json;

/// Test Choice step with greater_than operator
#[tokio::test]
async fn test_choice_greater_than_operator() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-greater-than",
        "Choice Greater Than",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 150}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100, "next": "high-value"}], "default": "low-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-greater-than", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-value".to_string()));
    assert!(!step_ids.contains(&"low-value".to_string()));

    Ok(())
}

/// Test Choice step with less_than operator
#[tokio::test]
async fn test_choice_less_than_operator() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-less-than",
        "Choice Less Than",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 50}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "less_than", "value": 100, "next": "low-value"}], "default": "high-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-less-than", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Wait step with invalid timestamp format
#[tokio::test]
async fn test_wait_invalid_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-invalid-timestamp",
        "Wait Invalid Timestamp",
        "1.0",
        vec![make_step(
            "wait-step",
            "Wait Invalid",
            StepType::StepTypeWait,
            json!({"until": "not-a-valid-timestamp"}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-invalid-timestamp", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    assert_eq!(wait_exec.step_status(), StepStatus::StepStatusFailed);
    assert!(!wait_exec.error.is_empty());
    assert!(wait_exec.error.contains("Invalid timestamp format"));

    Ok(())
}

/// Test Signal step with missing signal_name config
#[tokio::test]
async fn test_signal_missing_config() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-missing-config",
        "Signal Missing Config",
        "1.0",
        vec![make_step(
            "wait-signal",
            "Wait Signal",
            StepType::StepTypeSignal,
            json!({"timeout_ms": 1000}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "signal-missing-config", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    Ok(())
}

/// Test Wait step with missing duration config
#[tokio::test]
async fn test_wait_missing_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-missing-duration",
        "Wait Missing Duration",
        "1.0",
        vec![make_step(
            "wait-step",
            "Wait Missing Duration",
            StepType::StepTypeWait,
            json!({}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-missing-duration", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    assert_eq!(wait_exec.step_status(), StepStatus::StepStatusFailed);
    assert!(!wait_exec.error.is_empty());
    assert!(wait_exec.error.contains("Wait step requires"));

    Ok(())
}

/// Test Choice step with non-numeric values for numeric operators
#[tokio::test]
async fn test_choice_non_numeric_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-non-numeric",
        "Choice Non-Numeric",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": "not-a-number"}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100, "next": "high-value"}], "default": "low-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-non-numeric", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Choice step with float comparison (greater_than)
#[tokio::test]
async fn test_choice_float_greater_than() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-float-gt",
        "Choice Float GT",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 150.5}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100.0, "next": "high-value"}], "default": "low-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-float-gt", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test Choice step with float comparison (less_than)
#[tokio::test]
async fn test_choice_float_less_than() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-float-lt",
        "Choice Float LT",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 50.5}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "less_than", "value": 100.0, "next": "low-value"}], "default": "high-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-float-lt", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"low-value".to_string()));

    Ok(())
}

/// Test execute_from_state with empty workflow
#[tokio::test]
async fn test_execute_from_state_empty_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition("empty-workflow", "Empty Workflow", "1.0", vec![]);
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "empty-workflow",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await?;

    WorkflowExecutor::execute_from_state(&storage, &ctx, &execution_id).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}

/// Test Choice step pointing to non-existent next step
#[tokio::test]
async fn test_choice_invalid_next_step() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-invalid-next",
        "Choice Invalid Next",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 150}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100, "next": "non-existent-step"}]}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-invalid-next", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}

/// Test Choice step with no matching choices and no default
#[tokio::test]
async fn test_choice_no_match_no_default() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-no-match",
        "Choice No Match",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 50}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100, "next": "high-value"}]}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-no-match", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(!step_ids.contains(&"high-value".to_string()));

    Ok(())
}

/// Test choice-selected step with invalid next
#[tokio::test]
async fn test_choice_path_invalid_next() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-path-invalid",
        "Choice Path Invalid",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"value": 150}}),
                None,
                None,
                None,
            ),
            make_step(
                "check-value",
                "Check Value",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.value", "operator": "greater_than", "value": 100, "next": "high-value"}], "default": "low-value"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-value",
                "High Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                Some("non-existent-step".to_string()),
                None,
                None,
            ),
            make_step(
                "low-value",
                "Low Value",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-path-invalid", "1.0", json!({}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}
