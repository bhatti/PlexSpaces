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

//! Step execution tracking tests
//!
//! ## Purpose
//! Tests for tracking individual step executions within workflows.
//! Following TDD principles - these tests are written FIRST.

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_workflow::*;
use serde_json::json;

/// Test recording step execution start
#[tokio::test]
async fn test_record_step_execution_start() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "step-test",
        "Step Test",
        "1.0",
        vec![make_step(
            "step-1",
            "First Step",
            StepType::StepTypeTask,
            json!({}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(
            &ctx,
            "step-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({"input": "test"}))
        .await?;

    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.step_id, "step-1");
    assert_eq!(step_exec.execution_id, execution_id);
    assert_eq!(step_exec.step_status(), StepStatus::StepStatusRunning);
    assert_eq!(step_exec.attempt, 1);

    Ok(())
}

/// Test recording step execution completion
#[tokio::test]
async fn test_record_step_execution_completion() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "completion-test",
        "Completion Test",
        "1.0",
        vec![make_step(
            "step-1",
            "Step 1",
            StepType::StepTypeTask,
            json!({}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(
            &ctx,
            "completion-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;

    storage
        .complete_step_execution(
            &step_exec_id,
            StepStatus::StepStatusCompleted,
            Some(json!({"result": "success"})),
            None,
        )
        .await?;

    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.step_status(), StepStatus::StepStatusCompleted);
    let output = step_exec.output_value().expect("Step should have output");
    assert_eq!(output["result"], "success");

    Ok(())
}

/// Test recording step execution failure
#[tokio::test]
async fn test_record_step_execution_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "failure-test",
        "Failure Test",
        "1.0",
        vec![make_step(
            "step-1",
            "Step 1",
            StepType::StepTypeTask,
            json!({}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(
            &ctx,
            "failure-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let step_exec_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;

    storage
        .complete_step_execution(
            &step_exec_id,
            StepStatus::StepStatusFailed,
            None,
            Some("Network timeout".to_string()),
        )
        .await?;

    let step_exec = storage.get_step_execution(&step_exec_id).await?;
    assert_eq!(step_exec.step_status(), StepStatus::StepStatusFailed);
    assert_eq!(step_exec.error, "Network timeout");

    Ok(())
}

/// Test retry tracking with attempt numbers
#[tokio::test]
async fn test_step_execution_retry_tracking() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "retry-test",
        "Retry Test",
        "1.0",
        vec![make_step(
            "step-1",
            "Step 1",
            StepType::StepTypeTask,
            json!({}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(
            &ctx,
            "retry-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let step_exec_id_1 = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;
    let step_exec_1 = storage.get_step_execution(&step_exec_id_1).await?;
    assert_eq!(step_exec_1.attempt, 1);

    storage
        .complete_step_execution(
            &step_exec_id_1,
            StepStatus::StepStatusFailed,
            None,
            Some("Temporary failure".to_string()),
        )
        .await?;

    let step_exec_id_2 = storage
        .create_step_execution_with_attempt(&execution_id, "step-1", json!({}), 2)
        .await?;
    let step_exec_2 = storage.get_step_execution(&step_exec_id_2).await?;
    assert_eq!(step_exec_2.attempt, 2);

    Ok(())
}

/// Test retrieving step execution history
#[tokio::test]
async fn test_get_step_execution_history() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "history-test",
        "History Test",
        "1.0",
        vec![
            make_step(
                "step-1",
                "Step 1",
                StepType::StepTypeTask,
                json!({}),
                None,
                None,
                None,
            ),
            make_step(
                "step-2",
                "Step 2",
                StepType::StepTypeTask,
                json!({}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(
            &ctx,
            "history-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let step1_id = storage
        .create_step_execution(&execution_id, "step-1", json!({}))
        .await?;
    storage
        .complete_step_execution(
            &step1_id,
            StepStatus::StepStatusCompleted,
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
            StepStatus::StepStatusCompleted,
            Some(json!({})),
            None,
        )
        .await?;

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].step_id, "step-1");
    assert_eq!(history[1].step_id, "step-2");

    Ok(())
}
