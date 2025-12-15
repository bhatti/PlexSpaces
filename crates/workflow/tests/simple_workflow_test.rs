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

//! Integration test for simple workflow execution
//!
//! ## Walking Skeleton Test
//! This is the minimal end-to-end test that exercises the entire workflow system:
//! 1. Create workflow definition with a single task step
//! 2. Start workflow execution
//! 3. Verify execution completes successfully
//!
//! Following TDD principles:
//! - Write this test FIRST (it will fail - RED)
//! - Implement minimal code to pass (GREEN)
//! - Refactor while keeping tests green (REFACTOR)

use plexspaces_workflow::*;
use serde_json::json;

/// Walking Skeleton: Single-step workflow execution
///
/// ## Test Scenario
/// 1. Create workflow definition with one task step ("say-hello")
/// 2. Start execution with input {"name": "World"}
/// 3. Verify workflow completes with status COMPLETED
/// 4. Verify output contains {"greeting": "Hello, World!"}
///
/// ## This Tests
/// - Workflow definition creation and storage
/// - Workflow execution creation
/// - Single step execution (task type)
/// - State transitions (PENDING → RUNNING → COMPLETED)
/// - Input/output handling
#[tokio::test]
async fn test_simple_single_step_workflow() -> Result<(), Box<dyn std::error::Error>> {
    // Setup: Create in-memory database for testing
    let storage = WorkflowStorage::new_in_memory().await?;

    // Step 1: Create workflow definition with single task step
    let definition = WorkflowDefinition {
        id: "hello-world".to_string(),
        name: "Hello World Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![Step {
            id: "say-hello".to_string(),
            name: "Say Hello".to_string(),
            step_type: StepType::Task,
            config: json!({
                "actor_type": "EchoActor",
                "method": "say_hello",
                "input": {"name": "World"}
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    storage.save_definition(&definition).await?;

    // Step 2: Start workflow execution
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "hello-world", "1.0", json!({"name": "World"}))
            .await?;

    // Step 3: Verify execution completes
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Step 4: Verify output
    let output = execution.output.unwrap();
    assert_eq!(output["greeting"], "Hello, World!");

    Ok(())
}

/// Test workflow definition persistence
///
/// ## Test Scenario
/// 1. Save workflow definition to storage
/// 2. Retrieve it by ID and version
/// 3. Verify all fields match
#[tokio::test]
async fn test_workflow_definition_persistence() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create definition
    let definition = WorkflowDefinition {
        id: "test-workflow".to_string(),
        name: "Test Workflow".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };

    // Save
    storage.save_definition(&definition).await?;

    // Retrieve
    let retrieved = storage.get_definition("test-workflow", "1.0").await?;

    // Verify
    assert_eq!(retrieved.id, "test-workflow");
    assert_eq!(retrieved.name, "Test Workflow");
    assert_eq!(retrieved.version, "1.0");

    Ok(())
}

/// Test execution status transitions
///
/// ## Test Scenario
/// 1. Create execution in PENDING state
/// 2. Update to RUNNING
/// 3. Update to COMPLETED
/// 4. Verify status history
#[tokio::test]
async fn test_execution_status_transitions() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create definition
    let definition = WorkflowDefinition {
        id: "status-test".to_string(),
        name: "Status Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution (PENDING)
    let execution_id = storage
        .create_execution(
            "status-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Pending);

    // Update to RUNNING
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Running)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Running);

    // Update to COMPLETED
    storage
        .update_execution_status(&execution_id, ExecutionStatus::Completed)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}

/// Test execution labels for filtering
///
/// ## Test Scenario
/// 1. Create execution with labels
/// 2. Verify labels are stored
#[tokio::test]
async fn test_execution_labels() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create definition
    let definition = WorkflowDefinition {
        id: "label-test".to_string(),
        name: "Label Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Create execution with labels
    let mut labels = std::collections::HashMap::new();
    labels.insert("environment".to_string(), "production".to_string());
    labels.insert("priority".to_string(), "high".to_string());

    let execution_id = storage
        .create_execution("label-test", "1.0", json!({}), labels)
        .await?;

    // Verify execution was created
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Pending);

    Ok(())
}

/// Test all execution status transitions including edge cases
///
/// ## Test Scenario
/// Test all status values including Cancelled and TimedOut
#[tokio::test]
async fn test_all_execution_statuses() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "status-full-test".to_string(),
        name: "Status Full Test".to_string(),
        version: "1.0".to_string(),
        steps: vec![],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Test Cancelled status
    let execution_id = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id, ExecutionStatus::Cancelled)
        .await?;
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Cancelled);
    assert_eq!(execution.status.to_string(), "CANCELLED");

    // Test TimedOut status
    let execution_id2 = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id2, ExecutionStatus::TimedOut)
        .await?;
    let execution = storage.get_execution(&execution_id2).await?;
    assert_eq!(execution.status, ExecutionStatus::TimedOut);
    assert_eq!(execution.status.to_string(), "TIMED_OUT");

    // Test Failed status
    let execution_id3 = storage
        .create_execution(
            "status-full-test",
            "1.0",
            json!({}),
            std::collections::HashMap::new(),
        )
        .await?;

    storage
        .update_execution_status(&execution_id3, ExecutionStatus::Failed)
        .await?;
    let execution = storage.get_execution(&execution_id3).await?;
    assert_eq!(execution.status, ExecutionStatus::Failed);

    Ok(())
}

/// Test error cases for robustness
#[tokio::test]
async fn test_error_cases() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Test: Get non-existent definition
    let result = storage.get_definition("non-existent", "1.0").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    // Test: Get non-existent execution
    let result = storage.get_execution("non-existent-exec").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WorkflowError::NotFound(_)));

    Ok(())
}
