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

//! Wait step workflow execution tests
//!
//! ## Purpose
//! Tests for Wait step (delay execution).
//! Following TDD principles - these tests are written FIRST (RED phase).

use plexspaces_workflow::*;
use serde_json::json;
use std::time::{Duration, Instant};

/// Test wait with fixed duration
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement Wait step execution
#[tokio::test]
async fn test_wait_fixed_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-duration".to_string(),
        name: "Wait Duration".to_string(),
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
                id: "wait-step".to_string(),
                name: "Wait 100ms".to_string(),
                step_type: StepType::Wait,
                config: json!({
                    "duration_ms": 100
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

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-duration", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify actual wait occurred (at least 100ms)
    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected at least 100ms wait, got {}ms",
        elapsed.as_millis()
    );

    // Verify all steps executed
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"start".to_string()));
    assert!(step_ids.contains(&"wait-step".to_string()));
    assert!(step_ids.contains(&"finish".to_string()));

    Ok(())
}

/// Test wait with duration in seconds
///
/// ## TDD Test (RED)
/// Support both milliseconds and seconds configuration
#[tokio::test]
async fn test_wait_duration_seconds() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-seconds".to_string(),
        name: "Wait Seconds".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait 0.2 seconds".to_string(),
            step_type: StepType::Wait,
            config: json!({
                "duration_secs": 0.2
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-seconds", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have waited at least 200ms
    assert!(
        elapsed >= Duration::from_millis(200),
        "Expected at least 200ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

/// Test wait until specific timestamp
///
/// ## TDD Test (RED)
/// Wait until a specific time (past time should complete immediately)
#[tokio::test]
async fn test_wait_until_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Set timestamp 150ms in the future
    let wait_until = chrono::Utc::now() + chrono::Duration::milliseconds(150);

    let definition = WorkflowDefinition {
        id: "wait-until".to_string(),
        name: "Wait Until".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait Until Timestamp".to_string(),
            step_type: StepType::Wait,
            config: json!({
                "until": wait_until.to_rfc3339()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-until", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have waited approximately 150ms
    assert!(
        elapsed >= Duration::from_millis(140),
        "Expected at least 140ms, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

/// Test wait until past timestamp (should complete immediately)
///
/// ## TDD Test (RED)
/// If timestamp is in the past, don't wait
#[tokio::test]
async fn test_wait_until_past_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Set timestamp in the past
    let wait_until = chrono::Utc::now() - chrono::Duration::seconds(10);

    let definition = WorkflowDefinition {
        id: "wait-past".to_string(),
        name: "Wait Past".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "wait-step".to_string(),
                name: "Wait Until Past".to_string(),
                step_type: StepType::Wait,
                config: json!({
                    "until": wait_until.to_rfc3339()
                }),
                ..Default::default()
            },
            Step {
                id: "after-wait".to_string(),
                name: "After Wait".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-past", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should complete quickly (no wait for past timestamp)
    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

/// Test wait with zero duration
///
/// ## TDD Test (RED)
/// Zero duration should complete immediately
#[tokio::test]
async fn test_wait_zero_duration() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-zero".to_string(),
        name: "Wait Zero".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "wait-step".to_string(),
            name: "Wait 0ms".to_string(),
            step_type: StepType::Wait,
            config: json!({
                "duration_ms": 0
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let start = Instant::now();
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-zero", "1.0", json!({})).await?;
    let elapsed = start.elapsed();

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should complete very quickly
    assert!(
        elapsed < Duration::from_millis(50),
        "Expected quick completion, got {}ms",
        elapsed.as_millis()
    );

    Ok(())
}

/// Test wait passes input through unchanged
///
/// ## TDD Test (RED)
/// Wait step should not modify workflow data
#[tokio::test]
async fn test_wait_passes_input_through() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "wait-passthrough".to_string(),
        name: "Wait Passthrough".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "generate".to_string(),
                name: "Generate Data".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"data": "test-value", "count": 42}
                }),
                ..Default::default()
            },
            Step {
                id: "wait-step".to_string(),
                name: "Wait".to_string(),
                step_type: StepType::Wait,
                config: json!({
                    "duration_ms": 50
                }),
                ..Default::default()
            },
            Step {
                id: "verify".to_string(),
                name: "Verify Data".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "wait-passthrough", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify wait step received and passed through the data
    let history = storage.get_step_execution_history(&execution_id).await?;
    let wait_exec = history
        .iter()
        .find(|s| s.step_id == "wait-step")
        .expect("Wait step should have executed");

    // Input should contain the generated data
    assert!(wait_exec.input.is_some());
    let input = wait_exec.input.as_ref().unwrap();
    assert_eq!(
        input.get("data").and_then(|v| v.as_str()),
        Some("test-value")
    );
    assert_eq!(input.get("count").and_then(|v| v.as_u64()), Some(42));

    // Output should be same as input
    assert!(wait_exec.output.is_some());
    let output = wait_exec.output.as_ref().unwrap();
    assert_eq!(output, input);

    Ok(())
}
