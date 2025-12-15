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

//! Workflow Actor Integration
//!
//! ## Purpose
//! Provides workflow execution as a first-class actor capability.
//! Actors can execute workflows with full lifecycle management and supervision.
//!
//! ## Features
//! - Start workflows via actor messages
//! - Query workflow execution status
//! - Cancel running workflows
//! - Send signals to waiting workflows
//! - Resume paused workflows
//! - Integration with actor supervision

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::executor::WorkflowExecutor;
use crate::storage::WorkflowStorage;
use crate::types::{ExecutionStatus, WorkflowError};

/// Message types for workflow control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowMessage {
    /// Start a new workflow execution
    Start {
        definition_id: String,
        definition_version: String,
        input: Value,
    },

    /// Query workflow execution status
    Query { execution_id: String },

    /// Cancel a running workflow
    Cancel { execution_id: String },

    /// Send a signal to a waiting workflow
    Signal {
        execution_id: String,
        signal_name: String,
        payload: Value,
    },

    /// Resume a paused workflow
    Resume { execution_id: String },
}

/// Response types from workflow operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowResponse {
    /// Workflow started successfully
    Started { execution_id: String },

    /// Workflow status query response
    Status {
        status: ExecutionStatus,
        output: Option<Value>,
    },

    /// Workflow cancelled
    Cancelled,

    /// Signal sent successfully
    SignalSent,

    /// Workflow resumed
    Resumed,

    /// Error occurred
    Error { message: String },
}

/// Workflow Actor - executes workflows as an actor behavior
///
/// ## Purpose
/// Wraps workflow execution in an actor, providing:
/// - Message-based workflow control
/// - Integration with actor lifecycle
/// - Support for supervision and fault tolerance
///
/// ## Usage
/// ```rust,no_run
/// use plexspaces_workflow::*;
/// use serde_json::json;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let storage = WorkflowStorage::new_in_memory().await?;
/// let actor = WorkflowActor::new("my-workflow-actor", storage).await?;
///
/// // Start a workflow
/// let msg = WorkflowMessage::Start {
///     definition_id: "my-workflow".to_string(),
///     definition_version: "1.0".to_string(),
///     input: json!({}),
/// };
///
/// let response = actor.handle_message(msg).await?;
/// # Ok(())
/// # }
/// ```
pub struct WorkflowActor {
    /// Actor ID
    id: String,

    /// Workflow storage
    storage: WorkflowStorage,

    /// Active workflow executions (execution_id -> handle)
    active_executions: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl WorkflowActor {
    /// Create a new workflow actor
    ///
    /// ## Arguments
    /// * `id` - Actor ID
    /// * `storage` - Workflow storage
    ///
    /// ## Returns
    /// New WorkflowActor instance
    pub async fn new(
        id: impl Into<String>,
        storage: WorkflowStorage,
    ) -> Result<Self, WorkflowError> {
        Ok(Self {
            id: id.into(),
            storage,
            active_executions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Handle a workflow message
    ///
    /// ## Arguments
    /// * `message` - Workflow control message
    ///
    /// ## Returns
    /// WorkflowResponse indicating result of operation
    ///
    /// ## Errors
    /// WorkflowError if operation fails
    pub async fn handle_message(
        &self,
        message: WorkflowMessage,
    ) -> Result<WorkflowResponse, WorkflowError> {
        match message {
            WorkflowMessage::Start {
                definition_id,
                definition_version,
                input,
            } => {
                self.start_workflow(definition_id, definition_version, input)
                    .await
            }
            WorkflowMessage::Query { execution_id } => self.query_workflow(execution_id).await,
            WorkflowMessage::Cancel { execution_id } => self.cancel_workflow(execution_id).await,
            WorkflowMessage::Signal {
                execution_id,
                signal_name,
                payload,
            } => self.send_signal(execution_id, signal_name, payload).await,
            WorkflowMessage::Resume { execution_id } => self.resume_workflow(execution_id).await,
        }
    }

    /// Start a new workflow execution
    async fn start_workflow(
        &self,
        definition_id: String,
        definition_version: String,
        input: Value,
    ) -> Result<WorkflowResponse, WorkflowError> {
        // Start workflow execution
        let execution_id = WorkflowExecutor::start_execution(
            &self.storage,
            &definition_id,
            &definition_version,
            input,
        )
        .await?;

        Ok(WorkflowResponse::Started { execution_id })
    }

    /// Query workflow execution status
    async fn query_workflow(
        &self,
        execution_id: String,
    ) -> Result<WorkflowResponse, WorkflowError> {
        let execution = self.storage.get_execution(&execution_id).await?;

        Ok(WorkflowResponse::Status {
            status: execution.status,
            output: execution.output,
        })
    }

    /// Cancel a running workflow
    async fn cancel_workflow(
        &self,
        execution_id: String,
    ) -> Result<WorkflowResponse, WorkflowError> {
        // Update execution status to cancelled
        self.storage
            .update_execution_status(&execution_id, ExecutionStatus::Cancelled)
            .await?;

        // Cancel the execution task if it's running
        let mut executions = self.active_executions.write().await;
        if let Some(handle) = executions.remove(&execution_id) {
            handle.abort();
        }

        Ok(WorkflowResponse::Cancelled)
    }

    /// Send a signal to a waiting workflow
    async fn send_signal(
        &self,
        execution_id: String,
        signal_name: String,
        payload: Value,
    ) -> Result<WorkflowResponse, WorkflowError> {
        self.storage
            .send_signal(&execution_id, &signal_name, payload)
            .await?;
        Ok(WorkflowResponse::SignalSent)
    }

    /// Resume a paused workflow
    async fn resume_workflow(
        &self,
        execution_id: String,
    ) -> Result<WorkflowResponse, WorkflowError> {
        // Execute workflow from current state
        WorkflowExecutor::execute_from_state(&self.storage, &execution_id).await?;
        Ok(WorkflowResponse::Resumed)
    }

    /// Get actor ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get workflow storage
    pub fn storage(&self) -> &WorkflowStorage {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Step, StepType, WorkflowDefinition};
    use serde_json::json;

    #[tokio::test]
    async fn test_workflow_actor_creation() {
        let storage = WorkflowStorage::new_in_memory().await.unwrap();
        let actor = WorkflowActor::new("test-actor", storage).await.unwrap();
        assert_eq!(actor.id(), "test-actor");
    }

    #[tokio::test]
    async fn test_workflow_actor_start_simple() {
        let storage = WorkflowStorage::new_in_memory().await.unwrap();

        // Create simple workflow
        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![Step {
                id: "step1".to_string(),
                name: "Step 1".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            }],
            ..Default::default()
        };
        storage.save_definition(&definition).await.unwrap();

        let actor = WorkflowActor::new("test-actor", storage.clone())
            .await
            .unwrap();

        // Start workflow
        let msg = WorkflowMessage::Start {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: json!({}),
        };

        let response = actor.handle_message(msg).await.unwrap();

        // Verify started
        match response {
            WorkflowResponse::Started { execution_id } => {
                assert!(!execution_id.is_empty());

                // Verify execution exists
                let execution = storage.get_execution(&execution_id).await.unwrap();
                assert_eq!(execution.status, ExecutionStatus::Completed);
            }
            _ => panic!("Expected Started response"),
        }
    }

    #[tokio::test]
    async fn test_workflow_actor_query() {
        let storage = WorkflowStorage::new_in_memory().await.unwrap();

        // Create workflow
        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
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
        storage.save_definition(&definition).await.unwrap();

        let actor = WorkflowActor::new("test-actor", storage.clone())
            .await
            .unwrap();

        // Start workflow
        let start_msg = WorkflowMessage::Start {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: json!({}),
        };

        let start_response = actor.handle_message(start_msg).await.unwrap();

        if let WorkflowResponse::Started { execution_id } = start_response {
            // Query status
            let query_msg = WorkflowMessage::Query { execution_id };
            let query_response = actor.handle_message(query_msg).await.unwrap();

            // Verify status
            match query_response {
                WorkflowResponse::Status { status, output } => {
                    assert_eq!(status, ExecutionStatus::Completed);
                    assert!(output.is_some());
                }
                _ => panic!("Expected Status response"),
            }
        }
    }

    #[tokio::test]
    async fn test_workflow_actor_cancel() {
        let storage = WorkflowStorage::new_in_memory().await.unwrap();

        // Create simple workflow
        let definition = WorkflowDefinition {
            id: "test-workflow".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![Step {
                id: "step1".to_string(),
                name: "Step 1".to_string(),
                step_type: StepType::Task,
                config: json!({"action": "succeed"}),
                ..Default::default()
            }],
            ..Default::default()
        };
        storage.save_definition(&definition).await.unwrap();

        let actor = WorkflowActor::new("test-actor", storage.clone())
            .await
            .unwrap();

        // Start workflow
        let start_msg = WorkflowMessage::Start {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: json!({}),
        };

        let start_response = actor.handle_message(start_msg).await.unwrap();

        if let WorkflowResponse::Started { execution_id } = start_response {
            // Cancel workflow
            let cancel_msg = WorkflowMessage::Cancel {
                execution_id: execution_id.clone(),
            };
            let cancel_response = actor.handle_message(cancel_msg).await.unwrap();

            // Verify cancelled
            match cancel_response {
                WorkflowResponse::Cancelled => {
                    // Check execution status
                    let execution = storage.get_execution(&execution_id).await.unwrap();
                    assert_eq!(execution.status, ExecutionStatus::Cancelled);
                }
                _ => panic!("Expected Cancelled response"),
            }
        }
    }

    #[tokio::test]
    async fn test_workflow_actor_signal() {
        let storage = WorkflowStorage::new_in_memory().await.unwrap();

        // Create workflow with signal step
        let definition = WorkflowDefinition {
            id: "signal-workflow".to_string(),
            name: "Signal Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![Step {
                id: "wait-signal".to_string(),
                name: "Wait Signal".to_string(),
                step_type: StepType::Signal,
                config: json!({
                    "signal_name": "approval",
                    "timeout_ms": 5000
                }),
                ..Default::default()
            }],
            ..Default::default()
        };
        storage.save_definition(&definition).await.unwrap();

        let actor = WorkflowActor::new("test-actor", storage.clone())
            .await
            .unwrap();

        // Create execution manually (don't start yet)
        let execution_id = storage
            .create_execution("signal-workflow", "1.0", json!({}), HashMap::new())
            .await
            .unwrap();

        // Send signal
        let signal_msg = WorkflowMessage::Signal {
            execution_id: execution_id.clone(),
            signal_name: "approval".to_string(),
            payload: json!({"approved": true}),
        };

        let signal_response = actor.handle_message(signal_msg).await.unwrap();

        // Verify signal sent
        match signal_response {
            WorkflowResponse::SignalSent => {
                // Verify signal stored
                let signal = storage
                    .check_signal(&execution_id, "approval")
                    .await
                    .unwrap();
                assert!(signal.is_some());
            }
            _ => panic!("Expected SignalSent response"),
        }
    }
}
