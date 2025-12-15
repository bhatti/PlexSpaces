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

use plexspaces_workflow::*;
use serde_json::json;

/// Test basic workflow execution as an actor behavior
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement WorkflowBehavior
#[tokio::test]
async fn test_workflow_actor_basic_execution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create simple workflow definition
    let definition = WorkflowDefinition {
        id: "simple-workflow".to_string(),
        name: "Simple Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "step1".to_string(),
            name: "Step 1".to_string(),
            step_type: StepType::Task,
            config: json!({"action": "succeed", "output": {"result": "done"}}),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create workflow actor
    let actor = WorkflowActor::new("workflow-actor-1", storage.clone()).await?;

    // Start workflow execution
    let start_msg = WorkflowMessage::Start {
        definition_id: "simple-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    // Verify execution started
    if let WorkflowResponse::Started { execution_id } = response {
        assert!(!execution_id.is_empty());

        // Query execution status
        let query_msg = WorkflowMessage::Query {
            execution_id: execution_id.clone(),
        };
        let status_response = actor.handle_message(query_msg).await?;

        // Verify execution completed
        if let WorkflowResponse::Status { status, output } = status_response {
            assert_eq!(status, ExecutionStatus::Completed);
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
///
/// ## TDD Test (RED)
/// Ensures workflow actors can handle complex multi-step workflows
#[tokio::test]
async fn test_workflow_actor_multi_step() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create multi-step workflow
    let definition = WorkflowDefinition {
        id: "multi-step".to_string(),
        name: "Multi Step".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"data": "test-data"}}),
                ..Default::default()
            },
            Step {
                id: "process".to_string(),
                name: "Process".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed", "output": {"processed": true}}),
                ..Default::default()
            },
            Step {
                id: "complete".to_string(),
                name: "Complete".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let actor = WorkflowActor::new("workflow-actor-2", storage.clone()).await?;

    // Start workflow
    let start_msg = WorkflowMessage::Start {
        definition_id: "multi-step".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        // Verify all steps executed
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
///
/// ## TDD Test (RED)
/// Workflows should support cancellation via actor messages
#[tokio::test]
async fn test_workflow_actor_cancellation() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with wait step (simulates long-running)
    let definition = WorkflowDefinition {
        id: "long-workflow".to_string(),
        name: "Long Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "step1".to_string(),
                name: "Step 1".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "wait".to_string(),
                name: "Wait".to_string(),
                step_type: StepType::Wait,
                config: json!({"duration_ms": 10000}), // 10 second wait
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let actor = WorkflowActor::new("workflow-actor-3", storage.clone()).await?;

    // Start workflow
    let start_msg = WorkflowMessage::Start {
        definition_id: "long-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        // Cancel workflow
        let cancel_msg = WorkflowMessage::Cancel {
            execution_id: execution_id.clone(),
        };
        let cancel_response = actor.handle_message(cancel_msg).await?;

        // Verify cancellation
        if let WorkflowResponse::Cancelled = cancel_response {
            // Check execution status
            let execution = storage.get_execution(&execution_id).await?;
            assert_eq!(execution.status, ExecutionStatus::Cancelled);
        } else {
            panic!("Expected Cancelled response");
        }
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor with Signal step
///
/// ## TDD Test (RED)
/// Workflow actors should support external signals
#[tokio::test]
async fn test_workflow_actor_with_signal() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with signal step
    let definition = WorkflowDefinition {
        id: "signal-workflow".to_string(),
        name: "Signal Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "start".to_string(),
                name: "Start".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "wait-approval".to_string(),
                name: "Wait for Approval".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "complete".to_string(),
                name: "Complete".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let actor = WorkflowActor::new("workflow-actor-4", storage.clone()).await?;

    // Start workflow (will pause at signal step)
    let start_msg = WorkflowMessage::Start {
        definition_id: "signal-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        // Send signal to continue execution
        let signal_msg = WorkflowMessage::Signal {
            execution_id: execution_id.clone(),
            signal_name: "approval".to_string(),
            payload: json!({"approved": true}),
        };

        let signal_response = actor.handle_message(signal_msg).await?;

        // Verify signal sent
        if let WorkflowResponse::SignalSent = signal_response {
            // Resume execution (workflow should complete now)
            let resume_msg = WorkflowMessage::Resume {
                execution_id: execution_id.clone(),
            };
            actor.handle_message(resume_msg).await?;

            // Check final status
            let execution = storage.get_execution(&execution_id).await?;
            assert_eq!(execution.status, ExecutionStatus::Completed);
        } else {
            panic!("Expected SignalSent response");
        }
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}

/// Test workflow actor with failure and retry
///
/// ## TDD Test (RED)
/// Workflow actors should handle step failures with retry logic
#[tokio::test]
async fn test_workflow_actor_failure_retry() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with flaky step
    let definition = WorkflowDefinition {
        id: "flaky-workflow".to_string(),
        name: "Flaky Workflow".to_string(),
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
                max_backoff: std::time::Duration::from_millis(100),
                jitter: 0.0,
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let actor = WorkflowActor::new("workflow-actor-5", storage.clone()).await?;

    // Start workflow (will retry and eventually succeed)
    let start_msg = WorkflowMessage::Start {
        definition_id: "flaky-workflow".to_string(),
        definition_version: "1.0".to_string(),
        input: json!({}),
    };

    let response = actor.handle_message(start_msg).await?;

    if let WorkflowResponse::Started { execution_id } = response {
        // Verify execution completed after retries
        let execution = storage.get_execution(&execution_id).await?;
        assert_eq!(execution.status, ExecutionStatus::Completed);

        // Verify retry attempts
        let history = storage.get_step_execution_history(&execution_id).await?;
        assert_eq!(history.len(), 3); // 3 attempts (2 failures + 1 success)
    } else {
        panic!("Expected Started response");
    }

    Ok(())
}
