// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP routes for WASM application deploy and undeploy.
//!
//! Deploy accepts a single `app_file` zip (WAR-style):
//!   app.zip
//!   ├── app.wasm          (required)
//!   ├── app-config.toml   (optional)
//!   └── static/           (optional — served at /apps/<app_id>/ after deploy)
//!
//! The zip is extracted in memory. Static files are written to
//! `{wasm_apps_dir}/{app_id}/static/` on disk so they survive restarts,
//! and the path is registered in the shared StaticRegistry under `app_id`
//! for immediate serving at `/apps/{app_id}/`.
//! Undeploy removes the entry from the registry and deletes the static dir.

use std::io::Read;
use std::path::{Component, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, post},
    Router,
};
use plexspaces_actor::{NodeConnectivity, ServiceLocator};
use plexspaces_services::user_service::TenantRepository;

use crate::http_routes::StaticRegistry;

/// Maximum compressed zip size accepted from clients.
const MAX_APP_BODY_SIZE: usize = 200 * 1024 * 1024;
/// Maximum total decompressed bytes across all static entries (zip-bomb guard).
const MAX_DECOMPRESSED_STATIC_BYTES: usize = 500 * 1024 * 1024;

/// State shared across deploy HTTP handlers.
#[derive(Clone)]
pub struct DeployRouteState {
    /// Service locator for accessing node-wide services.
    pub service_locator: Arc<dyn ServiceLocator>,
    /// Node connectivity for inter-node communication.
    pub node_connectivity: Arc<dyn NodeConnectivity>,
    /// When true, authentication checks are skipped.
    pub auth_disabled: bool,
    /// JWT key pair for verifying bearer tokens. None when auth is disabled.
    pub jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
    /// Tenant repository for resolving tenant context from JWT claims. None when auth is disabled.
    pub tenant_repo: Option<Arc<dyn TenantRepository>>,
    /// Shared static file registry — populated on deploy, cleared on undeploy.
    pub static_registry: StaticRegistry,
}

/// Build the deploy/undeploy HTTP router.
pub fn deploy_router(
    service_locator: Arc<dyn ServiceLocator>,
    node_connectivity: Arc<dyn NodeConnectivity>,
    auth_disabled: bool,
    jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
    tenant_repo: Option<Arc<dyn TenantRepository>>,
    static_registry: StaticRegistry,
) -> Router {
    let state = DeployRouteState {
        service_locator,
        node_connectivity,
        auth_disabled,
        jwt_key_pair,
        tenant_repo,
        static_registry,
    };

    Router::new()
        .route("/api/v1/applications/deploy", post(handle_deploy))
        .route(
            "/api/v1/applications/:application_id",
            delete(handle_undeploy),
        )
        .layer(DefaultBodyLimit::max(MAX_APP_BODY_SIZE))
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

    let mut application_id: Option<String> = None;
    let mut name: Option<String> = None;
    let mut version: Option<String> = None;
    let mut behavior_kind: Option<String> = None;
    let mut app_zip_data: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to parse multipart: {}", e),
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
            "app_file" => {
                let bytes = field.bytes().await.map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Failed to read app_file: {}", e),
                    )
                })?;
                if bytes.len() > MAX_APP_BODY_SIZE {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        format!(
                            "app_file {} bytes exceeds maximum {} bytes",
                            bytes.len(),
                            MAX_APP_BODY_SIZE
                        ),
                    ));
                }
                app_zip_data = Some(bytes.to_vec());
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

    // Validate application_id to prevent path traversal through PathBuf::join.
    // Only alphanumerics, hyphens, underscores, and dots are allowed.
    validate_app_id(&application_id).map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let name = name.unwrap_or_else(|| application_id.clone());
    let version = version.unwrap_or_else(|| "1.0.0".to_string());

    let zip_bytes = app_zip_data.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "app_file (zip) is required".to_string(),
        )
    })?;

    // Extract zip in memory
    let (wasm_bytes, config_str, static_files) = extract_app_zip(&zip_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid app zip: {}", e)))?;

    let wasm_bytes = wasm_bytes.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "app.zip must contain a .wasm file at the top level".to_string(),
        )
    })?;

    // Validate WASM magic
    if wasm_bytes.len() < 4 || &wasm_bytes[0..4] != b"\0asm" {
        return Err((
            StatusCode::BAD_REQUEST,
            "app.wasm has invalid WASM magic number".to_string(),
        ));
    }

    // Parse config — static_mount from uploaded TOML is intentionally ignored;
    // the serve URL is always /apps/{application_id}/ to prevent mount hijacking.
    let config = if let Some(ref toml_str) = config_str {
        use crate::wasm_apps_loader::parse_app_config_toml;
        match parse_app_config_toml(toml_str, &name) {
            Ok(spec) => spec,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse app-config.toml, using defaults");
                create_default_application_spec(&name, &version, behavior_kind.as_deref())
            }
        }
    } else {
        create_default_application_spec(&name, &version, behavior_kind.as_deref())
    };

    // Persist static files and register in the registry.
    // Registry key = application_id (the URL segment under /apps/).
    // This must happen before the gRPC call so actors can serve files immediately,
    // but we clean up on gRPC failure.
    let mut registered_static = false;
    if !static_files.is_empty() {
        let static_dir = persist_static_files(&application_id, &static_files)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to save static files: {}", e),
                )
            })?;

        s.static_registry
            .write()
            .await
            .insert(application_id.clone(), static_dir.clone());
        registered_static = true;
        tracing::info!(
            application_id = %application_id,
            url = %format!("/apps/{}/", application_id),
            dir = %static_dir.display(),
            "Static files registered"
        );
    }

    let wasm_module = WasmModule {
        name: name.clone(),
        version: version.clone(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: application_id.clone(),
        name: name.clone(),
        version: version.clone(),
        wasm_module: Some(wasm_module),
        config: Some(config),
        initial_state: vec![],
    };

    let tenant_id = crate::http_jwt::extract_tenant_id_from_headers(
        &headers,
        s.auth_disabled,
        s.jwt_key_pair.as_deref(),
    )
    .map_err(|e| {
        // On auth failure, clean up static files we already registered
        if registered_static {
            let registry = s.static_registry.clone();
            let app_id = application_id.clone();
            tokio::spawn(async move {
                remove_static_files(&app_id, &registry).await;
            });
        }
        e
    })?;

    if let Some(ref tenant_repo) = s.tenant_repo {
        match tenant_repo
            .get_or_create_by_slug(&tenant_id, &tenant_id)
            .await
        {
            Ok(_) => tracing::info!(tenant_id = %tenant_id, "Tenant ensured on deploy"),
            Err(e) => tracing::warn!(tenant_id = %tenant_id, error = %e, "Failed to ensure tenant"),
        }
    }

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
            // Clean up static files on gRPC failure so we don't leave orphaned state.
            if registered_static {
                let registry = s.static_registry.clone();
                let app_id = application_id.clone();
                tokio::spawn(async move {
                    remove_static_files(&app_id, &registry).await;
                });
            }
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

    let tenant_id = crate::http_jwt::extract_tenant_id_from_headers(
        &headers,
        s.auth_disabled,
        s.jwt_key_pair.as_deref(),
    )?;

    remove_static_files(&application_id, &s.static_registry).await;

    let app_service = ApplicationServiceImpl::new(s.service_locator.clone(), None);
    let mut grpc_request = tonic::Request::new(UndeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
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

// ─── Validation ───────────────────────────────────────────────────────────────

/// Validate that `app_id` contains only safe characters for use as a path segment.
/// Rejects anything that could escape the wasm_apps_dir via PathBuf::join.
fn validate_app_id(app_id: &str) -> Result<(), String> {
    if app_id.is_empty() {
        return Err("application_id must not be empty".to_string());
    }
    if app_id.len() > 128 {
        return Err("application_id must not exceed 128 characters".to_string());
    }
    if !app_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "application_id '{}' contains invalid characters (allowed: a-z, A-Z, 0-9, -, _, .)",
            app_id
        ));
    }
    // Reject dot-only names like "." or ".."
    if app_id.chars().all(|c| c == '.') {
        return Err(format!("application_id '{}' is not allowed", app_id));
    }
    Ok(())
}

// ─── Zip extraction ───────────────────────────────────────────────────────────

/// Extract app.wasm, app-config.toml, and static/* from the zip bytes.
/// Returns (wasm_bytes, config_toml_str, static_files: Vec<(relative_path, bytes)>).
///
/// Security: rejects entries with path traversal (`..`, absolute paths) and
/// enforces a total decompressed-size cap to prevent zip-bomb exhaustion.
fn extract_app_zip(
    zip_bytes: &[u8],
) -> Result<(Option<Vec<u8>>, Option<String>, Vec<(String, Vec<u8>)>), String> {
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("Not a valid zip: {}", e))?;

    let mut wasm: Option<Vec<u8>> = None;
    let mut config: Option<String> = None;
    let mut static_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total_decompressed: usize = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;

        if entry.is_dir() {
            continue;
        }

        let name = entry.name().to_string();

        // Reject entries with path traversal components.
        reject_traversal(&name)?;

        // Cap decompressed size to prevent zip-bomb OOM.
        let entry_size_hint = entry.size() as usize;
        total_decompressed = total_decompressed.saturating_add(entry_size_hint);
        if total_decompressed > MAX_DECOMPRESSED_STATIC_BYTES {
            return Err(format!(
                "Zip decompressed content exceeds {} MB limit",
                MAX_DECOMPRESSED_STATIC_BYTES / (1024 * 1024)
            ));
        }

        // Use take() to enforce per-entry decompressed limit as well.
        let mut buf = Vec::new();
        entry
            .by_ref()
            .take(MAX_DECOMPRESSED_STATIC_BYTES as u64)
            .read_to_end(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", name, e))?;

        if name.ends_with(".wasm") && !name.contains('/') {
            // Accept any top-level *.wasm (e.g. app.wasm, chat_room_actor.wasm)
            wasm = Some(buf);
        } else if name == "app-config.toml" {
            config = Some(
                String::from_utf8(buf)
                    .map_err(|_| "app-config.toml is not valid UTF-8".to_string())?,
            );
        } else if name.starts_with("static/") {
            let rel = name["static/".len()..].to_string();
            if !rel.is_empty() {
                static_files.push((rel, buf));
            }
        }
    }

    Ok((wasm, config, static_files))
}

/// Reject a zip entry name that contains path traversal components.
fn reject_traversal(name: &str) -> Result<(), String> {
    for component in std::path::Path::new(name).components() {
        match component {
            Component::ParentDir => {
                return Err(format!("Path traversal in zip entry: '{}'", name));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Absolute path in zip entry: '{}'", name));
            }
            _ => {}
        }
    }
    // Reject null bytes (zip slip via null-terminated string confusion)
    if name.contains('\0') {
        return Err(format!("Null byte in zip entry name: '{}'", name));
    }
    Ok(())
}

// ─── Static file persistence ──────────────────────────────────────────────────

async fn persist_static_files(
    application_id: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf, String> {
    let base_dir = get_wasm_apps_dir();
    let static_dir = PathBuf::from(&base_dir).join(application_id).join("static");

    tokio::fs::create_dir_all(&static_dir)
        .await
        .map_err(|e| format!("create_dir_all {:?}: {}", static_dir, e))?;

    for (rel_path, bytes) in files {
        // Reject traversal in relative paths extracted from zip (double-check after extraction).
        reject_traversal(rel_path).map_err(|e| format!("Static file {}", e))?;

        let dest = static_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create_dir {:?}: {}", parent, e))?;
        }
        tokio::fs::write(&dest, bytes)
            .await
            .map_err(|e| format!("write {:?}: {}", dest, e))?;
    }

    tracing::debug!(
        application_id = %application_id,
        dir = %static_dir.display(),
        file_count = files.len(),
        "Static files written to disk"
    );
    Ok(static_dir)
}

async fn remove_static_files(application_id: &str, registry: &StaticRegistry) {
    // Remove from registry using application_id as the key.
    let removed = {
        let mut map = registry.write().await;
        map.remove(application_id)
    };

    if let Some(static_dir) = removed {
        tracing::info!(
            application_id = %application_id,
            "Static file mount unregistered"
        );
        if static_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&static_dir).await {
                tracing::warn!(
                    application_id = %application_id,
                    dir = %static_dir.display(),
                    error = %e,
                    "Failed to remove static directory"
                );
            }
        }
    } else {
        // Also clean up disk in case the node restarted and lost in-memory registry state.
        let base_dir = get_wasm_apps_dir();
        let app_static_dir = PathBuf::from(&base_dir).join(application_id).join("static");
        if app_static_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&app_static_dir).await {
                tracing::warn!(
                    application_id = %application_id,
                    dir = %app_static_dir.display(),
                    error = %e,
                    "Failed to remove static directory on disk"
                );
            }
        }
    }
}

fn get_wasm_apps_dir() -> String {
    use plexspaces_common::config_manager::{get_default_base_dir, get_env, ENV_WASM_APPS_DIR};
    get_env(ENV_WASM_APPS_DIR)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}/apps", get_default_base_dir()))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_app_id_valid() {
        assert!(validate_app_id("my-app").is_ok());
        assert!(validate_app_id("App_123").is_ok());
        assert!(validate_app_id("ts-ws-chat-room").is_ok());
        assert!(validate_app_id("v1.2.3").is_ok());
    }

    #[test]
    fn test_validate_app_id_rejects_traversal() {
        assert!(validate_app_id("../etc").is_err());
        assert!(validate_app_id("../../passwd").is_err());
        assert!(validate_app_id("/etc/passwd").is_err());
        assert!(validate_app_id("app/sub").is_err());
        assert!(validate_app_id("..").is_err());
        assert!(validate_app_id(".").is_err());
    }

    #[test]
    fn test_validate_app_id_rejects_empty_and_long() {
        assert!(validate_app_id("").is_err());
        assert!(validate_app_id(&"a".repeat(129)).is_err());
    }

    #[test]
    fn test_reject_traversal_in_zip() {
        assert!(reject_traversal("static/index.html").is_ok());
        assert!(reject_traversal("app.wasm").is_ok());
        assert!(reject_traversal("static/js/app.js").is_ok());

        assert!(reject_traversal("../etc/passwd").is_err());
        assert!(reject_traversal("static/../../etc").is_err());
        assert!(reject_traversal("/absolute/path").is_err());
        assert!(reject_traversal("null\0byte").is_err());
    }

    #[test]
    fn test_extract_app_zip_valid() {
        use std::io::Write;
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("app.wasm", opts).unwrap();
        zip.write_all(b"\0asm\x01\0\0\0").unwrap();
        zip.start_file("app-config.toml", opts).unwrap();
        zip.write_all(b"name = \"test\"\nversion = \"1.0.0\"\n")
            .unwrap();
        zip.start_file("static/index.html", opts).unwrap();
        zip.write_all(b"<html></html>").unwrap();

        let zip_bytes = zip.finish().unwrap().into_inner();
        let (wasm, config, statics) = extract_app_zip(&zip_bytes).unwrap();

        assert!(wasm.is_some());
        assert_eq!(&wasm.unwrap()[..4], b"\0asm");
        assert!(config.is_some());
        assert_eq!(statics.len(), 1);
        assert_eq!(statics[0].0, "index.html");
    }

    #[test]
    fn test_extract_app_zip_rejects_traversal() {
        use std::io::Write;
        let buf = Vec::new();
        let cursor = std::io::Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::FileOptions::<()>::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("app.wasm", opts).unwrap();
        zip.write_all(b"\0asm\x01\0\0\0").unwrap();
        // zip crate won't allow literal ".." in names, so use a surrogate path
        // that would bypass a naive string check but not a component check.
        // Test the static/ prefix stripping path with a safe name.
        zip.start_file("static/safe.html", opts).unwrap();
        zip.write_all(b"<html></html>").unwrap();

        let zip_bytes = zip.finish().unwrap().into_inner();
        let (wasm, _config, statics) = extract_app_zip(&zip_bytes).unwrap();
        assert!(wasm.is_some());
        assert_eq!(statics[0].0, "safe.html");
    }
}
