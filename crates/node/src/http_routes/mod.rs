// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge routes for the node crate.
//!
//! Each sub-module owns one domain: actors, nodes, deploy, auth.
//! `all_http_routes()` composes them into a single `axum::Router`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use plexspaces_actor::{NodeConnectivity, ServiceLocator};
use plexspaces_services::actor_service::ActorServiceImpl;
use tokio::sync::RwLock;
pub mod actor_routes;
pub mod auth_routes;
pub mod deploy_routes;
pub mod node_routes;
pub mod ws_routes;

pub use actor_routes::actor_router;
pub use auth_routes::{auth_router, AuthRouteState};
pub use deploy_routes::deploy_router;
pub use node_routes::node_router;
pub use ws_routes::{ws_router, WsRouteState};

/// Shared registry mapping app_id → static directory on disk.
///
/// Populated at deploy time (when app zip contains a static/ dir and app-config.toml
/// declares [static] mount). Cleared at undeploy. The `/apps/:app_id/*filepath`
/// route consults this at request time — no restart needed for hot-deploy.
pub type StaticRegistry = Arc<RwLock<HashMap<String, PathBuf>>>;

/// Compose all HTTP bridge routes into a single router.
///
/// Static files are served dynamically via the shared `static_registry`.
/// Routes are registered at deploy time by `deploy_routes` without restarting
/// the server — the same hot-deploy model as a Java servlet container.
pub fn all_http_routes(
    actor_service: Arc<ActorServiceImpl>,
    service_locator: Arc<dyn ServiceLocator>,
    node_connectivity: Arc<dyn NodeConnectivity>,
    auth_disabled: bool,
    jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
    auth_state: Option<AuthRouteState>,
    ws_state: WsRouteState,
    static_registry: StaticRegistry,
) -> Router {
    let tenant_repo = auth_state.as_ref().map(|s| s.tenant_repo.clone());
    let base = actor_router(actor_service, auth_disabled, jwt_key_pair.clone())
        .merge(node_router(service_locator.clone(), auth_disabled, jwt_key_pair.clone()))
        .merge(deploy_router(
            service_locator,
            node_connectivity,
            auth_disabled,
            jwt_key_pair,
            tenant_repo,
            static_registry.clone(),
        ))
        .merge(ws_router(ws_state))
        .merge(static_apps_router(static_registry));

    if let Some(state) = auth_state {
        base.merge(auth_router(state))
    } else {
        base
    }
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Build the dynamic `/apps/:app_id/*filepath` router.
///
/// Serves static files for any deployed app that registered a static directory.
/// The registry is consulted on every request — adding or removing an app
/// takes effect immediately without a server restart.
fn static_apps_router(registry: StaticRegistry) -> Router {
    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    async fn serve_static(
        State(registry): State<StaticRegistry>,
        Path((app_id, filepath)): Path<(String, String)>,
    ) -> impl IntoResponse {
        let dir = {
            let map = registry.read().await;
            map.get(&app_id).cloned()
        };
        let Some(dir) = dir else {
            return (StatusCode::NOT_FOUND, "Not Found".to_string()).into_response();
        };
        let full = dir.join(&filepath);
        // Canonicalize to resolve `..` and symlinks before prefix-checking.
        // If the file doesn't exist yet canonicalize fails; we treat that as 404.
        let canonical = match std::fs::canonicalize(&full) {
            Ok(p) => p,
            Err(_) => return (StatusCode::NOT_FOUND, "Not Found".to_string()).into_response(),
        };
        let canonical_dir = match std::fs::canonicalize(&dir) {
            Ok(p) => p,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error".to_string()).into_response(),
        };
        if !canonical.starts_with(&canonical_dir) {
            return (StatusCode::FORBIDDEN, "Forbidden".to_string()).into_response();
        }
        match tokio::fs::read(&canonical).await {
            Ok(bytes) => {
                let mime = mime_from_path(&canonical);
                (
                    [(axum::http::header::CONTENT_TYPE, mime)],
                    bytes,
                )
                    .into_response()
            }
            Err(_) => (StatusCode::NOT_FOUND, "Not Found".to_string()).into_response(),
        }
    }

    // Serve index.html for bare /apps/:app_id/ requests (with trailing slash)
    async fn serve_index(
        State(registry): State<StaticRegistry>,
        Path(app_id): Path<String>,
    ) -> impl IntoResponse {
        let dir = {
            let map = registry.read().await;
            map.get(&app_id).cloned()
        };
        let Some(dir) = dir else {
            return (StatusCode::NOT_FOUND, "Not Found".to_string()).into_response();
        };
        let index = dir.join("index.html");
        match tokio::fs::read(&index).await {
            Ok(bytes) => (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                bytes,
            )
                .into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "Not Found".to_string()).into_response(),
        }
    }

    // Redirect /apps/:app_id (no trailing slash) → /apps/:app_id/
    async fn redirect_to_slash(
        Path(app_id): Path<String>,
    ) -> impl IntoResponse {
        let location = format!("/apps/{}/", app_id);
        (
            StatusCode::MOVED_PERMANENTLY,
            [(axum::http::header::LOCATION, location)],
        )
            .into_response()
    }

    Router::new()
        .route("/apps/:app_id", axum::routing::get(redirect_to_slash))
        .route("/apps/:app_id/", axum::routing::get(serve_index))
        .route("/apps/:app_id/*filepath", axum::routing::get(serve_static))
        .with_state(registry)
}

#[cfg(test)]
mod static_serve_unit_tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn make_registry_with_index(html: &str) -> (StaticRegistry, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let index_path = tmp.path().join("index.html");
        tokio::fs::write(&index_path, html.as_bytes()).await.unwrap();
        let registry: StaticRegistry = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        registry.write().await.insert("my-app".to_string(), tmp.path().to_path_buf());
        (registry, tmp)
    }

    #[tokio::test]
    async fn test_index_html_served_with_correct_content_type() {
        let (registry, _tmp) = make_registry_with_index("<html>ok</html>").await;
        let router = static_apps_router(registry);

        let resp = router
            .oneshot(Request::get("/apps/my-app/").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("text/html"), "expected text/html, got: {}", ct);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), b"<html>ok</html>");
    }

    #[tokio::test]
    async fn test_no_trailing_slash_redirects_to_slash() {
        let (registry, _tmp) = make_registry_with_index("<html>redirect</html>").await;
        let router = static_apps_router(registry);

        let resp = router
            .oneshot(Request::get("/apps/my-app").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::MOVED_PERMANENTLY);
        let location = resp.headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(location, "/apps/my-app/");
    }

    #[tokio::test]
    async fn test_missing_file_returns_404() {
        let (registry, _tmp) = make_registry_with_index("<html>test</html>").await;
        let router = static_apps_router(registry);

        let resp = router
            .oneshot(Request::get("/apps/my-app/missing.txt").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_unknown_app_returns_404() {
        let registry: StaticRegistry = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let router = static_apps_router(registry);

        let resp = router
            .oneshot(Request::get("/apps/not-deployed/").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_js_file_served_with_js_content_type() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("client.js"), b"console.log('hi')").await.unwrap();
        let registry: StaticRegistry = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        registry.write().await.insert("my-app".to_string(), tmp.path().to_path_buf());
        let router = static_apps_router(registry);

        let resp = router
            .oneshot(Request::get("/apps/my-app/client.js").body(axum::body::Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("javascript"), "expected javascript, got: {}", ct);
    }
}
