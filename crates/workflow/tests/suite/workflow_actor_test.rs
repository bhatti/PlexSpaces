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

//! Workflow-Actor Integration Tests
//!
//! ## Purpose
//! Tests the integration between the workflow engine and the actor system.
//! Workflows can be executed as actor behaviors with full actor lifecycle support.
//!
//! ## Test Coverage
//! - Starting workflows via actor messages
//! - Querying workflow execution status
//! - Canceling running workflows
//! - Workflow execution with supervision
//! - Workflow state persistence in actor state

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_workflow::*;
use serde_json::json;

/// Test basic workflow execution as an actor behavior
#[tokio::test]
async fn test_workflow_actor_basic_execution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "simple-workflow",
        "Simple Workflow",
        "1.0",
        vec![make_step(
            "step1",
            "Step 1",
            StepType::StepTypeTask,
            json!({"action": "succeed", "output": {"result": "done"}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let actor = WorkflowActor::new(ctx.clone(), "workflow-actor-1", storage.clone()).await?;

    let start_msg = WorkflowMessage::Start {
        definition_id: "simple-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        assert!(!execution_id.is_empty());

        let query_msg = WorkflowMessage::Query {
            execution_id: execution_id.clone(),
        };
        let status_response = actor.handle_message(query_msg).await?;

        if let WorkflowResponse::Status {
            status_code,
            output,
        } = status_response
        {
            assert_eq!(
                ExecutionStatus::try_from(status_code).unwrap(),
                ExecutionStatus::ExecutionStatusCompleted
            );
            assert!(output.is_some());
            let output_val = output.unwrap();
            assert_eq!(
                output_val.get("result").and_then(|v| v.as_str()),
                Some("done")
            );
        } else {
            panic!("Expected Status response");
        }
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor with multi-step workflow
#[tokio::test]
async fn test_workflow_actor_multi_step() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "multi-step",
        "Multi Step",
        "1.0",
        vec![
            make_step(
                "generate",
                "Generate",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"data": "test-data"}}),
                None,
                None,
                None,
            ),
            make_step(
                "process",
                "Process",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"processed": true}}),
                None,
                None,
                None,
            ),
            make_step(
                "complete",
                "Complete",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let actor = WorkflowActor::new(ctx.clone(), "workflow-actor-2", storage.clone()).await?;

    let start_msg = WorkflowMessage::Start {
        definition_id: "multi-step".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        let history = storage.get_step_execution_history(&execution_id).await?;
        assert_eq!(history.len(), 3);

        let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
        assert!(step_ids.contains(&"generate".to_string()));
        assert!(step_ids.contains(&"process".to_string()));
        assert!(step_ids.contains(&"complete".to_string()));
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor cancellation
#[tokio::test]
async fn test_workflow_actor_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "long-workflow",
        "Long Workflow",
        "1.0",
        vec![
            make_step(
                "step1",
                "Step 1",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "wait",
                "Wait",
                StepType::StepTypeWait,
                json!({"duration_ms": 10000}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let actor = WorkflowActor::new(ctx.clone(), "workflow-actor-3", storage.clone()).await?;

    let start_msg = WorkflowMessage::Start {
        definition_id: "long-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        let cancel_msg = WorkflowMessage::Cancel {
            execution_id: execution_id.clone(),
        };
        let cancel_response = actor.handle_message(cancel_msg).await?;

        if let WorkflowResponse::Cancelled = cancel_response {
            let execution = storage.get_execution(&ctx, &execution_id).await?;
            assert_eq!(
                execution.execution_status(),
                ExecutionStatus::ExecutionStatusCancelled
            );
        } else {
            panic!("Expected Cancelled response");
        }
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor with Signal step
#[tokio::test]
async fn test_workflow_actor_with_signal() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-workflow",
        "Signal Workflow",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "wait-approval",
                "Wait for Approval",
                StepType::StepTypeSignal,
                json!({"signal_name": "approval", "timeout_ms": 5000}),
                None,
                None,
                None,
            ),
            make_step(
                "complete",
                "Complete",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let actor = WorkflowActor::new(ctx.clone(), "workflow-actor-4", storage.clone()).await?;

    let start_msg = WorkflowMessage::Start {
        definition_id: "signal-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        let signal_msg = WorkflowMessage::Signal {
            execution_id: execution_id.clone(),
            signal_name: "approval".to_string(),
            payload: json!({"approved": true}),
        };

        let signal_response = actor.handle_message(signal_msg).await?;

        if let WorkflowResponse::SignalSent = signal_response {
            let resume_msg = WorkflowMessage::Resume {
                execution_id: execution_id.clone(),
            };
            actor.handle_message(resume_msg).await?;

            let execution = storage.get_execution(&ctx, &execution_id).await?;
            assert_eq!(
                execution.execution_status(),
                ExecutionStatus::ExecutionStatusCompleted
            );
        } else {
            panic!("Expected SignalSent response");
        }
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor with failure and retry
#[tokio::test]
async fn test_workflow_actor_failure_retry() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let retry_config = RetryConfig {
        max_attempts: 3,
        initial_interval_ms: 10,
        backoff_rate: 2.0,
        max_interval_ms: 100,
        retryable_errors: vec![],
    };

    let definition = make_workflow_definition(
        "flaky-workflow",
        "Flaky Workflow",
        "1.0",
        vec![make_step(
            "flaky-step",
            "Flaky Step",
            StepType::StepTypeTask,
            json!({"action": "flaky", "fail_count": 2}),
            None,
            None,
            Some(retry_config),
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let actor = WorkflowActor::new(ctx.clone(), "workflow-actor-5", storage.clone()).await?;

    let start_msg = WorkflowMessage::Start {
        definition_id: "flaky-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        let execution = storage.get_execution(&ctx, &execution_id).await?;
        assert_eq!(
            execution.execution_status(),
            ExecutionStatus::ExecutionStatusCompleted
        );

        let history = storage.get_step_execution_history(&execution_id).await?;
        assert_eq!(history.len(), 3); // 3 attempts (2 failures + 1 success)
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}
