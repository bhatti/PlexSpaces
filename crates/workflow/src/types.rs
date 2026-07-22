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

//! Workflow type definitions
//!
//! ## Purpose
//! Core types for workflow orchestration following proto-first design.
//! All data models are defined in `workflow.proto` — this module re-exports
//! the proto-generated types as the canonical types and adds extension traits
//! for SQL string serialization.
//!
//! ## Proto-First Design
//! See `proto/plexspaces/v1/workflow.proto` for the source of truth.

// Re-export proto-generated types as the canonical types
pub use plexspaces_proto::workflow::v1::{
    ExecutionStatus, RetryConfig, Step, StepExecution, StepStatus, StepType, WorkflowDefinition,
    WorkflowExecution,
};

// Re-export proto-generated error enum
pub use plexspaces_proto::workflow::v1::WorkflowError as WorkflowErrorProto;

/// Extension trait for WorkflowExecution — ergonomic status access
pub trait WorkflowExecutionExt {
    /// Get the execution status as the proto enum variant
    fn execution_status(&self) -> ExecutionStatus;
}

impl WorkflowExecutionExt for WorkflowExecution {
    fn execution_status(&self) -> ExecutionStatus {
        ExecutionStatus::try_from(self.status)
            .unwrap_or(ExecutionStatus::ExecutionStatusUnspecified)
    }
}

/// Extension trait for StepExecution — ergonomic status access
pub trait StepExecutionExt {
    /// Get the step status as the proto enum variant
    fn step_status(&self) -> StepStatus;
    /// Get output as serde_json Value
    fn output_value(&self) -> Option<serde_json::Value>;
    /// Get input as serde_json Value
    fn input_value(&self) -> Option<serde_json::Value>;
}

impl StepExecutionExt for StepExecution {
    fn step_status(&self) -> StepStatus {
        StepStatus::try_from(self.status).unwrap_or(StepStatus::StepStatusUnspecified)
    }

    fn output_value(&self) -> Option<serde_json::Value> {
        self.output.as_ref().map(prost_struct_to_value)
    }

    fn input_value(&self) -> Option<serde_json::Value> {
        self.input.as_ref().map(prost_struct_to_value)
    }
}

/// Extension trait for WorkflowExecution — ergonomic output access
pub trait WorkflowExecutionOutputExt {
    /// Get output as serde_json Value
    fn output_value(&self) -> Option<serde_json::Value>;
}

impl WorkflowExecutionOutputExt for WorkflowExecution {
    fn output_value(&self) -> Option<serde_json::Value> {
        self.output.as_ref().map(prost_struct_to_value)
    }
}

/// Convert a serde_json::Value to prost_types::Struct.
/// Returns None if the value is not an object (or on any error).
pub fn json_value_to_prost_struct(v: &serde_json::Value) -> Option<prost_types::Struct> {
    match v {
        serde_json::Value::Object(map) => {
            let fields: prost::alloc::collections::BTreeMap<String, prost_types::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), json_value_to_prost_value(v)))
                .collect();
            Some(prost_types::Struct { fields })
        }
        // For non-object values, wrap in a sentinel struct so we can round-trip
        _ => {
            let mut fields: prost::alloc::collections::BTreeMap<String, prost_types::Value> =
                prost::alloc::collections::BTreeMap::new();
            fields.insert("__value__".to_string(), json_value_to_prost_value(v));
            Some(prost_types::Struct { fields })
        }
    }
}

fn json_value_to_prost_value(v: &serde_json::Value) -> prost_types::Value {
    let kind = match v {
        serde_json::Value::Null => prost_types::value::Kind::NullValue(0),
        serde_json::Value::Bool(b) => prost_types::value::Kind::BoolValue(*b),
        serde_json::Value::Number(n) => {
            prost_types::value::Kind::NumberValue(n.as_f64().unwrap_or(0.0))
        }
        serde_json::Value::String(s) => prost_types::value::Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => {
            prost_types::value::Kind::ListValue(prost_types::ListValue {
                values: arr.iter().map(json_value_to_prost_value).collect(),
            })
        }
        serde_json::Value::Object(_) => {
            if let Some(s) = json_value_to_prost_struct(v) {
                prost_types::value::Kind::StructValue(s)
            } else {
                prost_types::value::Kind::NullValue(0)
            }
        }
    };
    prost_types::Value { kind: Some(kind) }
}

/// Convert a prost_types::Struct to serde_json::Value
/// Detects the "__value__" sentinel wrapper used for non-object values.
fn prost_struct_to_value(s: &prost_types::Struct) -> serde_json::Value {
    // Detect sentinel wrapper for non-object values (arrays, scalars)
    if s.fields.len() == 1 {
        if let Some(inner) = s.fields.get("__value__") {
            return prost_value_to_json(inner);
        }
    }
    let mut map = serde_json::Map::new();
    for (k, v) in &s.fields {
        map.insert(k.clone(), prost_value_to_json(v));
    }
    serde_json::Value::Object(map)
}

fn prost_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    match &v.kind {
        None => serde_json::Value::Null,
        Some(prost_types::value::Kind::NullValue(_)) => serde_json::Value::Null,
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(prost_types::value::Kind::ListValue(list)) => {
            serde_json::Value::Array(list.values.iter().map(prost_value_to_json).collect())
        }
        Some(prost_types::value::Kind::StructValue(s)) => prost_struct_to_value(s),
    }
}

/// Workflow error type (wraps proto enum for thiserror compatibility)
///
/// ## Proto-First Design
/// The proto enum (`WorkflowErrorProto`) is the wire contract. This wrapper adds:
/// - String payloads (proto enum values carry no message)
/// - `thiserror::Error` implementation for `?` ergonomics
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

impl WorkflowError {
    /// Return the proto error code for this error.
    pub fn code(&self) -> WorkflowErrorProto {
        match self {
            WorkflowError::Storage(_) => WorkflowErrorProto::WorkflowErrorStorage,
            WorkflowError::Serialization(_) => WorkflowErrorProto::WorkflowErrorSerialization,
            WorkflowError::NotFound(_) => WorkflowErrorProto::WorkflowErrorNotFound,
            WorkflowError::InvalidDefinition(_) => {
                WorkflowErrorProto::WorkflowErrorInvalidDefinition
            }
            WorkflowError::Execution(_) => WorkflowErrorProto::WorkflowErrorExecution,
            WorkflowError::ConcurrentUpdate(_) => WorkflowErrorProto::WorkflowErrorConcurrentUpdate,
        }
    }
}

impl From<WorkflowError> for WorkflowErrorProto {
    fn from(err: WorkflowError) -> Self {
        err.code()
    }
}

impl From<WorkflowErrorProto> for WorkflowError {
    fn from(proto: WorkflowErrorProto) -> Self {
        match proto {
            WorkflowErrorProto::WorkflowErrorUnspecified => {
                WorkflowError::Execution("Unspecified error".to_string())
            }
            WorkflowErrorProto::WorkflowErrorStorage => {
                WorkflowError::Storage("Storage error".to_string())
            }
            WorkflowErrorProto::WorkflowErrorSerialization => {
                WorkflowError::Serialization("Serialization error".to_string())
            }
            WorkflowErrorProto::WorkflowErrorNotFound => {
                WorkflowError::NotFound("Not found".to_string())
            }
            WorkflowErrorProto::WorkflowErrorInvalidDefinition => {
                WorkflowError::InvalidDefinition("Invalid definition".to_string())
            }
            WorkflowErrorProto::WorkflowErrorExecution => {
                WorkflowError::Execution("Execution error".to_string())
            }
            WorkflowErrorProto::WorkflowErrorConcurrentUpdate => {
                WorkflowError::ConcurrentUpdate("Concurrent update".to_string())
            }
        }
    }
}

/// Extension trait for ExecutionStatus — needed by SQL/DDB storage for string serialization
pub trait ExecutionStatusExt {
    /// Serialize to SQL string representation
    fn as_sql_str(&self) -> &'static str;
    /// Parse from SQL string representation
    fn from_sql_str(s: &str) -> Result<ExecutionStatus, WorkflowError>;
}

impl ExecutionStatusExt for ExecutionStatus {
    fn as_sql_str(&self) -> &'static str {
        match self {
            ExecutionStatus::ExecutionStatusPending => "PENDING",
            ExecutionStatus::ExecutionStatusRunning => "RUNNING",
            ExecutionStatus::ExecutionStatusCompleted => "COMPLETED",
            ExecutionStatus::ExecutionStatusFailed => "FAILED",
            ExecutionStatus::ExecutionStatusCancelled => "CANCELLED",
            ExecutionStatus::ExecutionStatusTimedOut => "TIMED_OUT",
            ExecutionStatus::ExecutionStatusUnspecified => "UNSPECIFIED",
        }
    }

    fn from_sql_str(s: &str) -> Result<ExecutionStatus, WorkflowError> {
        match s.to_uppercase().as_str() {
            "PENDING" => Ok(ExecutionStatus::ExecutionStatusPending),
            "RUNNING" => Ok(ExecutionStatus::ExecutionStatusRunning),
            "COMPLETED" => Ok(ExecutionStatus::ExecutionStatusCompleted),
            "FAILED" => Ok(ExecutionStatus::ExecutionStatusFailed),
            "CANCELLED" => Ok(ExecutionStatus::ExecutionStatusCancelled),
            "TIMED_OUT" | "TIMEDOUT" => Ok(ExecutionStatus::ExecutionStatusTimedOut),
            _ => Err(WorkflowError::InvalidDefinition(format!(
                "Unknown status: {}",
                s
            ))),
        }
    }
}

/// Extension trait for StepStatus — needed by SQL/DDB storage for string serialization
pub trait StepStatusExt {
    /// Serialize to SQL string representation
    fn as_sql_str(&self) -> &'static str;
    /// Parse from SQL string representation
    fn from_sql_str(s: &str) -> Result<StepStatus, WorkflowError>;
}

impl StepStatusExt for StepStatus {
    fn as_sql_str(&self) -> &'static str {
        match self {
            StepStatus::StepStatusPending => "PENDING",
            StepStatus::StepStatusRunning => "RUNNING",
            StepStatus::StepStatusCompleted => "COMPLETED",
            StepStatus::StepStatusFailed => "FAILED",
            StepStatus::StepStatusRetrying => "RETRYING",
            StepStatus::StepStatusCancelled => "CANCELLED",
            StepStatus::StepStatusUnspecified => "UNSPECIFIED",
        }
    }

    fn from_sql_str(s: &str) -> Result<StepStatus, WorkflowError> {
        match s.to_uppercase().as_str() {
            "PENDING" => Ok(StepStatus::StepStatusPending),
            "RUNNING" => Ok(StepStatus::StepStatusRunning),
            "COMPLETED" => Ok(StepStatus::StepStatusCompleted),
            "FAILED" => Ok(StepStatus::StepStatusFailed),
            "RETRYING" => Ok(StepStatus::StepStatusRetrying),
            "CANCELLED" => Ok(StepStatus::StepStatusCancelled),
            _ => Err(WorkflowError::InvalidDefinition(format!(
                "Unknown step status: {}",
                s
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_status_from_sql_str() {
        assert_eq!(
            ExecutionStatus::from_sql_str("PENDING").unwrap(),
            ExecutionStatus::ExecutionStatusPending
        );
        assert_eq!(
            ExecutionStatus::from_sql_str("running").unwrap(),
            ExecutionStatus::ExecutionStatusRunning
        );
        assert_eq!(
            ExecutionStatus::from_sql_str("COMPLETED").unwrap(),
            ExecutionStatus::ExecutionStatusCompleted
        );
        assert!(ExecutionStatus::from_sql_str("INVALID").is_err());
    }

    #[test]
    fn test_execution_status_as_sql_str() {
        assert_eq!(
            ExecutionStatus::ExecutionStatusPending.as_sql_str(),
            "PENDING"
        );
        assert_eq!(
            ExecutionStatus::ExecutionStatusRunning.as_sql_str(),
            "RUNNING"
        );
        assert_eq!(
            ExecutionStatus::ExecutionStatusCompleted.as_sql_str(),
            "COMPLETED"
        );
        assert_eq!(
            ExecutionStatus::ExecutionStatusFailed.as_sql_str(),
            "FAILED"
        );
    }

    #[test]
    fn test_step_status_all_variants() {
        assert_eq!(
            StepStatus::from_sql_str("PENDING").unwrap(),
            StepStatus::StepStatusPending
        );
        assert_eq!(
            StepStatus::from_sql_str("RUNNING").unwrap(),
            StepStatus::StepStatusRunning
        );
        assert_eq!(
            StepStatus::from_sql_str("COMPLETED").unwrap(),
            StepStatus::StepStatusCompleted
        );
        assert_eq!(
            StepStatus::from_sql_str("FAILED").unwrap(),
            StepStatus::StepStatusFailed
        );
        assert_eq!(
            StepStatus::from_sql_str("RETRYING").unwrap(),
            StepStatus::StepStatusRetrying
        );
        assert_eq!(
            StepStatus::from_sql_str("CANCELLED").unwrap(),
            StepStatus::StepStatusCancelled
        );
        assert!(StepStatus::from_sql_str("INVALID").is_err());

        assert_eq!(StepStatus::StepStatusPending.as_sql_str(), "PENDING");
        assert_eq!(StepStatus::StepStatusRunning.as_sql_str(), "RUNNING");
        assert_eq!(StepStatus::StepStatusCompleted.as_sql_str(), "COMPLETED");
        assert_eq!(StepStatus::StepStatusFailed.as_sql_str(), "FAILED");
        assert_eq!(StepStatus::StepStatusRetrying.as_sql_str(), "RETRYING");
        assert_eq!(StepStatus::StepStatusCancelled.as_sql_str(), "CANCELLED");
    }
}
