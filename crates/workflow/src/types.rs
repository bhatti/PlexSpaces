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

//! Workflow type definitions
//!
//! ## Purpose
//! Core types for workflow orchestration following TDD principles.
//! Minimal implementation to support tests.
//!
//! ## Proto-First Design
//! Error types are defined in proto and wrapped here for thiserror compatibility.
//! See `proto/plexspaces/v1/workflow.proto` for the source of truth.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

// Re-export proto-generated error enum
pub use plexspaces_proto::workflow::v1::WorkflowError as WorkflowErrorProto;

/// Workflow error type (wraps proto enum for thiserror compatibility)
///
/// ## Proto-First Design
/// The proto enum is the source of truth. This wrapper provides:
/// - String messages (proto enum doesn't have payloads)
/// - thiserror::Error implementation
/// - Backward compatibility with existing code
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    /// Storage operation failed
    #[error("Storage error: {0}")]
    Storage(String),

    /// Serialization/deserialization failed
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Workflow or execution not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid workflow definition
    #[error("Invalid definition: {0}")]
    InvalidDefinition(String),

    /// Execution error
    #[error("Execution error: {0}")]
    Execution(String),

    /// Concurrent update detected (optimistic locking failure)
    #[error("Concurrent update: {0}")]
    ConcurrentUpdate(String),
}

impl From<WorkflowError> for WorkflowErrorProto {
    fn from(err: WorkflowError) -> Self {
        match err {
            WorkflowError::Storage(_) => WorkflowErrorProto::WorkflowErrorStorage,
            WorkflowError::Serialization(_) => WorkflowErrorProto::WorkflowErrorSerialization,
            WorkflowError::NotFound(_) => WorkflowErrorProto::WorkflowErrorNotFound,
            WorkflowError::InvalidDefinition(_) => WorkflowErrorProto::WorkflowErrorInvalidDefinition,
            WorkflowError::Execution(_) => WorkflowErrorProto::WorkflowErrorExecution,
            WorkflowError::ConcurrentUpdate(_) => WorkflowErrorProto::WorkflowErrorExecution, // Map to Execution for proto
        }
    }
}

impl From<WorkflowErrorProto> for WorkflowError {
    fn from(proto: WorkflowErrorProto) -> Self {
        match proto {
            WorkflowErrorProto::WorkflowErrorUnspecified => WorkflowError::Execution("Unspecified error".to_string()),
            WorkflowErrorProto::WorkflowErrorStorage => WorkflowError::Storage("Storage error".to_string()),
            WorkflowErrorProto::WorkflowErrorSerialization => WorkflowError::Serialization("Serialization error".to_string()),
            WorkflowErrorProto::WorkflowErrorNotFound => WorkflowError::NotFound("Not found".to_string()),
            WorkflowErrorProto::WorkflowErrorInvalidDefinition => WorkflowError::InvalidDefinition("Invalid definition".to_string()),
            WorkflowErrorProto::WorkflowErrorExecution => WorkflowError::Execution("Execution error".to_string()),
        }
    }
}

/// Workflow definition - template for workflow execution
///
/// ## TDD Note
/// Minimal implementation to pass `test_workflow_definition_persistence`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// Workflow ID (unique identifier)
    pub id: String,

    /// Workflow name (human-readable)
    pub name: String,

    /// Workflow version (semantic versioning)
    pub version: String,

    /// Workflow steps (ordered)
    pub steps: Vec<Step>,

    /// Optional timeout for entire workflow
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<std::time::Duration>,

    /// Optional retry policy
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,
}

impl Default for WorkflowDefinition {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            version: String::new(),
            steps: Vec::new(),
            timeout: None,
            retry_policy: None,
        }
    }
}

/// Individual workflow step
///
/// ## TDD Note
/// Minimal implementation to support `test_simple_single_step_workflow`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Step ID (unique within workflow)
    pub id: String,

    /// Step name (human-readable)
    pub name: String,

    /// Step type (task, parallel, map, choice, wait, signal)
    pub step_type: StepType,

    /// Step configuration (JSON object)
    pub config: Value,

    /// Optional next step (if not specified, uses next in array)
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,

    /// Optional error handler step
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_error: Option<String>,

    /// Optional retry policy for this step
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<RetryPolicy>,
}

impl Default for Step {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            step_type: StepType::Task,
            config: Value::Null,
            next: None,
            on_error: None,
            retry_policy: None,
        }
    }
}

/// Step types
///
/// ## Design
/// Following AWS Step Functions patterns with PlexSpaces actor integration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepType {
    /// Task: Invoke an actor method (GenServer call)
    Task,

    /// Parallel: Execute multiple branches concurrently
    Parallel,

    /// Map: Execute same step for each item in array
    Map,

    /// Choice: Conditional branching based on state
    Choice,

    /// Wait: Delay execution for duration or until timestamp
    Wait,

    /// Signal: Wait for external signal (Awakeable pattern)
    Signal,
}

/// Workflow execution - runtime instance of workflow definition
///
/// ## TDD Note
/// Minimal fields to pass `test_execution_status_transitions`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Execution ID (ULID for sortability)
    pub execution_id: String,

    /// Definition ID this execution is based on
    pub definition_id: String,

    /// Definition version this execution is based on
    pub definition_version: String,

    /// Current execution status
    pub status: ExecutionStatus,

    /// Current step ID (if running)
    pub current_step_id: Option<String>,

    /// Initial input (for display/debugging)
    pub input: Option<Value>,

    /// Final output (if completed)
    pub output: Option<Value>,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Node ID executing this workflow (node_owner)
    pub node_id: Option<String>,

    /// Version for optimistic locking
    pub version: u64,

    /// Last heartbeat timestamp (for health monitoring)
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
}

/// Execution status states
///
/// ## Design
/// State machine for workflow execution lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Execution created but not started
    Pending,

    /// Execution in progress
    Running,

    /// Execution completed successfully
    Completed,

    /// Execution failed with error
    Failed,

    /// Execution cancelled by user
    Cancelled,

    /// Execution timed out
    TimedOut,
}

impl ExecutionStatus {
    /// Parse status from string (for SQL storage)
    pub fn from_string(s: &str) -> Result<Self, WorkflowError> {
        match s.to_uppercase().as_str() {
            "PENDING" => Ok(Self::Pending),
            "RUNNING" => Ok(Self::Running),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            "CANCELLED" => Ok(Self::Cancelled),
            "TIMED_OUT" | "TIMEDOUT" => Ok(Self::TimedOut),
            _ => Err(WorkflowError::InvalidDefinition(format!(
                "Unknown status: {}",
                s
            ))),
        }
    }
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::TimedOut => "TIMED_OUT",
        };
        write!(f, "{}", s)
    }
}

/// Retry policy for steps and workflows
///
/// ## Design
/// Exponential backoff with jitter (best practice for distributed systems)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,

    /// Initial backoff duration
    pub initial_backoff: std::time::Duration,

    /// Backoff multiplier (e.g., 2.0 for exponential)
    pub backoff_multiplier: f64,

    /// Maximum backoff duration (cap for exponential growth)
    pub max_backoff: std::time::Duration,

    /// Add random jitter to prevent thundering herd (0.0-1.0)
    pub jitter: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: std::time::Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_backoff: std::time::Duration::from_secs(60),
            jitter: 0.1,
        }
    }
}

/// Step execution - runtime instance of a workflow step
///
/// ## TDD Note
/// Minimal implementation to pass step execution tests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecution {
    /// Step execution ID (ULID)
    pub step_execution_id: String,

    /// Workflow execution ID this belongs to
    pub execution_id: String,

    /// Step ID from workflow definition
    pub step_id: String,

    /// Current step status
    pub status: StepExecutionStatus,

    /// Step input (for display/debugging)
    pub input: Option<Value>,

    /// Step output (if completed)
    pub output: Option<Value>,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Retry attempt number (1 = first attempt)
    pub attempt: u32,
}

/// Step execution status states
///
/// ## Design
/// State machine for individual step lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepExecutionStatus {
    /// Step waiting to run
    Pending,

    /// Step currently executing
    Running,

    /// Step completed successfully
    Completed,

    /// Step failed with error
    Failed,

    /// Step retrying after failure
    Retrying,

    /// Step cancelled by user
    Cancelled,
}

impl StepExecutionStatus {
    /// Parse status from string (for SQL storage)
    pub fn from_string(s: &str) -> Result<Self, WorkflowError> {
        match s.to_uppercase().as_str() {
            "PENDING" => Ok(Self::Pending),
            "RUNNING" => Ok(Self::Running),
            "COMPLETED" => Ok(Self::Completed),
            "FAILED" => Ok(Self::Failed),
            "RETRYING" => Ok(Self::Retrying),
            "CANCELLED" => Ok(Self::Cancelled),
            _ => Err(WorkflowError::InvalidDefinition(format!(
                "Unknown step status: {}",
                s
            ))),
        }
    }
}

impl fmt::Display for StepExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Retrying => "RETRYING",
            Self::Cancelled => "CANCELLED",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_status_from_string() {
        assert_eq!(
            ExecutionStatus::from_string("PENDING").unwrap(),
            ExecutionStatus::Pending
        );
        assert_eq!(
            ExecutionStatus::from_string("running").unwrap(),
            ExecutionStatus::Running
        );
        assert_eq!(
            ExecutionStatus::from_string("COMPLETED").unwrap(),
            ExecutionStatus::Completed
        );
        assert!(ExecutionStatus::from_string("INVALID").is_err());
    }

    #[test]
    fn test_execution_status_to_string() {
        assert_eq!(ExecutionStatus::Pending.to_string(), "PENDING");
        assert_eq!(ExecutionStatus::Running.to_string(), "RUNNING");
        assert_eq!(ExecutionStatus::Completed.to_string(), "COMPLETED");
        assert_eq!(ExecutionStatus::Failed.to_string(), "FAILED");
    }

    #[test]
    fn test_workflow_definition_serialization() {
        let def = WorkflowDefinition {
            id: "test".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0".to_string(),
            steps: vec![],
            timeout: None,
            retry_policy: None,
        };

        let json = serde_json::to_string(&def).unwrap();
        let parsed: WorkflowDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, "test");
        assert_eq!(parsed.name, "Test Workflow");
        assert_eq!(parsed.version, "1.0");
    }

    #[test]
    fn test_step_execution_status_all_variants() {
        // Test from_string for all variants
        assert_eq!(
            StepExecutionStatus::from_string("PENDING").unwrap(),
            StepExecutionStatus::Pending
        );
        assert_eq!(
            StepExecutionStatus::from_string("RUNNING").unwrap(),
            StepExecutionStatus::Running
        );
        assert_eq!(
            StepExecutionStatus::from_string("COMPLETED").unwrap(),
            StepExecutionStatus::Completed
        );
        assert_eq!(
            StepExecutionStatus::from_string("FAILED").unwrap(),
            StepExecutionStatus::Failed
        );
        assert_eq!(
            StepExecutionStatus::from_string("RETRYING").unwrap(),
            StepExecutionStatus::Retrying
        );
        assert_eq!(
            StepExecutionStatus::from_string("CANCELLED").unwrap(),
            StepExecutionStatus::Cancelled
        );

        // Test error case
        assert!(StepExecutionStatus::from_string("INVALID").is_err());

        // Test to_string for all variants
        assert_eq!(StepExecutionStatus::Pending.to_string(), "PENDING");
        assert_eq!(StepExecutionStatus::Running.to_string(), "RUNNING");
        assert_eq!(StepExecutionStatus::Completed.to_string(), "COMPLETED");
        assert_eq!(StepExecutionStatus::Failed.to_string(), "FAILED");
        assert_eq!(StepExecutionStatus::Retrying.to_string(), "RETRYING");
        assert_eq!(StepExecutionStatus::Cancelled.to_string(), "CANCELLED");
    }

    #[test]
    fn test_retry_policy_default() {
        // Test RetryPolicy::default() to cover default implementation
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.initial_backoff, std::time::Duration::from_secs(1));
        assert_eq!(policy.backoff_multiplier, 2.0);
        assert_eq!(policy.max_backoff, std::time::Duration::from_secs(60));
        assert_eq!(policy.jitter, 0.1);
    }
}
