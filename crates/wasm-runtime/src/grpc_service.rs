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

//! gRPC Service Implementation for WASM Runtime
//!
//! ## Purpose
//! Implements the WasmRuntimeService gRPC interface defined in wasm.proto,
//! wrapping the WasmDeploymentService for network-based deployment.
//!
//! ## Architecture
//! ```text
//! gRPC Client → WasmRuntimeServiceImpl → WasmDeploymentService → WasmRuntime
//! ```
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_wasm_runtime::grpc_service::WasmRuntimeServiceImpl;
//! use plexspaces_wasm_runtime::WasmRuntime;
//! use plexspaces_proto::v1::wasm_runtime_service_server::WasmRuntimeServiceServer;
//! use std::sync::Arc;
//! use tonic::transport::Server;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let runtime = WasmRuntime::new().await?;
//! let service = WasmRuntimeServiceImpl::new(Arc::new(runtime));
//!
//! // Start gRPC server
//! Server::builder()
//!     .add_service(WasmRuntimeServiceServer::new(service))
//!     .serve("0.0.0.0:9001".parse()?)
//!     .await?;
//! # Ok(())
//! # }
//! ```

use crate::deployment_service::WasmDeploymentService;
use crate::{WasmConfig, WasmError, WasmRuntime};
use plexspaces_proto::wasm::v1::{
    wasm_runtime_service_server::WasmRuntimeService, DeployWasmModuleRequest,
    DeployWasmModuleResponse, InstantiateActorRequest, InstantiateActorResponse,
    MigrateActorRequest, MigrateActorResponse, WasmError as ProtoWasmError, WasmErrorCode,
};
use plexspaces_proto::prost_types;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// gRPC service implementation for WASM Runtime
///
/// Wraps WasmDeploymentService to provide network-based deployment via gRPC.
/// Implements the `WasmRuntimeService` trait generated from wasm.proto.
#[derive(Debug, Clone)]
pub struct WasmRuntimeServiceImpl {
    /// Deployment service handling actual WASM operations
    deployment_service: Arc<WasmDeploymentService>,
}

impl WasmRuntimeServiceImpl {
    /// Create new gRPC service
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        let deployment_service = Arc::new(WasmDeploymentService::new(runtime));
        Self {
            deployment_service,
        }
    }

    /// Convert internal WasmError to proto WasmError
    fn to_proto_error(error: WasmError) -> ProtoWasmError {
        let (code, message, expected_hash, actual_hash) = match &error {
            WasmError::CompilationError(msg) => (
                WasmErrorCode::WasmErrorCodeCompilationFailed,
                msg.clone(),
                None,
                None,
            ),
            WasmError::InstantiationError(msg) => (
                WasmErrorCode::WasmErrorCodeInstantiationFailed,
                msg.clone(),
                None,
                None,
            ),
            WasmError::ModuleNotFound(msg) => (
                WasmErrorCode::WasmErrorCodeModuleNotFound,
                msg.clone(),
                None,
                None,
            ),
            WasmError::HashMismatch { expected, actual } => (
                WasmErrorCode::WasmErrorCodeHashMismatch,
                format!("Hash mismatch: expected {}, got {}", expected, actual),
                Some(expected.clone()),
                Some(actual.clone()),
            ),
            WasmError::ActorFunctionError(msg) => (
                WasmErrorCode::WasmErrorCodeActorFunctionError,
                msg.clone(),
                None,
                None,
            ),
            WasmError::HostFunctionError(msg) => (
                WasmErrorCode::WasmErrorCodeHostFunctionError,
                msg.clone(),
                None,
                None,
            ),
            WasmError::ResourceLimitExceeded(msg) => (
                WasmErrorCode::WasmErrorCodeResourceLimitExceeded,
                msg.clone(),
                None,
                None,
            ),
            WasmError::CapabilityDenied(msg) => (
                WasmErrorCode::WasmErrorCodeCapabilityDenied,
                msg.clone(),
                None,
                None,
            ),
            WasmError::CacheError(msg) => (
                WasmErrorCode::WasmErrorCodeCacheError,
                msg.clone(),
                None,
                None,
            ),
            WasmError::SerializationError(msg) => (
                WasmErrorCode::WasmErrorCodeSerializationError,
                msg.clone(),
                None,
                None,
            ),
            WasmError::WasmtimeError(msg) => (
                WasmErrorCode::WasmErrorCodeWasmtimeError,
                msg.to_string(),
                None,
                None,
            ),
            WasmError::PoolExhausted(msg) => (
                WasmErrorCode::WasmErrorCodeCacheError,
                msg.clone(),
                None,
                None,
            ),
            WasmError::ConfigurationError(msg) => (
                WasmErrorCode::WasmErrorCodeWasmtimeError,
                msg.clone(),
                None,
                None,
            ),
        };

        ProtoWasmError {
            code: code as i32,
            message,
            details: HashMap::new(),
            expected_hash: expected_hash.unwrap_or_default(),
            actual_hash: actual_hash.unwrap_or_default(),
        }
    }
}

#[tonic::async_trait]
impl WasmRuntimeService for WasmRuntimeServiceImpl {
    async fn deploy_module(
        &self,
        request: Request<DeployWasmModuleRequest>,
    ) -> Result<Response<DeployWasmModuleResponse>, Status> {
        let req = request.into_inner();

        let module = req.module.ok_or_else(|| {
            Status::invalid_argument("module field is required")
        })?;

        if module.name.is_empty() {
            return Err(Status::invalid_argument("module.name cannot be empty"));
        }
        if module.version.is_empty() {
            return Err(Status::invalid_argument("module.version cannot be empty"));
        }
        if module.module_bytes.is_empty() {
            return Err(Status::invalid_argument("module.module_bytes cannot be empty"));
        }

        match self
            .deployment_service
            .deploy_module(&module.name, &module.version, &module.module_bytes)
            .await
        {
            Ok(module_hash) => {
                tracing::info!(
                    module_name = %module.name,
                    module_version = %module.version,
                    module_hash = %module_hash,
                    "Module deployed via gRPC"
                );

                let response = DeployWasmModuleResponse {
                    success: true,
                    module_hash,
                    nodes_pre_warmed: 1,
                    error: None,
                };

                Ok(Response::new(response))
            }
            Err(err) => {
                let response = DeployWasmModuleResponse {
                    success: false,
                    module_hash: String::new(),
                    nodes_pre_warmed: 0,
                    error: Some(Self::to_proto_error(err)),
                };

                Ok(Response::new(response))
            }
        }
    }

    async fn instantiate_actor(
        &self,
        request: Request<InstantiateActorRequest>,
    ) -> Result<Response<InstantiateActorResponse>, Status> {
        let req = request.into_inner();

        if req.module_ref.is_empty() {
            return Err(Status::invalid_argument("module_ref cannot be empty"));
        }
        if req.actor_id.is_empty() {
            return Err(Status::invalid_argument("actor_id cannot be empty"));
        }

        let config = req.config.map(|_| WasmConfig::default());

        match self
            .deployment_service
            .instantiate_actor(&req.module_ref, &req.actor_id, &req.initial_state, config)
            .await
        {
            Ok(actor_id) => {
                tracing::info!(
                    module_ref = %req.module_ref,
                    actor_id = %actor_id,
                    "Actor instantiated via gRPC"
                );

                let response = InstantiateActorResponse {
                    success: true,
                    actor_id,
                    node_id: "local".to_string(),
                    created_at: Some(prost_types::Timestamp {
                        seconds: chrono::Utc::now().timestamp(),
                        nanos: 0,
                    }),
                    error: None,
                };

                Ok(Response::new(response))
            }
            Err(err) => {
                let response = InstantiateActorResponse {
                    success: false,
                    actor_id: req.actor_id,
                    node_id: String::new(),
                    created_at: None,
                    error: Some(Self::to_proto_error(err)),
                };

                Ok(Response::new(response))
            }
        }
    }

    async fn migrate_actor(
        &self,
        request: Request<MigrateActorRequest>,
    ) -> Result<Response<MigrateActorResponse>, Status> {
        let req = request.into_inner();

        tracing::warn!(
            actor_id = %req.actor_id,
            source = %req.source_node_id,
            target = %req.target_node_id,
            "Migration requested but not implemented"
        );

        Err(Status::unimplemented(
            "Actor migration not implemented - Phase 8",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::wasm::v1::WasmModule as ProtoWasmModule;

    const SIMPLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
        0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00,
        0x00, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
    ];

    #[tokio::test]
    async fn test_deploy_module_via_grpc() {
        let runtime = WasmRuntime::new().await.unwrap();
        let service = WasmRuntimeServiceImpl::new(Arc::new(runtime));

        let module = ProtoWasmModule {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            module_bytes: SIMPLE_WASM.to_vec(),
            module_hash: String::new(),
            wit_interface: String::new(),
            source_languages: vec![],
            metadata: None,
            created_at: None,
            size_bytes: SIMPLE_WASM.len() as u64,
            version_number: 1,
        };

        let req = Request::new(DeployWasmModuleRequest {
            module: Some(module),
            pre_warm: 0,
            target_node_tags: vec![],
        });

        let resp = service.deploy_module(req).await.unwrap().into_inner();
        assert!(resp.success);
        assert_eq!(resp.module_hash.len(), 64);
    }
}
