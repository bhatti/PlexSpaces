// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge for TracingControlService.
//!
//! `POST /api/v1/admin/tracing/enable`  → `TracingControlService::enable_tracing`
//! `POST /api/v1/admin/tracing/disable` → `TracingControlService::disable_tracing`
//! `GET  /api/v1/admin/tracing/status`  → `TracingControlService::get_tracing_status`

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use plexspaces_proto::admin::v1::tracing_control_service_server::TracingControlService as TracingControlServiceTrait;
use plexspaces_proto::admin::v1::{
    DisableTracingRequest, EnableTracingRequest, GetTracingStatusRequest, TracingTarget,
};
use plexspaces_services::tracing_control_service::TracingControlServiceImpl;
use serde_json::Value;

/// Parse `"target"` from a JSON body as either a numeric code or a proto enum name string.
/// Returns `Err((400, msg))` when the field is present but unrecognizable.
fn parse_target(body: &Value) -> Result<i32, (StatusCode, String)> {
    let v = match body.get("target") {
        None => return Ok(0),
        Some(v) => v,
    };
    if let Some(n) = v.as_i64() {
        return Ok(n as i32);
    }
    if let Some(s) = v.as_str() {
        return TracingTarget::from_str_name(s)
            .map(|t| t as i32)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("unknown tracing target: {s}"),
                )
            });
    }
    Err((
        StatusCode::BAD_REQUEST,
        "target must be an integer or a TracingTarget name string".to_string(),
    ))
}

/// State shared by admin route handlers.
#[derive(Clone)]
pub struct AdminRouteState {
    /// TracingControlService implementation.
    pub tracing_service: Arc<TracingControlServiceImpl>,
}

/// Build the admin HTTP bridge router.
pub fn admin_router(tracing_service: Arc<TracingControlServiceImpl>) -> Router {
    let state = AdminRouteState { tracing_service };
    Router::new()
        .route(
            "/api/v1/admin/tracing/enable",
            post(enable_tracing),
        )
        .route(
            "/api/v1/admin/tracing/disable",
            post(disable_tracing),
        )
        .route(
            "/api/v1/admin/tracing/status",
            get(get_tracing_status),
        )
        .with_state(state)
}

async fn enable_tracing(
    State(s): State<AdminRouteState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let req = EnableTracingRequest {
        request_id: body
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        target: parse_target(&body)?,
        actor_type: body
            .get("actor_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        actor_id: body
            .get("actor_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };
    s.tracing_service
        .enable_tracing(tonic::Request::new(req))
        .await
        .map(|resp| {
            let r = resp.into_inner();
            Json(serde_json::json!({ "success": r.success, "message": r.message }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message().to_string()))
}

async fn disable_tracing(
    State(s): State<AdminRouteState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let req = DisableTracingRequest {
        request_id: body
            .get("request_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        target: parse_target(&body)?,
        actor_type: body
            .get("actor_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        actor_id: body
            .get("actor_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };
    s.tracing_service
        .disable_tracing(tonic::Request::new(req))
        .await
        .map(|resp| {
            let r = resp.into_inner();
            Json(serde_json::json!({ "success": r.success, "message": r.message }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message().to_string()))
}

async fn get_tracing_status(
    State(s): State<AdminRouteState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    s.tracing_service
        .get_tracing_status(tonic::Request::new(GetTracingStatusRequest {
            request_id: String::new(),
        }))
        .await
        .map(|resp| {
            let r = resp.into_inner();
            let entries: Vec<Value> = r
                .entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "target": e.target,
                        "value": e.value,
                        "enabled": e.enabled,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "global_enabled": r.global_enabled,
                "entries": entries,
            }))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.message().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn make_router() -> Router {
        admin_router(Arc::new(TracingControlServiceImpl::new()))
    }

    #[tokio::test]
    async fn test_status_returns_200_with_empty_entries() {
        let router = make_router();
        let resp = router
            .oneshot(
                Request::get("/api/v1/admin/tracing/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["global_enabled"], false);
        assert_eq!(json["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_enable_and_status_round_trip() {
        let svc = Arc::new(TracingControlServiceImpl::new());
        let router = admin_router(svc.clone());
        let payload = serde_json::json!({
            "request_id": "test",
            "target": 1,
            "actor_type": "Counter"
        });
        let resp = router
            .oneshot(
                Request::post("/api/v1/admin/tracing/enable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        use plexspaces_actor::TracingGate;
        assert!(svc.is_enabled("Counter", "tenant/ns/Counter/abc"));
    }

    #[tokio::test]
    async fn test_enable_with_string_target() {
        let svc = Arc::new(TracingControlServiceImpl::new());
        let router = admin_router(svc.clone());
        let payload = serde_json::json!({
            "request_id": "t2",
            "target": "TRACING_TARGET_ACTOR_TYPE",
            "actor_type": "Worker"
        });
        let resp = router
            .oneshot(
                Request::post("/api/v1/admin/tracing/enable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        use plexspaces_actor::TracingGate;
        assert!(svc.is_enabled("Worker", "tenant/ns/Worker/x"));
    }

    #[tokio::test]
    async fn test_enable_with_unknown_target_returns_400() {
        let router = make_router();
        let payload = serde_json::json!({ "target": "INVALID_TARGET" });
        let resp = router
            .oneshot(
                Request::post("/api/v1/admin/tracing/enable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// Verify that disable/ACTOR_TYPE suppresses tracing even when global is on.
    ///
    /// This exercises the correctness fix where `.remove()` was replaced with
    /// `.insert(type, false)` so that `is_enabled()` returns false when global=true
    /// but the specific type is explicitly disabled.
    #[tokio::test]
    async fn test_disable_actor_type_suppresses_when_global_on() {
        use plexspaces_actor::TracingGate;
        let svc = Arc::new(TracingControlServiceImpl::new());
        let router = admin_router(svc.clone());

        // Enable tracing globally first.
        let enable_payload = serde_json::json!({ "request_id": "e1", "target": 3 });
        let resp = router
            .clone()
            .oneshot(
                Request::post("/api/v1/admin/tracing/enable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(enable_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(svc.is_enabled("SensitiveActor", "any-id"), "global on → should be enabled");

        // Now disable just the SensitiveActor type.
        let disable_payload = serde_json::json!({
            "request_id": "d1",
            "target": 1,
            "actor_type": "SensitiveActor"
        });
        let resp = router
            .oneshot(
                Request::post("/api/v1/admin/tracing/disable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(disable_payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // SensitiveActor must be suppressed even though global is still on.
        assert!(!svc.is_enabled("SensitiveActor", "any-id"), "per-type disable must override global=true");
        // Other actor types must still be enabled.
        assert!(svc.is_enabled("OtherActor", "other-id"), "other types should remain enabled");
    }

    #[tokio::test]
    async fn test_disable_global_clears_all() {
        use plexspaces_actor::TracingGate;
        let svc = Arc::new(TracingControlServiceImpl::new());
        let router = admin_router(svc.clone());

        // Enable global + one specific type.
        let enable_all = serde_json::json!({ "request_id": "ea", "target": 3 });
        router
            .clone()
            .oneshot(
                Request::post("/api/v1/admin/tracing/enable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(enable_all.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Disable globally — all overrides should be cleared.
        let disable_all = serde_json::json!({ "request_id": "da", "target": 3 });
        let resp = router
            .oneshot(
                Request::post("/api/v1/admin/tracing/disable")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(disable_all.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!svc.is_enabled("AnyActor", "any-id"), "global disable must turn everything off");
    }
}
