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

//! Choice workflow execution tests
//!
//! ## Purpose
//! Tests for conditional branching based on workflow state.
//! Following TDD principles - these tests are written FIRST (RED phase).

use plexspaces_workflow::*;
use serde_json::json;

/// Test choice with simple condition (equals)
///
/// ## TDD Test (RED)
/// This test will FAIL until we implement choice step execution
#[tokio::test]
async fn test_choice_simple_condition() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    // Create workflow with choice step
    let definition = WorkflowDefinition {
        id: "choice-simple".to_string(),
        name: "Choice Simple".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "approved"}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.status",
                            "operator": "equals",
                            "value": "approved",
                            "next": "approved-path"
                        },
                        {
                            "variable": "$.status",
                            "operator": "equals",
                            "value": "rejected",
                            "next": "rejected-path"
                        }
                    ],
                    "default": "default-path"
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "rejected-path".to_string(),
                name: "Rejected Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "default-path".to_string(),
                name: "Default Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    // Execute workflow
    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-simple", "1.0", json!({})).await?;

    // Verify workflow completed
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Verify choice took approved path
    let history = storage.get_step_execution_history(&execution_id).await?;

    // Should have executed: set-status, choice-step, approved-path
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"set-status".to_string()));
    assert!(step_ids.contains(&"choice-step".to_string()));
    assert!(step_ids.contains(&"approved-path".to_string()));

    // Should NOT have executed rejected-path or default-path
    assert!(!step_ids.contains(&"rejected-path".to_string()));
    assert!(!step_ids.contains(&"default-path".to_string()));

    Ok(())
}

/// Test choice with default path
///
/// ## TDD Test (RED)
/// If no choice matches, should take default path
#[tokio::test]
async fn test_choice_default_path() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-default".to_string(),
        name: "Choice Default".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "pending"}  // Doesn't match any choice
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.status",
                            "operator": "equals",
                            "value": "approved",
                            "next": "approved-path"
                        }
                    ],
                    "default": "default-path"
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "default-path".to_string(),
                name: "Default Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-default", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have taken default path
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"default-path".to_string()));
    assert!(!step_ids.contains(&"approved-path".to_string()));

    Ok(())
}

/// Test choice with numeric comparison
///
/// ## TDD Test (RED)
/// Support numeric comparison operators (greater_than, less_than)
#[tokio::test]
async fn test_choice_numeric_comparison() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-numeric".to_string(),
        name: "Choice Numeric".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-amount".to_string(),
                name: "Set Amount".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"amount": 150}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Amount".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.amount",
                            "operator": "greater_than",
                            "value": 100,
                            "next": "high-amount"
                        },
                        {
                            "variable": "$.amount",
                            "operator": "less_than",
                            "value": 50,
                            "next": "low-amount"
                        }
                    ],
                    "default": "medium-amount"
                }),
                ..Default::default()
            },
            Step {
                id: "high-amount".to_string(),
                name: "High Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "low-amount".to_string(),
                name: "Low Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "medium-amount".to_string(),
                name: "Medium Amount".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-numeric", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have taken high-amount path (150 > 100)
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"high-amount".to_string()));
    assert!(!step_ids.contains(&"low-amount".to_string()));
    assert!(!step_ids.contains(&"medium-amount".to_string()));

    Ok(())
}

/// Test choice with boolean condition
///
/// ## TDD Test (RED)
/// Support boolean true/false checks
#[tokio::test]
async fn test_choice_boolean() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-boolean".to_string(),
        name: "Choice Boolean".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-flag".to_string(),
                name: "Set Flag".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"is_valid": true}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Flag".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.is_valid",
                            "operator": "equals",
                            "value": true,
                            "next": "valid-path"
                        }
                    ],
                    "default": "invalid-path"
                }),
                ..Default::default()
            },
            Step {
                id: "valid-path".to_string(),
                name: "Valid Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
            Step {
                id: "invalid-path".to_string(),
                name: "Invalid Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-boolean", "1.0", json!({})).await?;

    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    // Should have taken valid-path
    let history = storage.get_step_execution_history(&execution_id).await?;
    let step_ids: Vec<String> = history.iter().map(|s| s.step_id.clone()).collect();
    assert!(step_ids.contains(&"valid-path".to_string()));
    assert!(!step_ids.contains(&"invalid-path".to_string()));

    Ok(())
}

/// Test choice missing default (should fail)
///
/// ## TDD Test (RED)
/// If no choice matches and no default, workflow should fail gracefully
#[tokio::test]
async fn test_choice_no_matching_choice_no_default() -> Result<(), Box<dyn std::error::Error>> {
    let storage = WorkflowStorage::new_in_memory().await?;

    let definition = WorkflowDefinition {
        id: "choice-no-default".to_string(),
        name: "Choice No Default".to_string(),
        version: "1.0".to_string(),
        steps: vec![
            Step {
                id: "set-status".to_string(),
                name: "Set Status".to_string(),
                step_type: StepType::Task,
                config: json!({
                    "action": "succeed",
                    "output": {"status": "unknown"}
                }),
                ..Default::default()
            },
            Step {
                id: "choice-step".to_string(),
                name: "Check Status".to_string(),
                step_type: StepType::Choice,
                config: json!({
                    "choices": [
                        {
                            "variable": "$.status",
                            "operator": "equals",
                            "value": "approved",
                            "next": "approved-path"
                        }
                    ]
                    // No default path
                }),
                ..Default::default()
            },
            Step {
                id: "approved-path".to_string(),
                name: "Approved Path".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    storage.save_definition(&definition).await?;

    let execution_id =
        WorkflowExecutor::start_execution(&storage, "choice-no-default", "1.0", json!({})).await?;

    // Workflow should complete (choice ends workflow if no match and no default)
    let execution = storage.get_execution(&execution_id).await?;
    assert_eq!(execution.status, ExecutionStatus::Completed);

    Ok(())
}
