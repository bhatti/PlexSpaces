// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP routes for WASM application deploy and undeploy.
//!
//! Deploy stays HTTP-only because WASM binaries can exceed the 5 MB gRPC message limit.

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, post},
    Router,
};
use plexspaces_actor::{NodeConnectivity, ServiceLocator};

const MAX_WASM_BODY_SIZE: usize = 100 * 1024 * 1024; // 100MB

/// State shared across deploy HTTP handlers.
#[derive(Clone)]
pub struct DeployRouteState {
    /// Service locator for accessing node services
    pub service_locator: Arc<dyn ServiceLocator>,
    /// Node connectivity interface
    pub node_connectivity: Arc<dyn NodeConnectivity>,
    /// Whether authentication is disabled
    pub auth_disabled: bool,
    /// JWT secret for token validation
    pub jwt_secret: Option<String>,
}

/// Build the deploy/undeploy HTTP router.
pub fn deploy_router(
    service_locator: Arc<dyn ServiceLocator>,
    node_connectivity: Arc<dyn NodeConnectivity>,
    auth_disabled: bool,
    jwt_secret: Option<String>,
) -> Router {
    let state = DeployRouteState {
        service_locator,
        node_connectivity,
        auth_disabled,
        jwt_secret,
    };

    Router::new()
        .route("/api/v1/applications/deploy", post(handle_deploy))
        .route(
            "/api/v1/applications/:application_id",
            delete(handle_undeploy),
        )
        .layer(DefaultBodyLimit::max(MAX_WASM_BODY_SIZE))
        .with_state(state)
}

async fn handle_deploy(
    State(s): State<DeployRouteState>,
    headers: axum::http::HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use plexspaces_proto::application::v1::{
        application_service_server::ApplicationService, DeployApplicationRequest,
    };
    use plexspaces_proto::wasm::v1::WasmModule;
    use plexspaces_services::application_service::ApplicationServiceImpl;
    use plexspaces_services::create_default_application_spec;
    use tonic::metadata::MetadataValue;

    let mut application_id = None;
    let mut name = None;
    let mut version = None;
    let mut behavior_kind = None;
    let mut wasm_file_data: Option<Vec<u8>> = None;
    let mut config_data: Option<String> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        tracing::error!(error = %e, "Multipart parsing error");
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to parse multipart form data: {}", e),
        )
    })? {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "application_id" => {
                application_id = Some(field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read application_id: {}", e),
                    )
                })?);
            }
            "name" => {
                name = Some(field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read name: {}", e),
                    )
                })?);
            }
            "version" => {
                version = Some(field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read version: {}", e),
                    )
                })?);
            }
            "behavior_kind" => {
                behavior_kind = Some(field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read behavior_kind: {}", e),
                    )
                })?);
            }
            "wasm_file" => {
                let bytes = field.bytes().await.map_err(|e| {
                    tracing::error!(error = %e, "WASM file read error");
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read wasm_file field: {}", e),
                    )
                })?;

                if bytes.len() > MAX_WASM_BODY_SIZE {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        format!(
                            "WASM file size {} bytes exceeds maximum {} bytes",
                            bytes.len(),
                            MAX_WASM_BODY_SIZE
                        ),
                    ));
                }

                if bytes.len() < 4 {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("WASM file too small: {} bytes", bytes.len()),
                    ));
                }

                if &bytes[0..4] != b"\0asm" {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "Invalid WASM file: missing magic number (got {:02x?}, expected 0061736d)",
                            &bytes[0..4]
                        ),
                    ));
                }

                wasm_file_data = Some(bytes.to_vec());
            }
            "config" => {
                config_data = Some(field.text().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read config: {}", e),
                    )
                })?);
            }
            _ => {}
        }
    }

    let application_id = application_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "application_id is required".to_string(),
        )
    })?;
    let name = name.ok_or_else(|| (StatusCode::BAD_REQUEST, "name is required".to_string()))?;
    let version = version.unwrap_or_else(|| "1.0.0".to_string());

    let wasm_module = wasm_file_data.map(|bytes| WasmModule {
        name: name.clone(),
        version: version.clone(),
        module_bytes: bytes,
        module_hash: String::new(),
        ..Default::default()
    });

    let config = if let Some(toml_str) = config_data {
        use crate::wasm_apps_loader::parse_app_config_toml;
        match parse_app_config_toml(&toml_str, &name) {
            Ok(spec) => spec,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse TOML config, using defaults");
                create_default_application_spec(&name, &version, behavior_kind.as_deref())
            }
        }
    } else {
        create_default_application_spec(&name, &version, behavior_kind.as_deref())
    };

    let request = DeployApplicationRequest {
        application_id: application_id.clone(),
        name: name.clone(),
        version: version.clone(),
        wasm_module,
        config: Some(config),
        initial_state: vec![],
    };

    let tenant_id =
        extract_tenant_id_from_headers(&headers, s.auth_disabled, s.jwt_secret.as_deref())?;

    let app_service =
        ApplicationServiceImpl::new(s.service_locator.clone(), Some(s.node_connectivity.clone()));
    let mut grpc_request = tonic::Request::new(request);
    grpc_request.metadata_mut().insert(
        "x-tenant-id",
        MetadataValue::try_from(tenant_id.as_str())
            .unwrap_or_else(|_| MetadataValue::from_static("")),
    );
    grpc_request.metadata_mut().insert(
        "x-namespace",
        MetadataValue::try_from(application_id.as_str())
            .unwrap_or_else(|_| MetadataValue::from_static("")),
    );

    let response = app_service
        .deploy_application(grpc_request)
        .await
        .map_err(|e| {
            tracing::error!(application_id = %application_id, error = %e, "deploy_application failed");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Deployment failed: {}", e))
        })?;

    let inner = response.into_inner();
    Ok(Json(serde_json::json!({
        "success": inner.success,
        "application_id": inner.application_id,
        "status": format!("{:?}", inner.status),
        "error": inner.error
    })))
}

async fn handle_undeploy(
    State(s): State<DeployRouteState>,
    headers: axum::http::HeaderMap,
    Path(application_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use plexspaces_proto::application::v1::{
        application_service_server::ApplicationService, UndeployApplicationRequest,
    };
    use plexspaces_services::application_service::ApplicationServiceImpl;
    use tonic::metadata::MetadataValue;

    let tenant_id =
        extract_tenant_id_from_headers(&headers, s.auth_disabled, s.jwt_secret.as_deref())?;

    let app_service = ApplicationServiceImpl::new(s.service_locator.clone(), None);
    let mut grpc_request = tonic::Request::new(UndeployApplicationRequest {
        application_id: application_id.clone(),
        timeout: None,
    });
    grpc_request.metadata_mut().insert(
        "x-tenant-id",
        MetadataValue::try_from(tenant_id.as_str())
            .unwrap_or_else(|_| MetadataValue::from_static("")),
    );

    let response = app_service
        .undeploy_application(grpc_request)
        .await
        .map_err(|e| {
            if e.code() == tonic::Code::NotFound {
                tracing::info!(application_id = %application_id, "Undeploy: application not found");
                (StatusCode::NOT_FOUND, e.message().to_string())
            } else {
                tracing::error!(application_id = %application_id, error = %e, "undeploy_application failed");
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Undeployment failed: {}", e))
            }
        })?;

    let inner = response.into_inner();
    Ok(Json(serde_json::json!({
        "success": inner.success,
        "error": inner.error
    })))
}

fn extract_tenant_id_from_headers(
    headers: &axum::http::HeaderMap,
    auth_disabled: bool,
    jwt_secret: Option<&str>,
) -> Result<String, (StatusCode, String)> {
    if auth_disabled {
        return Ok(String::new());
    }
    let secret = jwt_secret.ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Auth enabled but JWT secret not configured".to_string(),
        )
    })?;
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    crate::http_jwt::validate_bearer_token(secret, auth_header.as_deref())
        .map(|claims| claims.tenant_id)
        .map_err(|e| {
            (
                StatusCode::UNAUTHORIZED,
                format!("Deploy requires valid JWT: {}", e),
            )
        })
}
