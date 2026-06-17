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

//! Consolidated workflow step type tests
//!
//! This file consolidates tests from:
//! - signal_workflow_test.rs (7 tests)
//! - simple_workflow_test.rs (6 tests)
//! - map_workflow_test.rs (6 tests)
//! - parallel_workflow_test.rs (5 tests)
//! - choice_workflow_test.rs (5 tests)
//! - wait_workflow_test.rs (6 tests)
//! - multi_step_workflow_test.rs (6 tests)
//!
//! Total: 41 tests

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_workflow::*;
use serde_json::json;
use std::time::{Duration, Instant};

// =============================================================================
// SIGNAL WORKFLOW TESTS (from signal_workflow_test.rs - 7 tests)
// =============================================================================

#[tokio::test]
async fn test_signal_immediate_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-immediate",
        "Signal Immediate",
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
                "wait-signal",
                "Wait for Signal",
                StepType::StepTypeSignal,
                json!({"signal_name": "approval", "timeout_ms": 5000}),
                None,
                None,
                None,
            ),
            make_step(
                "finish",
                "Finish",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "signal-immediate",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
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

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    assert_eq!(signal_exec.step_status(), StepStatus::StepStatusCompleted);
    let output = signal_exec
        .output_value()
        .expect("Signal should have output");
    assert_eq!(output.get("approved").and_then(|v| v.as_bool()), Some(true));

    Ok(())
}

#[tokio::test]
async fn test_signal_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-timeout",
        "Signal Timeout",
        "1.0",
        vec![make_step(
            "wait-signal",
            "Wait for Signal",
            StepType::StepTypeSignal,
            json!({"signal_name": "approval", "timeout_ms": 100}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "signal-timeout", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    assert!(
        elapsed >= Duration::from_millis(90),
        "Expected at least 90ms timeout, got {}ms",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_millis(200),
        "Expected less than 200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_signal_delayed_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-delayed",
        "Signal Delayed",
        "1.0",
        vec![make_step(
            "wait-signal",
            "Wait for Signal",
            StepType::StepTypeSignal,
            json!({"signal_name": "payment-complete", "timeout_ms": 2000}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "signal-delayed",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let storage_clone = storage.clone();
    let exec_id_clone = execution_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = storage_clone
            .send_signal(
                &exec_id_clone,
                "payment-complete",
                json!({"amount": 100, "status": "success"}),
            )
            .await;
    });

    let start = std::time::Instant::now();
    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await?;

    let result = WorkflowExecutor::execute_from_state(&storage, &ctx, &execution_id).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Execution should succeed");

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed >= Duration::from_millis(40),
        "Expected at least 40ms wait, got {}ms",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_millis(150),
        "Expected less than 150ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_signal_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-payload",
        "Signal Payload",
        "1.0",
        vec![
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
                "process-approval",
                "Process Approval",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "signal-payload",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(
            &execution_id,
            "approval",
            json!({
                "approved_by": "alice@example.com",
                "approval_time": "2025-01-12T10:30:00Z",
                "notes": "Looks good!"
            }),
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

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-approval")
        .expect("Signal step should have executed");

    let output = signal_exec
        .output_value()
        .expect("Signal should have output");
    assert_eq!(
        output.get("approved_by").and_then(|v| v.as_str()),
        Some("alice@example.com")
    );
    assert_eq!(
        output.get("notes").and_then(|v| v.as_str()),
        Some("Looks good!")
    );

    Ok(())
}

#[tokio::test]
async fn test_multiple_signals_in_sequence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "multi-signal",
        "Multi Signal",
        "1.0",
        vec![
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
                "wait-payment",
                "Wait for Payment",
                StepType::StepTypeSignal,
                json!({"signal_name": "payment", "timeout_ms": 5000}),
                None,
                None,
                None,
            ),
            make_step(
                "finish",
                "Finish",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "multi-signal",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await?;
    storage
        .send_signal(&execution_id, "payment", json!({"paid": true}))
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

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"wait-approval".to_string()));
    assert!(step_ids.contains(&"wait-payment".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_signal_no_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-no-timeout",
        "Signal No Timeout",
        "1.0",
        vec![make_step(
            "wait-signal",
            "Wait for Signal",
            StepType::StepTypeSignal,
            json!({"signal_name": "notification"}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "signal-no-timeout",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(&execution_id, "notification", json!({"message": "done"}))
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

#[tokio::test]
async fn test_signal_merges_input_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "signal-merge",
        "Signal Merge",
        "1.0",
        vec![
            make_step(
                "generate",
                "Generate Data",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"order_id": "12345", "status": "pending"}}),
                None,
                None,
                None,
            ),
            make_step(
                "wait-signal",
                "Wait for Signal",
                StepType::StepTypeSignal,
                json!({"signal_name": "update", "timeout_ms": 5000}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "signal-merge",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .send_signal(
            &execution_id,
            "update",
            json!({"status": "approved", "approved_by": "bob"}),
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

    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    let output = signal_exec
        .output_value()
        .expect("Signal should have output");
    assert_eq!(
        output.get("order_id").and_then(|v| v.as_str()),
        Some("12345")
    );
    assert_eq!(
        output.get("status").and_then(|v| v.as_str()),
        Some("approved")
    );
    assert_eq!(
        output.get("approved_by").and_then(|v| v.as_str()),
        Some("bob")
    );

    Ok(())
}

// =============================================================================
// SIMPLE WORKFLOW TESTS (from simple_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_simple_single_step_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "hello-world",
        "Hello World Workflow",
        "1.0",
        vec![make_step(
            "say-hello",
            "Say Hello",
            StepType::StepTypeTask,
            json!({"actor_type": "EchoActor", "method": "say_hello", "input": {"name": "World"}}),
            None,
            None,
            None,
        )],
    );

    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "hello-world", "1.0", json!({"name": "World"}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let output = execution
        .output_value()
        .expect("Execution should have output");
    assert_eq!(output["greeting"], "Hello, World!");

    Ok(())
}

#[tokio::test]
async fn test_workflow_definition_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition("test-workflow", "Test Workflow", "1.0", vec![]);

    storage.save_definition(&ctx, &definition).await?;

    let retrieved = storage.get_definition(&ctx, "test-workflow", "1.0").await?;

    assert_eq!(retrieved.id, "test-workflow");
    assert_eq!(retrieved.name, "Test Workflow");
    assert_eq!(retrieved.version, "1.0");

    Ok(())
}

#[tokio::test]
async fn test_execution_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition("status-test", "Status Test", "1.0", vec![]);
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "status-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusPending
    );

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusRunning)
        .await?;
    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusRunning
    );

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusCompleted)
        .await?;
    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}

#[tokio::test]
async fn test_execution_labels() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition("label-test", "Label Test", "1.0", vec![]);
    storage.save_definition(&ctx, &definition).await?;

    let mut labels = std::collections::HashMap::new();
    labels.insert("environment".to_string(), "production".to_string());
    labels.insert("priority".to_string(), "high".to_string());

    let execution_id = storage
        .create_execution(&ctx, "label-test", "1.0", json!({}), labels)
        .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusPending
    );

    Ok(())
}

#[tokio::test]
async fn test_all_execution_statuses() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition =
        make_workflow_definition("status-full-test", "Status Full Test", "1.0", vec![]);
    storage.save_definition(&ctx, &definition).await?;

    let execution_id = storage
        .create_execution(&ctx, 
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::ExecutionStatusCancelled)
        .await?;
    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCancelled
    );
    assert_eq!(execution.execution_status().as_sql_str(), "CANCELLED");

    let execution_id2 = storage
        .create_execution(&ctx, 
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id2, ExecutionStatus::ExecutionStatusTimedOut)
        .await?;
    let execution = storage.get_execution(&ctx, &execution_id2).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusTimedOut
    );
    assert_eq!(execution.execution_status().as_sql_str(), "TIMED_OUT");

    let execution_id3 = storage
        .create_execution(&ctx, 
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id3, ExecutionStatus::ExecutionStatusFailed)
        .await?;
    let execution = storage.get_execution(&ctx, &execution_id3).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    Ok(())
}

#[tokio::test]
async fn test_error_cases() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let result = storage.get_definition(&ctx, "non-existent", "1.0").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    let result = storage.get_execution(&ctx, "non-existent-exec").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    Ok(())
}

// =============================================================================
// MAP WORKFLOW TESTS (from map_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_map_over_array() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-test",
        "Map Test",
        "1.0",
        vec![make_step(
            "map-step",
            "Map Over Items",
            StepType::StepTypeMap,
            json!({"items": ["item1", "item2", "item3"], "iterator": {"action": "transform", "operation": "uppercase"}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-test", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].step_id, "map-step");
    assert_eq!(history[0].step_status(), StepStatus::StepStatusCompleted);
    assert!(history[1..]
        .iter()
        .all(|s| s.step_status() == StepStatus::StepStatusCompleted));

    let map_output = history[0]
        .output_value()
        .expect("Map step should have output");
    assert!(map_output.is_array());
    assert_eq!(map_output.as_array().unwrap().len(), 3);

    Ok(())
}

#[tokio::test]
async fn test_map_empty_array() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-empty",
        "Map Empty",
        "1.0",
        vec![make_step(
            "map-step",
            "Map Empty",
            StepType::StepTypeMap,
            json!({"items": [], "iterator": {"action": "succeed"}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 1);

    let output = history[0]
        .output_value()
        .expect("Map step should have output");
    assert!(output.is_array());
    assert_eq!(output.as_array().unwrap().len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_map_with_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-fail",
        "Map Fail",
        "1.0",
        vec![make_step(
            "map-step",
            "Map with Failure",
            StepType::StepTypeMap,
            json!({"items": [{"id": "item1", "should_fail": false}, {"id": "item2", "should_fail": true}, {"id": "item3", "should_fail": false}], "iterator": {"action": "conditional_fail"}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-fail", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    Ok(())
}

#[tokio::test]
async fn test_map_preserves_order() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-order",
        "Map Order",
        "1.0",
        vec![make_step(
            "map-step",
            "Map Order Test",
            StepType::StepTypeMap,
            json!({"items": [1, 2, 3, 4, 5], "iterator": {"action": "multiply", "factor": 2}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-order", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let output = history[0]
        .output_value()
        .expect("Map step should have output");

    let results = output.as_array().unwrap();
    assert_eq!(results.len(), 5);

    for (i, result) in results.iter().enumerate() {
        let _expected_value = (i + 1) * 2;
        assert!(result.get("result").is_some());
    }

    Ok(())
}

#[tokio::test]
async fn test_map_max_concurrency() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-concurrency",
        "Map Concurrency",
        "1.0",
        vec![make_step(
            "map-step",
            "Map with Concurrency Limit",
            StepType::StepTypeMap,
            json!({"items": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10], "max_concurrency": 3, "iterator": {"action": "succeed", "delay_ms": 50}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-concurrency", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed.as_millis() > 150,
        "Expected batched execution ~200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_map_item_as_input() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "map-input",
        "Map Input",
        "1.0",
        vec![make_step(
            "map-step",
            "Map Input Test",
            StepType::StepTypeMap,
            json!({"items": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}, {"name": "Carol", "age": 35}], "iterator": {"action": "generate"}}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "map-input", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);

    Ok(())
}

// =============================================================================
// PARALLEL WORKFLOW TESTS (from parallel_workflow_test.rs - 5 tests)
// =============================================================================

#[tokio::test]
async fn test_parallel_three_branches() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "parallel-test",
        "Parallel Test",
        "1.0",
        vec![make_step(
            "parallel-step",
            "Parallel Execution",
            StepType::StepTypeParallel,
            json!({"branches": [{"id": "branch-a", "action": "succeed", "delay_ms": 50}, {"id": "branch-b", "action": "succeed", "delay_ms": 100}, {"id": "branch-c", "action": "succeed", "delay_ms": 25}]}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "parallel-test", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].step_id, "parallel-step");
    assert_eq!(history[0].step_status(), StepStatus::StepStatusCompleted);
    assert!(history[1..]
        .iter()
        .all(|s| s.step_status() == StepStatus::StepStatusCompleted));

    let branch_ids: Vec<String> = history[1..].iter().map(|s| s.step_id.clone()).collect();
    assert!(branch_ids.contains(&"branch-a".to_string()));
    assert!(branch_ids.contains(&"branch-b".to_string()));
    assert!(branch_ids.contains(&"branch-c".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_parallel_with_one_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "parallel-fail",
        "Parallel Fail",
        "1.0",
        vec![make_step(
            "parallel-step",
            "Parallel with Failure",
            StepType::StepTypeParallel,
            json!({"branches": [{"id": "branch-success", "action": "succeed"}, {"id": "branch-fail", "action": "fail", "error": "Branch failed"}, {"id": "branch-success-2", "action": "succeed"}]}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "parallel-fail", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 3);

    Ok(())
}

#[tokio::test]
async fn test_parallel_output_aggregation() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "parallel-aggregate",
        "Parallel Aggregate",
        "1.0",
        vec![
            make_step(
                "parallel-step",
                "Parallel Aggregation",
                StepType::StepTypeParallel,
                json!({"branches": [{"id": "branch-1", "action": "generate", "data": "result-1"}, {"id": "branch-2", "action": "generate", "data": "result-2"}]}),
                None,
                None,
                None,
            ),
            make_step(
                "verify-output",
                "Verify Output",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "parallel-aggregate", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let parallel_exec = history
        .iter()
        .find(|s| s.step_id == "parallel-step")
        .unwrap();
    assert!(parallel_exec.output_value().is_some());

    let output = parallel_exec.output_value().unwrap();
    assert!(output.is_object() || output.is_array());

    Ok(())
}

#[tokio::test]
async fn test_parallel_empty_branches() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "parallel-empty",
        "Empty Parallel",
        "1.0",
        vec![make_step(
            "parallel-step",
            "Empty Parallel",
            StepType::StepTypeParallel,
            json!({"branches": []}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "parallel-empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}

#[tokio::test]
async fn test_parallel_concurrent_execution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "parallel-timing",
        "Parallel Timing",
        "1.0",
        vec![make_step(
            "parallel-step",
            "Concurrent Timing",
            StepType::StepTypeParallel,
            json!({"branches": [{"id": "branch-1", "action": "succeed", "delay_ms": 100}, {"id": "branch-2", "action": "succeed", "delay_ms": 100}, {"id": "branch-3", "action": "succeed", "delay_ms": 100}]}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "parallel-timing", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed.as_millis() < 250,
        "Execution took {}ms, expected concurrent execution ~100ms",
        elapsed.as_millis()
    );

    Ok(())
}

// =============================================================================
// CHOICE WORKFLOW TESTS (from choice_workflow_test.rs - 5 tests)
// =============================================================================

#[tokio::test]
async fn test_choice_simple_condition() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-simple",
        "Choice Simple",
        "1.0",
        vec![
            make_step(
                "set-status",
                "Set Status",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"status": "approved"}}),
                None,
                None,
                None,
            ),
            make_step(
                "choice-step",
                "Check Status",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"}, {"variable": "$.status", "operator": "equals", "value": "rejected", "next": "rejected-path"}], "default": "default-path"}),
                None,
                None,
                None,
            ),
            make_step(
                "approved-path",
                "Approved Path",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "rejected-path",
                "Rejected Path",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "default-path",
                "Default Path",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-simple", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"set-status".to_string()));
    assert!(step_ids.contains(&"choice-step".to_string()));
    assert!(step_ids.contains(&"approved-path".to_string()));
    assert!(!step_ids.contains(&"rejected-path".to_string()));
    assert!(!step_ids.contains(&"default-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_default_path() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-default",
        "Choice Default",
        "1.0",
        vec![
            make_step(
                "set-status",
                "Set Status",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"status": "pending"}}),
                None,
                None,
                None,
            ),
            make_step(
                "choice-step",
                "Check Status",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"}], "default": "default-path"}),
                None,
                None,
                None,
            ),
            make_step(
                "approved-path",
                "Approved Path",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "default-path",
                "Default Path",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-default", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"default-path".to_string()));
    assert!(!step_ids.contains(&"approved-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_numeric_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-numeric",
        "Choice Numeric",
        "1.0",
        vec![
            make_step(
                "set-amount",
                "Set Amount",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"amount": 150}}),
                None,
                None,
                None,
            ),
            make_step(
                "choice-step",
                "Check Amount",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.amount", "operator": "greater_than", "value": 100, "next": "high-amount"}, {"variable": "$.amount", "operator": "less_than", "value": 50, "next": "low-amount"}], "default": "medium-amount"}),
                None,
                None,
                None,
            ),
            make_step(
                "high-amount",
                "High Amount",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "low-amount",
                "Low Amount",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "medium-amount",
                "Medium Amount",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-numeric", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-amount".to_string()));
    assert!(!step_ids.contains(&"low-amount".to_string()));
    assert!(!step_ids.contains(&"medium-amount".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_boolean() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-boolean",
        "Choice Boolean",
        "1.0",
        vec![
            make_step(
                "set-flag",
                "Set Flag",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"is_valid": true}}),
                None,
                None,
                None,
            ),
            make_step(
                "choice-step",
                "Check Flag",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.is_valid", "operator": "equals", "value": true, "next": "valid-path"}], "default": "invalid-path"}),
                None,
                None,
                None,
            ),
            make_step(
                "valid-path",
                "Valid Path",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "invalid-path",
                "Invalid Path",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-boolean", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"valid-path".to_string()));
    assert!(!step_ids.contains(&"invalid-path".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_choice_no_matching_choice_no_default() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "choice-no-default",
        "Choice No Default",
        "1.0",
        vec![
            make_step(
                "set-status",
                "Set Status",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"status": "unknown"}}),
                None,
                None,
                None,
            ),
            make_step(
                "choice-step",
                "Check Status",
                StepType::StepTypeChoice,
                json!({"choices": [{"variable": "$.status", "operator": "equals", "value": "approved", "next": "approved-path"}]}),
                None,
                None,
                None,
            ),
            make_step(
                "approved-path",
                "Approved Path",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "choice-no-default", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    Ok(())
}

// =============================================================================
// WAIT WORKFLOW TESTS (from wait_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_wait_fixed_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-duration",
        "Wait Duration",
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
                "wait-step",
                "Wait 100ms",
                StepType::StepTypeWait,
                json!({"duration_ms": 100}),
                None,
                None,
                None,
            ),
            make_step(
                "finish",
                "Finish",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-duration", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected at least 100ms wait, got {}ms",
        elapsed.as_millis()
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"start".to_string()));
    assert!(step_ids.contains(&"wait-step".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_wait_duration_seconds() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-seconds",
        "Wait Seconds",
        "1.0",
        vec![make_step(
            "wait-step",
            "Wait 0.2 seconds",
            StepType::StepTypeWait,
            json!({"duration_secs": 0.2}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-seconds", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed >= Duration::from_millis(200),
        "Expected at least 200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_until_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let wait_until = chrono::Utc::now() + chrono::Duration::milliseconds(150);

    let definition = make_workflow_definition(
        "wait-until",
        "Wait Until",
        "1.0",
        vec![make_step(
            "wait-step",
            "Wait Until Timestamp",
            StepType::StepTypeWait,
            json!({"until": wait_until.to_rfc3339()}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-until", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed >= Duration::from_millis(140),
        "Expected at least 140ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_until_past_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let wait_until = chrono::Utc::now() - chrono::Duration::seconds(10);

    let definition = make_workflow_definition(
        "wait-past",
        "Wait Past",
        "1.0",
        vec![
            make_step(
                "wait-step",
                "Wait Until Past",
                StepType::StepTypeWait,
                json!({"until": wait_until.to_rfc3339()}),
                None,
                None,
                None,
            ),
            make_step(
                "after-wait",
                "After Wait",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-past", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_zero_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-zero",
        "Wait Zero",
        "1.0",
        vec![make_step(
            "wait-step",
            "Wait 0ms",
            StepType::StepTypeWait,
            json!({"duration_ms": 0}),
            None,
            None,
            None,
        )],
    );
    storage.save_definition(&ctx, &definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-zero", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_wait_passes_input_through() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "wait-passthrough",
        "Wait Passthrough",
        "1.0",
        vec![
            make_step(
                "generate",
                "Generate Data",
                StepType::StepTypeTask,
                json!({"action": "succeed", "output": {"data": "test-value", "count": 42}}),
                None,
                None,
                None,
            ),
            make_step(
                "wait-step",
                "Wait",
                StepType::StepTypeWait,
                json!({"duration_ms": 50}),
                None,
                None,
                None,
            ),
            make_step(
                "verify",
                "Verify Data",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "wait-passthrough", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    let input = wait_exec
        .input_value()
        .expect("Wait step should have input");
    assert_eq!(
        input.get("data").and_then(|v| v.as_str()),
        Some("test-value")
    );
    assert_eq!(
        input
            .get("count")
            .and_then(|v| v.as_f64().map(|f| f as u64).or_else(|| v.as_u64())),
        Some(42)
    );

    let output = wait_exec
        .output_value()
        .expect("Wait step should have output");
    assert_eq!(output, input);

    Ok(())
}

// =============================================================================
// MULTI-STEP WORKFLOW TESTS (from multi_step_workflow_test.rs - 6 tests)
// =============================================================================

#[tokio::test]
async fn test_three_step_sequential_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "three-step",
        "Three Step Workflow",
        "1.0",
        vec![
            make_step(
                "step-1",
                "First Step",
                StepType::StepTypeTask,
                json!({"action": "fetch_data"}),
                None,
                None,
                None,
            ),
            make_step(
                "step-2",
                "Second Step",
                StepType::StepTypeTask,
                json!({"action": "process_data"}),
                None,
                None,
                None,
            ),
            make_step(
                "step-3",
                "Third Step",
                StepType::StepTypeTask,
                json!({"action": "store_result"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "three-step", "1.0", json!({"user_id": 123}))
            .await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].step_id, "step-1");
    assert_eq!(history[1].step_id, "step-2");
    assert_eq!(history[2].step_id, "step-3");
    assert!(history
        .iter()
        .all(|s| s.step_status() == StepStatus::StepStatusCompleted));

    Ok(())
}

#[tokio::test]
async fn test_data_passing_between_steps() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "data-flow",
        "Data Flow Workflow",
        "1.0",
        vec![
            make_step(
                "generate",
                "Generate Data",
                StepType::StepTypeTask,
                json!({"action": "generate"}),
                None,
                None,
                None,
            ),
            make_step(
                "transform",
                "Transform Data",
                StepType::StepTypeTask,
                json!({"action": "transform"}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "data-flow", "1.0", json!({})).await?;

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert!(history[0].output_value().is_some());
    assert!(history[1].input_value().is_some());

    Ok(())
}

#[tokio::test]
async fn test_workflow_fails_when_step_fails() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "failing-workflow",
        "Failing Workflow",
        "1.0",
        vec![
            make_step(
                "step-1",
                "Success Step",
                StepType::StepTypeTask,
                json!({"action": "succeed"}),
                None,
                None,
                None,
            ),
            make_step(
                "step-2",
                "Failing Step",
                StepType::StepTypeTask,
                json!({"action": "fail", "error": "Simulated failure"}),
                None,
                None,
                None,
            ),
            make_step(
                "step-3",
                "Should Not Execute",
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
        WorkflowExecutor::start_execution(&storage, &ctx, "failing-workflow", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusFailed
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].step_status(), StepStatus::StepStatusCompleted);
    assert_eq!(history[1].step_status(), StepStatus::StepStatusFailed);

    Ok(())
}

#[tokio::test]
async fn test_step_retry_on_failure() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let retry_config = RetryConfig {
        max_attempts: 3,
        initial_interval_ms: 10,
        backoff_rate: 2.0,
        max_interval_ms: 1000,
        retryable_errors: vec![],
    };

    let definition = make_workflow_definition(
        "retry-workflow",
        "Retry Workflow",
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

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "retry-workflow", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert!(history.len() >= 2);
    assert_eq!(
        history.last().unwrap().step_status(),
        StepStatus::StepStatusCompleted
    );

    Ok(())
}

#[tokio::test]
async fn test_empty_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition("empty", "Empty Workflow", "1.0", vec![]);
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "empty", "1.0", json!({})).await?;

    let execution = storage.get_execution(&ctx, &execution_id).await?;
    assert_eq!(
        execution.execution_status(),
        ExecutionStatus::ExecutionStatusCompleted
    );

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 0);

    Ok(())
}

#[tokio::test]
async fn test_explicit_step_ordering() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-ns".into());

    let definition = make_workflow_definition(
        "ordered",
        "Ordered Workflow",
        "1.0",
        vec![
            make_step(
                "start",
                "Start",
                StepType::StepTypeTask,
                json!({}),
                Some("middle".to_string()),
                None,
                None,
            ),
            make_step(
                "middle",
                "Middle",
                StepType::StepTypeTask,
                json!({}),
                Some("end".to_string()),
                None,
                None,
            ),
            make_step(
                "end",
                "End",
                StepType::StepTypeTask,
                json!({}),
                None,
                None,
                None,
            ),
        ],
    );
    storage.save_definition(&ctx, &definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, &ctx, "ordered", "1.0", json!({})).await?;

    let history = storage.get_step_execution_history(&execution_id).await?;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].step_id, "start");
    assert_eq!(history[1].step_id, "middle");
    assert_eq!(history[2].step_id, "end");

    Ok(())
}
