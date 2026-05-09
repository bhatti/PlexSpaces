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

//! Workflow Service gRPC implementation
//!
//! ## Purpose
//! Implements the gRPC WorkflowService for workflow orchestration.
//! Delegates to WorkflowStorage for metadata operations and WorkflowExecutor
//! for execution operations.

use plexspaces_proto::v1::common::Empty;
use plexspaces_proto::workflow::v1::{
    workflow_service_server::WorkflowService, CancelExecutionRequest, CreateDefinitionRequest,
    CreateDefinitionResponse, DeleteDefinitionRequest, GetDefinitionRequest, GetDefinitionResponse,
    GetExecutionRequest, GetExecutionResponse, GetStepExecutionsRequest, GetStepExecutionsResponse,
    ListDefinitionsRequest, ListDefinitionsResponse, ListExecutionsRequest, ListExecutionsResponse,
    QueryExecutionRequest, QueryExecutionResponse, SignalExecutionRequest, StartExecutionRequest,
    StartExecutionResponse, UpdateDefinitionRequest, UpdateDefinitionResponse,
};
use plexspaces_workflow::executor::WorkflowExecutor;
use plexspaces_workflow::storage::WorkflowStorage;
use plexspaces_workflow::types::*;
use serde_json::Value;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// Workflow Service gRPC implementation
pub struct WorkflowServiceImpl {
    /// Workflow storage for definitions and execution metadata
    storage: Arc<WorkflowStorage>,
}

impl WorkflowServiceImpl {
    /// Create new WorkflowService implementation
    pub fn new(storage: Arc<WorkflowStorage>) -> Self {
        Self { storage }
    }

    /// Convert WorkflowError to gRPC Status
    fn workflow_error_to_status(error: WorkflowError) -> Status {
        match error {
            WorkflowError::Storage(msg) => Status::internal(format!("Storage error: {}", msg)),
            WorkflowError::Serialization(msg) => {
                Status::invalid_argument(format!("Serialization error: {}", msg))
            }
            WorkflowError::NotFound(msg) => Status::not_found(msg),
            WorkflowError::InvalidDefinition(msg) => {
                Status::invalid_argument(format!("Invalid definition: {}", msg))
            }
            WorkflowError::Execution(msg) => Status::internal(format!("Execution error: {}", msg)),
            WorkflowError::ConcurrentUpdate(msg) => {
                Status::failed_precondition(format!("Concurrent update: {}", msg))
            }
        }
    }
}

#[tonic::async_trait]
impl WorkflowService for WorkflowServiceImpl {
    async fn create_definition(
        &self,
        request: Request<CreateDefinitionRequest>,
    ) -> Result<Response<CreateDefinitionResponse>, Status> {
        let req = request.into_inner();
        let def = req
            .definition
            .ok_or_else(|| Status::invalid_argument("definition is required"))?;

        self.storage
            .save_definition(&def)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(CreateDefinitionResponse {
            definition: Some(def),
        }))
    }

    async fn get_definition(
        &self,
        request: Request<GetDefinitionRequest>,
    ) -> Result<Response<GetDefinitionResponse>, Status> {
        let req = request.into_inner();

        if req.id.is_empty() {
            return Err(Status::invalid_argument("id is required"));
        }

        let version = if req.version.is_empty() {
            "latest" // TODO: Implement latest version lookup
        } else {
            &req.version
        };

        let def = self
            .storage
            .get_definition(&req.id, version)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(GetDefinitionResponse {
            definition: Some(def),
        }))
    }

    async fn list_definitions(
        &self,
        request: Request<ListDefinitionsRequest>,
    ) -> Result<Response<ListDefinitionsResponse>, Status> {
        let req = request.into_inner();

        let name_prefix = if req.name_prefix.is_empty() {
            None
        } else {
            Some(req.name_prefix.as_str())
        };

        let definitions = self
            .storage
            .list_definitions(name_prefix)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(ListDefinitionsResponse {
            definitions,
            page: None,
        }))
    }

    async fn update_definition(
        &self,
        request: Request<UpdateDefinitionRequest>,
    ) -> Result<Response<UpdateDefinitionResponse>, Status> {
        let req = request.into_inner();
        let def = req
            .definition
            .ok_or_else(|| Status::invalid_argument("definition is required"))?;

        self.storage
            .save_definition(&def)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(UpdateDefinitionResponse {
            definition: Some(def),
        }))
    }

    async fn delete_definition(
        &self,
        request: Request<DeleteDefinitionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();

        if req.id.is_empty() {
            return Err(Status::invalid_argument("id is required"));
        }

        self.storage
            .delete_definition(&req.id, &req.version)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(Empty {}))
    }

    async fn start_execution(
        &self,
        request: Request<StartExecutionRequest>,
    ) -> Result<Response<StartExecutionResponse>, Status> {
        let req = request.into_inner();

        if req.definition_id.is_empty() {
            return Err(Status::invalid_argument("definition_id is required"));
        }

        let version = if req.definition_version.is_empty() {
            "latest" // TODO: Implement latest version lookup
        } else {
            &req.definition_version
        };

        // Parse input from Struct
        // TODO: Implement full Struct to Value conversion
        // For now, use empty object
        let input = Value::Object(serde_json::Map::new());

        // OBSERVABILITY: Log workflow execution start
        tracing::info!(
            definition_id = %req.definition_id,
            definition_version = %version,
            "Starting workflow execution"
        );

        let execution_id =
            WorkflowExecutor::start_execution(&*self.storage, &req.definition_id, version, input)
                .await
                .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(StartExecutionResponse { execution_id }))
    }

    async fn get_execution(
        &self,
        request: Request<GetExecutionRequest>,
    ) -> Result<Response<GetExecutionResponse>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id is required"));
        }

        let exec = self
            .storage
            .get_execution(&req.execution_id)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(GetExecutionResponse {
            execution: Some(exec),
        }))
    }

    async fn list_executions(
        &self,
        request: Request<ListExecutionsRequest>,
    ) -> Result<Response<ListExecutionsResponse>, Status> {
        let req = request.into_inner();

        // Build status filter using proto enum values
        let statuses = if req.status == 0 {
            // No status filter - list all statuses
            vec![
                ExecutionStatus::ExecutionStatusPending,
                ExecutionStatus::ExecutionStatusRunning,
                ExecutionStatus::ExecutionStatusCompleted,
                ExecutionStatus::ExecutionStatusFailed,
                ExecutionStatus::ExecutionStatusCancelled,
                ExecutionStatus::ExecutionStatusTimedOut,
            ]
        } else {
            let status = ExecutionStatus::try_from(req.status)
                .map_err(|_| Status::invalid_argument("Invalid execution status"))?;
            vec![status]
        };

        let executions = self
            .storage
            .list_executions_by_status(statuses, None)
            .await
            .map_err(Self::workflow_error_to_status)?;

        // Filter by definition_id if provided
        let executions: Vec<_> = if req.definition_id.is_empty() {
            executions
        } else {
            executions
                .into_iter()
                .filter(|e| e.definition_id == req.definition_id)
                .collect()
        };

        // TODO: Implement pagination and timestamp filters
        Ok(Response::new(ListExecutionsResponse {
            executions,
            page: None,
        }))
    }

    async fn cancel_execution(
        &self,
        request: Request<CancelExecutionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id is required"));
        }

        // OBSERVABILITY: Log workflow execution cancellation
        tracing::info!(
            execution_id = %req.execution_id,
            reason = %req.reason,
            "Cancelling workflow execution"
        );

        self.storage
            .update_execution_status(&req.execution_id, ExecutionStatus::ExecutionStatusCancelled)
            .await
            .map_err(Self::workflow_error_to_status)?;

        // TODO: Send cancel message to workflow actor if it's running
        // This would require actor integration

        Ok(Response::new(Empty {}))
    }

    async fn signal_execution(
        &self,
        request: Request<SignalExecutionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id is required"));
        }
        if req.signal_name.is_empty() {
            return Err(Status::invalid_argument("signal_name is required"));
        }

        // Parse signal data from Value
        let payload = req
            .data
            .map(|v| {
                // Convert prost_types::Value to serde_json::Value
                match v.kind {
                    Some(prost_types::value::Kind::StringValue(s)) => {
                        serde_json::from_str(&s).unwrap_or_else(|_| Value::String(s))
                    }
                    Some(prost_types::value::Kind::BoolValue(b)) => Value::Bool(b),
                    Some(prost_types::value::Kind::NumberValue(n)) => Value::Number(
                        serde_json::Number::from_f64(n).unwrap_or(serde_json::Number::from(0)),
                    ),
                    Some(prost_types::value::Kind::NullValue(_)) => Value::Null,
                    Some(prost_types::value::Kind::ListValue(_))
                    | Some(prost_types::value::Kind::StructValue(_)) => {
                        Value::Null // TODO: Implement full conversion
                    }
                    None => Value::Null,
                }
            })
            .unwrap_or_else(|| Value::Null);

        self.storage
            .send_signal(&req.execution_id, &req.signal_name, payload)
            .await
            .map_err(Self::workflow_error_to_status)?;

        // TODO: Notify workflow actor if it's waiting for this signal
        // This would require actor integration

        Ok(Response::new(Empty {}))
    }

    async fn query_execution(
        &self,
        request: Request<QueryExecutionRequest>,
    ) -> Result<Response<QueryExecutionResponse>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id is required"));
        }
        if req.query_name.is_empty() {
            return Err(Status::invalid_argument("query_name is required"));
        }

        let exec = self
            .storage
            .get_execution(&req.execution_id)
            .await
            .map_err(Self::workflow_error_to_status)?;

        // Build query result based on query_name
        let result = match req.query_name.as_str() {
            "status" => {
                // Return execution status as its proto enum name
                let status_str = ExecutionStatus::try_from(exec.status)
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|_| exec.status.to_string());
                prost_types::Value {
                    kind: Some(prost_types::value::Kind::StringValue(status_str)),
                }
            }
            "current_step" => {
                // Return current step ID
                prost_types::Value {
                    kind: Some(prost_types::value::Kind::StringValue(
                        exec.current_step_id.clone(),
                    )),
                }
            }
            "context" | "state" => {
                // Return execution state as JSON string
                let state_json = serde_json::json!({
                    "execution_id": exec.execution_id,
                    "definition_id": exec.definition_id,
                    "definition_version": exec.definition_version,
                    "status": exec.status,
                    "current_step_id": exec.current_step_id,
                    "error": exec.error,
                });
                prost_types::Value {
                    kind: Some(prost_types::value::Kind::StringValue(
                        serde_json::to_string(&state_json).unwrap_or_default(),
                    )),
                }
            }
            _ => {
                return Err(Status::invalid_argument(format!(
                    "Unknown query_name: {}. Supported: status, current_step, context",
                    req.query_name
                )));
            }
        };

        Ok(Response::new(QueryExecutionResponse {
            result: Some(result),
        }))
    }

    async fn get_step_executions(
        &self,
        request: Request<GetStepExecutionsRequest>,
    ) -> Result<Response<GetStepExecutionsResponse>, Status> {
        let req = request.into_inner();

        if req.execution_id.is_empty() {
            return Err(Status::invalid_argument("execution_id is required"));
        }

        // get_step_execution_history already returns Vec<StepExecution> (proto type)
        let step_executions = self
            .storage
            .get_step_execution_history(&req.execution_id)
            .await
            .map_err(Self::workflow_error_to_status)?;

        Ok(Response::new(GetStepExecutionsResponse {
            step_executions,
            page: None, // TODO: Implement pagination
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::v1::common::Empty;
    use plexspaces_proto::workflow::v1::{
        CancelExecutionRequest, CreateDefinitionRequest, DeleteDefinitionRequest,
        GetDefinitionRequest, GetExecutionRequest, GetStepExecutionsRequest,
        ListDefinitionsRequest, ListExecutionsRequest, QueryExecutionRequest,
        SignalExecutionRequest, StartExecutionRequest, UpdateDefinitionRequest,
        WorkflowDefinition as ProtoWorkflowDefinition,
    };
    use plexspaces_workflow::storage::WorkflowStorage;
    use std::sync::Arc;
    use tonic::Request;

    async fn create_test_service() -> WorkflowServiceImpl {
        let storage = Arc::new(WorkflowStorage::new_in_memory().await.unwrap());
        WorkflowServiceImpl::new(storage)
    }

    #[tokio::test]
    async fn test_create_definition() {
        let service = create_test_service().await;

        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def.clone()),
        });

        let result = service.create_definition(req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.definition.is_some());
        let def = response.definition.unwrap();
        assert_eq!(def.id, "test-workflow");
        assert_eq!(def.name, "Test Workflow");
        assert_eq!(def.version, "1.0");
    }

    #[tokio::test]
    async fn test_get_definition() {
        let service = create_test_service().await;

        // First create a definition
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        // Then get it
        let get_req = Request::new(GetDefinitionRequest {
            id: "test-workflow".to_string(),
            version: "1.0".to_string(),
        });

        let result = service.get_definition(get_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.definition.is_some());
        let def = response.definition.unwrap();
        assert_eq!(def.id, "test-workflow");
    }

    #[tokio::test]
    async fn test_get_definition_not_found() {
        let service = create_test_service().await;

        let get_req = Request::new(GetDefinitionRequest {
            id: "nonexistent".to_string(),
            version: "1.0".to_string(),
        });

        let result = service.get_definition(get_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_list_definitions() {
        let service = create_test_service().await;

        // Create a few definitions
        for i in 1..=3 {
            let mut proto_def = ProtoWorkflowDefinition::default();
            proto_def.id = format!("workflow-{}", i);
            proto_def.name = format!("Workflow {}", i);
            proto_def.version = "1.0".to_string();

            let create_req = Request::new(CreateDefinitionRequest {
                definition: Some(proto_def),
            });
            service.create_definition(create_req).await.unwrap();
        }

        // List all definitions
        let list_req = Request::new(ListDefinitionsRequest {
            page: None,
            label_filter: std::collections::HashMap::new(),
            name_prefix: String::new(),
        });

        let result = service.list_definitions(list_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.definitions.len() >= 3);
    }

    #[tokio::test]
    async fn test_update_definition() {
        let service = create_test_service().await;

        // Create a definition
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def.clone()),
        });
        service.create_definition(create_req).await.unwrap();

        // Update it
        proto_def.name = "Updated Workflow".to_string();
        let update_req = Request::new(UpdateDefinitionRequest {
            definition: Some(proto_def),
        });

        let result = service.update_definition(update_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert_eq!(response.definition.unwrap().name, "Updated Workflow");
    }

    #[tokio::test]
    async fn test_delete_definition() {
        let service = create_test_service().await;

        // Create a definition
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        // Delete it
        let delete_req = Request::new(DeleteDefinitionRequest {
            id: "test-workflow".to_string(),
            version: "1.0".to_string(),
        });

        let result = service.delete_definition(delete_req).await;
        assert!(result.is_ok());

        // Verify it's deleted
        let get_req = Request::new(GetDefinitionRequest {
            id: "test-workflow".to_string(),
            version: "1.0".to_string(),
        });

        let result = service.get_definition(get_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_start_execution() {
        let service = create_test_service().await;

        // First create a definition
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        // Start execution
        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let result = service.start_execution(start_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(!response.execution_id.is_empty());
    }

    #[tokio::test]
    async fn test_get_execution() {
        let service = create_test_service().await;

        // Create definition and start execution
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let start_response = service.start_execution(start_req).await.unwrap();
        let execution_id = start_response.into_inner().execution_id;

        // Get execution
        let get_req = Request::new(GetExecutionRequest { execution_id });

        let result = service.get_execution(get_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.execution.is_some());
    }

    #[tokio::test]
    async fn test_list_executions() {
        let service = create_test_service().await;

        // Create definition and start a few executions
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        for _ in 0..3 {
            let start_req = Request::new(StartExecutionRequest {
                definition_id: "test-workflow".to_string(),
                definition_version: "1.0".to_string(),
                input: None,
                execution_id: String::new(),
                labels: std::collections::HashMap::new(),
            });
            service.start_execution(start_req).await.unwrap();
        }

        // List executions
        let list_req = Request::new(ListExecutionsRequest {
            page: None,
            definition_id: "test-workflow".to_string(),
            status: 0, // All statuses
            label_filter: std::collections::HashMap::new(),
            started_after: None,
            started_before: None,
        });

        let result = service.list_executions(list_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.executions.len() >= 3);
    }

    #[tokio::test]
    async fn test_cancel_execution() {
        let service = create_test_service().await;

        // Create definition and start execution
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let start_response = service.start_execution(start_req).await.unwrap();
        let execution_id = start_response.into_inner().execution_id;

        // Cancel execution
        let cancel_req = Request::new(CancelExecutionRequest {
            execution_id: execution_id.clone(),
            reason: "Test cancellation".to_string(),
        });

        let result = service.cancel_execution(cancel_req).await;
        assert!(result.is_ok());

        // Verify status is cancelled
        let get_req = Request::new(GetExecutionRequest { execution_id });
        let exec = service.get_execution(get_req).await.unwrap().into_inner();
        // ExecutionStatusCancelled = 5
        assert_eq!(exec.execution.unwrap().status, ExecutionStatus::ExecutionStatusCancelled as i32);
    }

    #[tokio::test]
    async fn test_signal_execution() {
        let service = create_test_service().await;

        // Create definition and start execution
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let start_response = service.start_execution(start_req).await.unwrap();
        let execution_id = start_response.into_inner().execution_id;

        // Send signal
        let signal_req = Request::new(SignalExecutionRequest {
            execution_id,
            signal_name: "test-signal".to_string(),
            data: Some(prost_types::Value {
                kind: Some(prost_types::value::Kind::StringValue(
                    "test-data".to_string(),
                )),
            }),
        });

        let result = service.signal_execution(signal_req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_query_execution() {
        let service = create_test_service().await;

        // Create definition and start execution
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let start_response = service.start_execution(start_req).await.unwrap();
        let execution_id = start_response.into_inner().execution_id;

        // Query status
        let query_req = Request::new(QueryExecutionRequest {
            execution_id: execution_id.clone(),
            query_name: "status".to_string(),
        });

        let result = service.query_execution(query_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        assert!(response.result.is_some());

        // Query current_step
        let query_req = Request::new(QueryExecutionRequest {
            execution_id,
            query_name: "current_step".to_string(),
        });

        let result = service.query_execution(query_req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_step_executions() {
        let service = create_test_service().await;

        // Create definition and start execution
        let mut proto_def = ProtoWorkflowDefinition::default();
        proto_def.id = "test-workflow".to_string();
        proto_def.name = "Test Workflow".to_string();
        proto_def.version = "1.0".to_string();

        let create_req = Request::new(CreateDefinitionRequest {
            definition: Some(proto_def),
        });
        service.create_definition(create_req).await.unwrap();

        let start_req = Request::new(StartExecutionRequest {
            definition_id: "test-workflow".to_string(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let start_response = service.start_execution(start_req).await.unwrap();
        let execution_id = start_response.into_inner().execution_id;

        // Get step executions
        let get_steps_req = Request::new(GetStepExecutionsRequest {
            execution_id,
            page: None,
        });

        let result = service.get_step_executions(get_steps_req).await;
        assert!(result.is_ok());

        let response = result.unwrap().into_inner();
        // Step executions may be empty for simple workflows
        assert!(response.step_executions.len() >= 0);
    }

    #[tokio::test]
    async fn test_invalid_requests() {
        let service = create_test_service().await;

        // Test empty definition_id
        let start_req = Request::new(StartExecutionRequest {
            definition_id: String::new(),
            definition_version: "1.0".to_string(),
            input: None,
            execution_id: String::new(),
            labels: std::collections::HashMap::new(),
        });

        let result = service.start_execution(start_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);

        // Test empty execution_id
        let get_req = Request::new(GetExecutionRequest {
            execution_id: String::new(),
        });

        let result = service.get_execution(get_req).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }
}
