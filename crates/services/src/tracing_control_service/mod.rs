// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! TracingControlService — runtime per-actor tracing toggle.
//!
//! Implements both the gRPC `TracingControlService` tonic trait and the
//! `TracingGate` trait from `plexspaces-actor`. Operators enable/disable
//! tracing per actor-type, actor-id, or globally via gRPC or the HTTP bridge.
//!
//! ## Default state
//! All actors start with tracing **disabled** — `is_enabled()` returns `false`,
//! the actor task sets `Dispatch::none()`, and zero span overhead is paid.
//!
//! ## Thread safety
//! All fields are lock-free (`AtomicBool` + `DashMap`). `is_enabled()` costs
//! ~two DashMap `get` operations (~10 ns total), safe to call on every message.

use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use plexspaces_actor::TracingGate;
use plexspaces_proto::admin::v1::tracing_control_service_server::TracingControlService;
use plexspaces_proto::admin::v1::{
    DisableTracingRequest, DisableTracingResponse, EnableTracingRequest, EnableTracingResponse,
    GetTracingStatusRequest, GetTracingStatusResponse, TracingEntry, TracingTarget,
};
use tonic::{Request, Response, Status};

/// Runtime tracing toggle for actors.
#[derive(Debug)]
pub struct TracingControlServiceImpl {
    /// Master switch: TRACING_TARGET_ALL (default false).
    global_enabled: AtomicBool,
    /// Per-actor-type overrides: actor_type → enabled.
    enabled_types: DashMap<String, bool>,
    /// Per-actor-id overrides: actor_id → enabled.
    enabled_actors: DashMap<String, bool>,
}

impl TracingControlServiceImpl {
    /// Create with all tracing disabled.
    pub fn new() -> Self {
        Self {
            global_enabled: AtomicBool::new(false),
            enabled_types: DashMap::new(),
            enabled_actors: DashMap::new(),
        }
    }
}

impl Default for TracingControlServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl TracingGate for TracingControlServiceImpl {
    /// Check order: actor_id override → actor_type override → global.
    /// All reads are lock-free DashMap `get` (~5 ns each).
    fn is_enabled(&self, actor_type: &str, actor_id: &str) -> bool {
        if let Some(v) = self.enabled_actors.get(actor_id) {
            return *v;
        }
        if let Some(v) = self.enabled_types.get(actor_type) {
            return *v;
        }
        self.global_enabled.load(Ordering::Relaxed)
    }
}

#[tonic::async_trait]
impl TracingControlService for TracingControlServiceImpl {
    async fn enable_tracing(
        &self,
        request: Request<EnableTracingRequest>,
    ) -> Result<Response<EnableTracingResponse>, Status> {
        let req = request.into_inner();
        let target = TracingTarget::try_from(req.target)
            .unwrap_or(TracingTarget::TracingTargetUnspecified);
        match target {
            TracingTarget::TracingTargetAll | TracingTarget::TracingTargetUnspecified => {
                self.global_enabled.store(true, Ordering::Relaxed);
                tracing::info!("Tracing enabled globally");
            }
            TracingTarget::TracingTargetActorType => {
                if req.actor_type.is_empty() {
                    return Err(Status::invalid_argument("actor_type required for ACTOR_TYPE target"));
                }
                self.enabled_types.insert(req.actor_type.clone(), true);
                tracing::info!(actor_type = %req.actor_type, "Tracing enabled for actor type");
            }
            TracingTarget::TracingTargetActorId => {
                if req.actor_id.is_empty() {
                    return Err(Status::invalid_argument("actor_id required for ACTOR_ID target"));
                }
                self.enabled_actors.insert(req.actor_id.clone(), true);
                tracing::info!(actor_id = %req.actor_id, "Tracing enabled for actor id");
            }
        }
        Ok(Response::new(EnableTracingResponse {
            success: true,
            message: "Tracing enabled".to_string(),
        }))
    }

    async fn disable_tracing(
        &self,
        request: Request<DisableTracingRequest>,
    ) -> Result<Response<DisableTracingResponse>, Status> {
        let req = request.into_inner();
        let target = TracingTarget::try_from(req.target)
            .unwrap_or(TracingTarget::TracingTargetUnspecified);
        match target {
            TracingTarget::TracingTargetAll | TracingTarget::TracingTargetUnspecified => {
                self.global_enabled.store(false, Ordering::Relaxed);
                self.enabled_types.clear();
                self.enabled_actors.clear();
                tracing::info!("Tracing disabled globally (all overrides cleared)");
            }
            TracingTarget::TracingTargetActorType => {
                if req.actor_type.is_empty() {
                    return Err(Status::invalid_argument("actor_type required for ACTOR_TYPE target"));
                }
                // Insert false rather than remove so that is_enabled() returns false even
                // when global_enabled is true (removal would fall through to global=true).
                self.enabled_types.insert(req.actor_type.clone(), false);
                tracing::info!(actor_type = %req.actor_type, "Tracing disabled for actor type");
            }
            TracingTarget::TracingTargetActorId => {
                if req.actor_id.is_empty() {
                    return Err(Status::invalid_argument("actor_id required for ACTOR_ID target"));
                }
                // Same reasoning as ACTOR_TYPE above.
                self.enabled_actors.insert(req.actor_id.clone(), false);
                tracing::info!(actor_id = %req.actor_id, "Tracing disabled for actor id");
            }
        }
        Ok(Response::new(DisableTracingResponse {
            success: true,
            message: "Tracing disabled".to_string(),
        }))
    }

    async fn get_tracing_status(
        &self,
        _request: Request<GetTracingStatusRequest>,
    ) -> Result<Response<GetTracingStatusResponse>, Status> {
        let mut entries: Vec<TracingEntry> = Vec::new();
        for r in self.enabled_types.iter() {
            entries.push(TracingEntry {
                target: TracingTarget::TracingTargetActorType as i32,
                value: r.key().clone(),
                enabled: *r.value(),
            });
        }
        for r in self.enabled_actors.iter() {
            entries.push(TracingEntry {
                target: TracingTarget::TracingTargetActorId as i32,
                value: r.key().clone(),
                enabled: *r.value(),
            });
        }
        Ok(Response::new(GetTracingStatusResponse {
            global_enabled: self.global_enabled.load(Ordering::Relaxed),
            entries,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_disabled() {
        let svc = TracingControlServiceImpl::new();
        assert!(!svc.is_enabled("Counter", "tenant/ns/Counter/abc"));
    }

    #[tokio::test]
    async fn test_enable_global() {
        let svc = TracingControlServiceImpl::new();
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "r1".into(),
            target: TracingTarget::TracingTargetAll as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(svc.is_enabled("Counter", "tenant/ns/Counter/abc"));
    }

    #[tokio::test]
    async fn test_enable_actor_type() {
        let svc = TracingControlServiceImpl::new();
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "r1".into(),
            target: TracingTarget::TracingTargetActorType as i32,
            actor_type: "Counter".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(svc.is_enabled("Counter", "tenant/ns/Counter/abc"));
        assert!(!svc.is_enabled("Other", "tenant/ns/Other/abc"));
    }

    #[tokio::test]
    async fn test_enable_actor_id_overrides_type() {
        let svc = TracingControlServiceImpl::new();
        // Disable type, but enable specific id
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "r1".into(),
            target: TracingTarget::TracingTargetActorId as i32,
            actor_id: "tenant/ns/Counter/specific".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(svc.is_enabled("Counter", "tenant/ns/Counter/specific"));
        assert!(!svc.is_enabled("Counter", "tenant/ns/Counter/other"));
    }

    #[tokio::test]
    async fn test_disable_clears_all() {
        let svc = TracingControlServiceImpl::new();
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "r1".into(),
            target: TracingTarget::TracingTargetAll as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
        svc.disable_tracing(Request::new(DisableTracingRequest {
            request_id: "r2".into(),
            target: TracingTarget::TracingTargetAll as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(!svc.is_enabled("Counter", "tenant/ns/Counter/abc"));
    }

    /// Disabling a specific actor type must suppress it even when global tracing is on.
    ///
    /// Before the fix, `disable_tracing(ACTOR_TYPE)` called `.remove()` which deleted
    /// the DashMap entry. When `is_enabled()` then fell through to `global_enabled=true`
    /// the suppress had no effect. The fix uses `.insert(type, false)`.
    #[tokio::test]
    async fn test_disable_actor_type_suppresses_when_global_on() {
        let svc = TracingControlServiceImpl::new();
        // Turn global on.
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "e1".into(),
            target: TracingTarget::TracingTargetAll as i32,
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(svc.is_enabled("Sensitive", "ns/Sensitive/x"));

        // Suppress just this type.
        svc.disable_tracing(Request::new(DisableTracingRequest {
            request_id: "d1".into(),
            target: TracingTarget::TracingTargetActorType as i32,
            actor_type: "Sensitive".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(!svc.is_enabled("Sensitive", "ns/Sensitive/x"), "per-type suppress must override global=true");
        assert!(svc.is_enabled("Other", "ns/Other/y"), "unaffected types must remain enabled");
    }

    /// Same as above but for actor-id granularity.
    #[tokio::test]
    async fn test_disable_actor_id_suppresses_when_global_on() {
        let svc = TracingControlServiceImpl::new();
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "e1".into(),
            target: TracingTarget::TracingTargetAll as i32,
            ..Default::default()
        }))
        .await
        .unwrap();

        svc.disable_tracing(Request::new(DisableTracingRequest {
            request_id: "d1".into(),
            target: TracingTarget::TracingTargetActorId as i32,
            actor_id: "ns/Actor/quiet-one".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
        assert!(!svc.is_enabled("Actor", "ns/Actor/quiet-one"), "per-id suppress must override global=true");
        assert!(svc.is_enabled("Actor", "ns/Actor/loud-one"), "other ids must remain enabled");
    }

    #[tokio::test]
    async fn test_get_status_returns_entries() {
        let svc = TracingControlServiceImpl::new();
        svc.enable_tracing(Request::new(EnableTracingRequest {
            request_id: "r1".into(),
            target: TracingTarget::TracingTargetActorType as i32,
            actor_type: "Counter".into(),
            ..Default::default()
        }))
        .await
        .unwrap();
        let resp = svc
            .get_tracing_status(Request::new(GetTracingStatusRequest {
                request_id: "r2".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert!(!resp.global_enabled);
        assert_eq!(resp.entries.len(), 1);
        assert_eq!(resp.entries[0].value, "Counter");
        assert!(resp.entries[0].enabled);
    }
}
