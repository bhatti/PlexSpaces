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

//! Signal step workflow execution tests
//!
//! ## Purpose
//! Tests for Signal step (Awakeable pattern - wait for external signals).
//! Following TDD principles - these tests are written FIRST (RED phase).

use plexspaces_workflow::*;
use serde_json::json;
use std::time::Duration;

/// Test signal with immediate resolution (signal already available)
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement Signal step execution
#[tokio::test]
async fn test_signal_immediate_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-immediate".to_string(),
        name: "Signal Immediate".to_string(),
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
                id: "wait-signal".to_string(),
                name: "Wait for Signal".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "finish".to_string(),
                name: "Finish".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (don't start yet)
    let execution_id = storage
        .create_execution(
            "signal-immediate",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Send signal BEFORE starting execution (immediate resolution)
    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await?;

    // Now start execution - signal is already available
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    // Execution should complete after receiving signal
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify signal step received the signal data
    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    assert_eq!(signal_exec.status, StepExecutionStatus::Completed);
    let output = signal_exec.output.as_ref().unwrap();
    assert_eq!(output.get("approved").and_then(|v| v.as_bool()), Some(true));

    Ok(())
}

/// Test signal with timeout (signal not received in time)
///
/// ## TDD Test (RED)
/// Signal step should fail if timeout expires before signal received
#[tokio::test]
async fn test_signal_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-timeout".to_string(),
        name: "Signal Timeout".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "approval",
                "timeout_ms": 100  // Short timeout
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = std::time::Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "signal-timeout", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    // Workflow should fail due to timeout
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    // Should have waited approximately 100ms
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

/// Test signal with delayed resolution (signal arrives during wait)
///
/// ## TDD Test (RED)
/// Simulate concurrent signal arrival while workflow is waiting
#[tokio::test]
async fn test_signal_delayed_resolution() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-delayed".to_string(),
        name: "Signal Delayed".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "payment-complete",
                "timeout_ms": 2000
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id = storage
        .create_execution(
            "signal-delayed",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Spawn background task to send signal after 50ms
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

    // Start execution (will wait for signal)
    let start = std::time::Instant::now();
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;

    // Execute workflow (should receive signal after ~50ms)
    let result = WorkflowExecutor::execute_from_state(&storage, &execution_id).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Execution should succeed");

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have waited approximately 50ms (signal arrival time)
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

/// Test signal with custom data payload
///
/// ## TDD Test (RED)
/// Signal should pass payload to next step
#[tokio::test]
async fn test_signal_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-payload".to_string(),
        name: "Signal Payload".to_string(),
        version: "1.0".to_string(),
        steps: vec![
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
                id: "process-approval".to_string(),
                name: "Process Approval".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (don't start yet)
    let execution_id = storage
        .create_execution(
            "signal-payload",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Send signal with custom payload BEFORE starting
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

    // Now start execution - signal is already available
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify signal payload was passed through
    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-approval")
        .expect("Signal step should have executed");

    let output = signal_exec.output.as_ref().unwrap();
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

/// Test multiple signals in sequence
///
/// ## TDD Test (RED)
/// Workflow with multiple signal steps should handle each independently
#[tokio::test]
async fn test_multiple_signals_in_sequence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "multi-signal".to_string(),
        name: "Multi Signal".to_string(),
        version: "1.0".to_string(),
        steps: vec![
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
                id: "wait-payment".to_string(),
                name: "Wait for Payment".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "payment",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
            Step {
                id: "finish".to_string(),
                name: "Finish".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (don't start yet)
    let execution_id = storage
        .create_execution(
            "multi-signal",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Send both signals BEFORE starting
    storage
        .send_signal(&execution_id, "approval", json!({"approved": true}))
        .await?;
    storage
        .send_signal(&execution_id, "payment", json!({"paid": true}))
        .await?;

    // Now start execution - both signals already available
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify both signal steps executed
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"wait-approval".to_string()));
    assert!(step_ids.contains(&"wait-payment".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

/// Test signal with no timeout (wait indefinitely)
///
/// ## TDD Test (RED)
/// Signal without timeout should wait until signal received
#[tokio::test]
async fn test_signal_no_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-no-timeout".to_string(),
        name: "Signal No Timeout".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-signal".to_string(),
            name: "Wait for Signal".to_string(),
            step_type: StepType::Signal,
            config: json!({
                "signal_name": "notification"
                // No timeout specified
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (don't start yet)
    let execution_id = storage
        .create_execution(
            "signal-no-timeout",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Send signal BEFORE starting
    storage
        .send_signal(&execution_id, "notification", json!({"message": "done"}))
        .await?;

    // Now start execution - signal already available
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

/// Test signal step merges input with signal data
///
/// ## TDD Test (RED)
/// Signal step should merge workflow input with signal payload
#[tokio::test]
async fn test_signal_merges_input_with_payload() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "signal-merge".to_string(),
        name: "Signal Merge".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate Data".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"order_id": "12345", "status": "pending"}
                }),
                ..Default::default()
            },
            Step {
                id: "wait-signal".to_string(),
                name: "Wait for Signal".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "update",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (don't start yet)
    let execution_id = storage
        .create_execution(
            "signal-merge",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    // Send signal with additional data BEFORE starting
    storage
        .send_signal(
            &execution_id,
            "update",
            json!({"status": "approved", "approved_by": "bob"}),
        )
        .await?;

    // Now start execution - signal already available
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    WorkflowExecutor::execute_from_state(&storage, &execution_id).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify signal output merged input with signal payload
    let history = storage.get_step_execution_history(&execution_id).await?;
    let signal_exec = history
        .iter()
        .find(|s| s.step_id == "wait-signal")
        .expect("Signal step should have executed");

    let output = signal_exec.output.as_ref().unwrap();
    // Should have original order_id
    assert_eq!(
        output.get("order_id").and_then(|v| v.as_str()),
        Some("12345")
    );
    // Should have updated status from signal
    assert_eq!(
        output.get("status").and_then(|v| v.as_str()),
        Some("approved")
    );
    // Should have new field from signal
    assert_eq!(
        output.get("approved_by").and_then(|v| v.as_str()),
        Some("bob")
    );

    Ok(())
}
