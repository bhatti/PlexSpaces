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

//! Execution trace facet — records an ordered, time-stamped sequence of method
//! calls for any actor and exports the complete trace on termination.
//!
//! ## Purpose
//! A **general-purpose** observability primitive.  It works for any actor —
//! workflows, sagas, agents, pipelines.  There is no coupling to agent/LLM
//! semantics; those belong in caller-supplied metadata via the `x-trace-label`
//! and `x-trace-*` headers.
//!
//! ## Facet priority
//! Default priority: **85** — runs after `DurabilityFacet` (90) so durable steps
//! are already committed before we capture them, and before `MetricsFacet` (80).
//!
//! ## Configuration (app-config.toml)
//! ```toml
//! { type = "execution_trace", priority = 85, config = {
//!   include_payloads   = true,
//!   max_steps          = 1000,
//!   max_retained_traces = 10,
//! }}
//! ```
//!
//! ## KV storage layout (KvTraceExporter)
//! ```text
//! trace:{trace_id}        → JSON ExecutionTrace
//! trace_index:{actor_id}  → JSON Vec<trace_id> (append-only per-actor list)
//! ```
//!
//! ## Concurrency model
//! `before_method` stores a `PendingStep` keyed by `correlation_id`.  The
//! correlation_id comes from the message header `correlation-id` (or a
//! generated fallback so concurrent messages never clobber each other).
//! All mutable state lives in `Mutex<InnerState>` so `&self` calls are safe.
//!
//! ## Outcome mapping from ExitReason
//! | ExitReason variant    | outcome string |
//! |-----------------------|----------------|
//! | Normal / Shutdown     | "completed"    |
//! | Error(msg)            | "error"        |
//! | Killed                | "killed"       |
//! | Linked                | "error"        |

use async_trait::async_trait;
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_facet::{ErrorHandling, ExitReason, Facet, FacetError, InterceptResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace_exporter::{ExecutionTrace, NoopTraceExporter, TraceExporter, TraceStep};

/// Default facet priority — after DurabilityFacet (90), before MetricsFacet (80).
pub const EXECUTION_TRACE_FACET_DEFAULT_PRIORITY: i32 = 85;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// State captured between `before_method` and `after_method` for one in-flight call.
struct PendingStep {
    method: String,
    label: String,
    input: Vec<u8>,
    started_at_ms: i64,
}

/// Parsed configuration.
struct Config {
    include_payloads: bool,
    max_steps: usize,
    max_retained_traces: usize,
}

impl Config {
    fn from_value(v: &Value) -> Self {
        Self {
            include_payloads: v
                .get("include_payloads")
                .and_then(|x| x.as_bool())
                .unwrap_or(true),
            max_steps: v.get("max_steps").and_then(|x| x.as_u64()).unwrap_or(1000) as usize,
            max_retained_traces: v
                .get("max_retained_traces")
                .and_then(|x| x.as_u64())
                .unwrap_or(10) as usize,
        }
    }
}

/// All mutable tracing state — protected behind a single `Mutex` so that
/// `before_method` and `after_method` (which take `&self`) can mutate safely.
struct InnerState {
    /// Actor ID once attached.
    actor_id: Option<String>,
    /// Current trace ID (ULID).
    trace_id: Option<String>,
    /// Wall-clock time of `on_attach` (ms).
    trace_started_at_ms: i64,
    /// Collected steps for the current lifecycle.
    steps: Vec<TraceStep>,
    /// In-flight steps keyed by correlation_id.
    pending: HashMap<String, PendingStep>,
    /// Completed traces retained in memory (bounded).
    completed_traces: Vec<ExecutionTrace>,
    /// Cached request context for the exporter.
    request_ctx: Option<RequestContext>,
}

impl InnerState {
    fn new() -> Self {
        Self {
            actor_id: None,
            trace_id: None,
            trace_started_at_ms: 0,
            steps: Vec::new(),
            pending: HashMap::new(),
            completed_traces: Vec::new(),
            request_ctx: None,
        }
    }
}

/// Execution trace facet.
pub struct ExecutionTraceFacet {
    config_value: Value,
    priority: i32,
    cfg: Config,
    exporter: Arc<dyn TraceExporter>,
    trace_metadata: HashMap<String, String>,
    inner: Mutex<InnerState>,
}

impl ExecutionTraceFacet {
    /// Create a new `ExecutionTraceFacet` with the `NoopTraceExporter`.
    pub fn new(config: Value, priority: i32) -> Self {
        Self::with_exporter(config, priority, Arc::new(NoopTraceExporter))
    }

    /// Create a new `ExecutionTraceFacet` with the provided exporter.
    pub fn with_exporter(config: Value, priority: i32, exporter: Arc<dyn TraceExporter>) -> Self {
        let cfg = Config::from_value(&config);
        let trace_metadata: HashMap<String, String> = config
            .get("metadata")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        Self {
            config_value: config,
            priority,
            cfg,
            exporter,
            trace_metadata,
            inner: Mutex::new(InnerState::new()),
        }
    }

    /// Returns all completed traces retained in memory.
    pub fn completed_traces(&self) -> Vec<ExecutionTrace> {
        self.inner.lock().unwrap().completed_traces.clone()
    }

    /// Returns the in-progress steps for the current trace (cloned).
    pub fn current_steps(&self) -> Vec<TraceStep> {
        self.inner.lock().unwrap().steps.clone()
    }

    /// Returns a snapshot of the current in-progress trace.
    pub fn current_trace_snapshot(&self) -> Option<ExecutionTrace> {
        let inner = self.inner.lock().unwrap();
        let actor_id = inner.actor_id.as_deref()?.to_string();
        let trace_id = inner.trace_id.as_deref()?.to_string();
        Some(ExecutionTrace {
            trace_id,
            actor_id,
            steps: inner.steps.clone(),
            outcome: "in_progress".to_string(),
            outcome_detail: String::new(),
            started_at_ms: inner.trace_started_at_ms,
            completed_at_ms: now_ms(),
            metadata: self.trace_metadata.clone(),
        })
    }

    fn map_exit_reason(reason: &ExitReason) -> (&'static str, String) {
        match reason {
            ExitReason::Normal | ExitReason::Shutdown => ("completed", String::new()),
            ExitReason::Error(msg) => ("error", msg.clone()),
            ExitReason::Killed => ("killed", String::new()),
            ExitReason::Linked { actor_id, reason } => (
                "error",
                format!("linked actor '{}' exited: {:?}", actor_id, reason),
            ),
        }
    }
}

#[async_trait]
impl Facet for ExecutionTraceFacet {
    fn facet_type(&self) -> &str {
        "execution_trace"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        let mut inner = self.inner.lock().unwrap();
        inner.actor_id = Some(actor_id.to_string());
        inner.trace_id = Some(ulid::Ulid::new().to_string());
        inner.trace_started_at_ms = now_ms();
        inner.steps.clear();
        inner.pending.clear();
        drop(inner);

        tracing::debug!(actor_id, "ExecutionTraceFacet: attached");
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        let mut inner = self.inner.lock().unwrap();
        inner.actor_id = None;
        inner.trace_id = None;
        Ok(())
    }

    /// Record the start of a method call.
    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        let mut inner = self.inner.lock().unwrap();

        if inner.steps.len() >= self.cfg.max_steps {
            return Ok(InterceptResult::Continue);
        }

        let correlation_id = headers
            .get("correlation-id")
            .cloned()
            .unwrap_or_else(|| ulid::Ulid::new().to_string());

        let label = headers
            .get("x-trace-label")
            .cloned()
            .unwrap_or_else(|| method.to_string());

        let input = if self.cfg.include_payloads {
            args.to_vec()
        } else {
            vec![]
        };

        inner.pending.insert(
            correlation_id,
            PendingStep {
                method: method.to_string(),
                label,
                input,
                started_at_ms: now_ms(),
            },
        );

        // Cache a request context for the exporter.
        if inner.request_ctx.is_none() {
            let actor_id = inner
                .actor_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            inner.request_ctx = Some(RequestContext::new_without_auth(
                actor_id,
                "execution_trace".to_string(),
            ));
        }

        Ok(InterceptResult::Continue)
    }

    /// Record the completion of a method call.
    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        result: &[u8],
        headers: &HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        let mut inner = self.inner.lock().unwrap();

        let correlation_id = headers.get("correlation-id").cloned().unwrap_or_default();
        let pending = inner.pending.remove(&correlation_id);
        let completed_at_ms = now_ms();

        let (started_at_ms, label, input) = match pending {
            Some(p) => (p.started_at_ms, p.label, p.input),
            None => (completed_at_ms, method.to_string(), vec![]),
        };

        if inner.steps.len() < self.cfg.max_steps {
            let output = if self.cfg.include_payloads {
                result.to_vec()
            } else {
                vec![]
            };

            let metadata: HashMap<String, String> = headers
                .iter()
                .filter(|(k, _)| k.starts_with("x-trace-"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            inner.steps.push(TraceStep {
                step_id: ulid::Ulid::new().to_string(),
                method: method.to_string(),
                label,
                input,
                output,
                started_at_ms,
                completed_at_ms,
                success: true,
                error: String::new(),
                metadata,
            });
        }

        Ok(InterceptResult::Continue)
    }

    /// Record a failed method call.
    async fn on_error(&self, method: &str, error: &str) -> Result<ErrorHandling, FacetError> {
        let mut inner = self.inner.lock().unwrap();

        let completed_at_ms = now_ms();

        let pending_key = inner
            .pending
            .iter()
            .find(|(_, p)| p.method == method)
            .map(|(k, _)| k.clone())
            .and_then(|k| {
                let p = inner.pending.remove(&k)?;
                Some((p.started_at_ms, p.label))
            });

        let (started_at_ms, label) =
            pending_key.unwrap_or_else(|| (completed_at_ms, method.to_string()));

        if inner.steps.len() < self.cfg.max_steps {
            inner.steps.push(TraceStep {
                step_id: ulid::Ulid::new().to_string(),
                method: method.to_string(),
                label,
                input: vec![],
                output: vec![],
                started_at_ms,
                completed_at_ms,
                success: false,
                error: error.to_string(),
                metadata: HashMap::new(),
            });
        }

        Ok(ErrorHandling::Propagate)
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }

    /// Export the completed trace.
    async fn on_terminate_start(
        &mut self,
        _actor_id: &str,
        reason: &ExitReason,
    ) -> Result<(), FacetError> {
        let (actor_id, trace_id, steps, started_at_ms, request_ctx) = {
            let mut inner = self.inner.lock().unwrap();
            let actor_id = match inner.actor_id.clone() {
                Some(id) => id,
                None => return Ok(()),
            };
            let trace_id = match inner.trace_id.clone() {
                Some(id) => id,
                None => return Ok(()),
            };
            let steps = std::mem::take(&mut inner.steps);
            let started_at_ms = inner.trace_started_at_ms;
            let request_ctx = inner.request_ctx.clone();
            (actor_id, trace_id, steps, started_at_ms, request_ctx)
        };

        let (outcome, outcome_detail) = Self::map_exit_reason(reason);
        let completed_at_ms = now_ms();

        let trace = ExecutionTrace {
            trace_id,
            actor_id: actor_id.clone(),
            steps,
            outcome: outcome.to_string(),
            outcome_detail,
            started_at_ms,
            completed_at_ms,
            metadata: self.trace_metadata.clone(),
        };

        // Retain in memory (bounded).
        {
            let mut inner = self.inner.lock().unwrap();
            inner.completed_traces.push(trace.clone());
            let max = self.cfg.max_retained_traces;
            while inner.completed_traces.len() > max {
                inner.completed_traces.remove(0);
            }
        }

        // Export — errors are non-fatal.
        let ctx = request_ctx.unwrap_or_else(|| {
            RequestContext::new_without_auth(actor_id.clone(), "execution_trace".to_string())
        });

        if let Err(e) = self.exporter.export(&ctx, &trace).await {
            tracing::warn!(
                actor_id = %actor_id,
                trace_id = %trace.trace_id,
                error = %e,
                "ExecutionTraceFacet: exporter failed (non-fatal)"
            );
        }

        metrics::counter!("execution_trace_exported_total",
            "actor_id" => actor_id.clone(),
            "outcome" => trace.outcome.clone()
        )
        .increment(1);

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_facet::ExitReason;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    // ── Spy exporter ──────────────────────────────────────────────────────────

    struct SpyExporter {
        exports: Arc<Mutex<Vec<ExecutionTrace>>>,
    }

    impl SpyExporter {
        fn new() -> (Arc<Mutex<Vec<ExecutionTrace>>>, Arc<Self>) {
            let exports = Arc::new(Mutex::new(Vec::new()));
            let spy = Arc::new(Self {
                exports: exports.clone(),
            });
            (exports, spy)
        }
    }

    #[async_trait]
    impl TraceExporter for SpyExporter {
        async fn export(
            &self,
            _ctx: &RequestContext,
            trace: &ExecutionTrace,
        ) -> Result<(), FacetError> {
            self.exports.lock().unwrap().push(trace.clone());
            Ok(())
        }
    }

    fn make_facet_spy() -> (Arc<Mutex<Vec<ExecutionTrace>>>, ExecutionTraceFacet) {
        let (exports, spy) = SpyExporter::new();
        let facet = ExecutionTraceFacet::with_exporter(
            json!({ "include_payloads": true }),
            EXECUTION_TRACE_FACET_DEFAULT_PRIORITY,
            spy,
        );
        (exports, facet)
    }

    fn make_headers(correlation_id: &str) -> HashMap<String, String> {
        let mut h = HashMap::new();
        h.insert("correlation-id".to_string(), correlation_id.to_string());
        h
    }

    fn make_headers_with_label(correlation_id: &str, label: &str) -> HashMap<String, String> {
        let mut h = make_headers(correlation_id);
        h.insert("x-trace-label".to_string(), label.to_string());
        h
    }

    // ── Construction ─────────────────────────────────────────────────────────

    #[test]
    fn test_default_priority() {
        let facet = ExecutionTraceFacet::new(json!({}), EXECUTION_TRACE_FACET_DEFAULT_PRIORITY);
        assert_eq!(facet.get_priority(), EXECUTION_TRACE_FACET_DEFAULT_PRIORITY);
        assert_eq!(facet.facet_type(), "execution_trace");
    }

    #[test]
    fn test_custom_priority() {
        let facet = ExecutionTraceFacet::new(json!({}), 70);
        assert_eq!(facet.get_priority(), 70);
    }

    // ── on_attach / on_detach ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_on_attach_sets_trace_id() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("my-actor", json!({})).await.unwrap();

        let inner = facet.inner.lock().unwrap();
        assert_eq!(inner.actor_id.as_deref(), Some("my-actor"));
        assert!(inner.trace_id.is_some());
        assert!(inner.trace_started_at_ms > 0);
    }

    #[tokio::test]
    async fn test_on_detach_clears_actor_id() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("my-actor", json!({})).await.unwrap();
        facet.on_detach("my-actor").await.unwrap();

        let inner = facet.inner.lock().unwrap();
        assert!(inner.actor_id.is_none());
        assert!(inner.trace_id.is_none());
    }

    // ── Step recording ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_before_after_records_step() {
        let mut facet = ExecutionTraceFacet::new(json!({ "include_payloads": true }), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        let headers = make_headers("corr-1");
        facet
            .before_method("handle_request", b"input-data", &headers)
            .await
            .unwrap();
        facet
            .after_method("handle_request", b"input-data", b"output-data", &headers)
            .await
            .unwrap();

        let steps = facet.current_steps();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].method, "handle_request");
        assert!(steps[0].success);
        assert!(steps[0].duration_ms() >= 0);
    }

    #[tokio::test]
    async fn test_step_label_from_header() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        let headers = make_headers_with_label("corr-1", "observe");
        facet
            .before_method("handle_request", b"", &headers)
            .await
            .unwrap();
        facet
            .after_method("handle_request", b"", b"", &headers)
            .await
            .unwrap();

        assert_eq!(facet.current_steps()[0].label, "observe");
    }

    #[tokio::test]
    async fn test_step_label_defaults_to_method() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        let headers = make_headers("corr-1");
        facet
            .before_method("my_method", b"", &headers)
            .await
            .unwrap();
        facet
            .after_method("my_method", b"", b"", &headers)
            .await
            .unwrap();

        assert_eq!(facet.current_steps()[0].label, "my_method");
    }

    #[tokio::test]
    async fn test_on_error_records_failed_step() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        let headers = make_headers("corr-1");
        facet
            .before_method("risky_method", b"input", &headers)
            .await
            .unwrap();
        facet
            .on_error("risky_method", "something exploded")
            .await
            .unwrap();

        let steps = facet.current_steps();
        assert_eq!(steps.len(), 1);
        assert!(!steps[0].success);
        assert_eq!(steps[0].error, "something exploded");
    }

    #[tokio::test]
    async fn test_multiple_steps_recorded() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        for i in 0..5 {
            let h = make_headers(&format!("corr-{}", i));
            facet.before_method("m", b"", &h).await.unwrap();
            facet.after_method("m", b"", b"", &h).await.unwrap();
        }

        assert_eq!(facet.current_steps().len(), 5);
    }

    // ── Payload inclusion ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_include_payloads_true() {
        let mut facet = ExecutionTraceFacet::new(json!({ "include_payloads": true }), 85);
        facet.on_attach("a1", json!({})).await.unwrap();
        let headers = make_headers("c1");
        facet
            .before_method("m", b"request-payload", &headers)
            .await
            .unwrap();
        facet
            .after_method("m", b"request-payload", b"response-payload", &headers)
            .await
            .unwrap();

        let steps = facet.current_steps();
        assert_eq!(steps[0].input, b"request-payload");
        assert_eq!(steps[0].output, b"response-payload");
    }

    #[tokio::test]
    async fn test_include_payloads_false() {
        let mut facet = ExecutionTraceFacet::new(json!({ "include_payloads": false }), 85);
        facet.on_attach("a1", json!({})).await.unwrap();
        let headers = make_headers("c1");
        facet
            .before_method("m", b"secret-payload", &headers)
            .await
            .unwrap();
        facet
            .after_method("m", b"secret-payload", b"secret-response", &headers)
            .await
            .unwrap();

        let steps = facet.current_steps();
        assert!(steps[0].input.is_empty());
        assert!(steps[0].output.is_empty());
    }

    // ── Max steps cap ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_max_steps_cap() {
        let mut facet = ExecutionTraceFacet::new(json!({ "max_steps": 3 }), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        for i in 0..10 {
            let h = make_headers(&format!("c{}", i));
            facet.before_method("m", b"", &h).await.unwrap();
            facet.after_method("m", b"", b"", &h).await.unwrap();
        }

        assert_eq!(facet.current_steps().len(), 3, "must not exceed max_steps");
    }

    // ── on_terminate_start / export ───────────────────────────────────────────

    #[tokio::test]
    async fn test_terminate_normal_exports_completed() {
        let (exports, mut facet) = make_facet_spy();
        facet.on_attach("a1", json!({})).await.unwrap();

        let h = make_headers("c1");
        facet.before_method("m", b"", &h).await.unwrap();
        facet.after_method("m", b"", b"ok", &h).await.unwrap();

        facet
            .on_terminate_start("a1", &ExitReason::Normal)
            .await
            .unwrap();

        let exports = exports.lock().unwrap();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].outcome, "completed");
        assert_eq!(exports[0].steps.len(), 1);
        assert_eq!(exports[0].actor_id, "a1");
    }

    #[tokio::test]
    async fn test_terminate_error_exports_error_outcome() {
        let (exports, mut facet) = make_facet_spy();
        facet.on_attach("a1", json!({})).await.unwrap();
        facet
            .on_terminate_start("a1", &ExitReason::Error("database down".into()))
            .await
            .unwrap();

        let exports = exports.lock().unwrap();
        assert_eq!(exports[0].outcome, "error");
        assert_eq!(exports[0].outcome_detail, "database down");
    }

    #[tokio::test]
    async fn test_terminate_killed_exports_killed_outcome() {
        let (exports, mut facet) = make_facet_spy();
        facet.on_attach("a1", json!({})).await.unwrap();
        facet
            .on_terminate_start("a1", &ExitReason::Killed)
            .await
            .unwrap();

        let exports = exports.lock().unwrap();
        assert_eq!(exports[0].outcome, "killed");
    }

    #[tokio::test]
    async fn test_terminate_shutdown_exports_completed() {
        let (exports, mut facet) = make_facet_spy();
        facet.on_attach("a1", json!({})).await.unwrap();
        facet
            .on_terminate_start("a1", &ExitReason::Shutdown)
            .await
            .unwrap();

        let exports = exports.lock().unwrap();
        assert_eq!(exports[0].outcome, "completed");
    }

    #[tokio::test]
    async fn test_terminate_linked_exports_error_outcome() {
        let (exports, mut facet) = make_facet_spy();
        facet.on_attach("a1", json!({})).await.unwrap();
        facet
            .on_terminate_start(
                "a1",
                &ExitReason::Linked {
                    actor_id: "supervisor".to_string(),
                    reason: Box::new(ExitReason::Error("crash".to_string())),
                },
            )
            .await
            .unwrap();

        let exports = exports.lock().unwrap();
        assert_eq!(exports[0].outcome, "error");
        assert!(exports[0].outcome_detail.contains("supervisor"));
    }

    // ── max_retained_traces eviction ──────────────────────────────────────────

    #[tokio::test]
    async fn test_max_retained_traces_eviction() {
        let max = 3usize;
        let (_, mut facet) = make_facet_spy();
        facet.cfg.max_retained_traces = max;

        for i in 0..5 {
            {
                let mut inner = facet.inner.lock().unwrap();
                inner.actor_id = Some("a1".to_string());
                inner.trace_id = Some(format!("trace-{}", i));
                inner.trace_started_at_ms = now_ms();
                inner.steps.clear();
            }
            facet
                .on_terminate_start("a1", &ExitReason::Normal)
                .await
                .unwrap();
        }

        let completed = facet.completed_traces();
        assert_eq!(
            completed.len(),
            max,
            "should retain at most max_retained_traces"
        );
        assert_eq!(completed[0].trace_id, "trace-2");
    }

    // ── Snapshot ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_current_trace_snapshot() {
        let mut facet = ExecutionTraceFacet::new(json!({}), 85);
        facet.on_attach("a1", json!({})).await.unwrap();

        let h = make_headers("c1");
        facet.before_method("m", b"", &h).await.unwrap();
        facet.after_method("m", b"", b"", &h).await.unwrap();

        let snapshot = facet.current_trace_snapshot().unwrap();
        assert_eq!(snapshot.actor_id, "a1");
        assert_eq!(snapshot.steps.len(), 1);
        assert_eq!(snapshot.outcome, "in_progress");
    }

    // ── ExitReason mapping ────────────────────────────────────────────────────

    #[test]
    fn test_exit_reason_mapping() {
        assert_eq!(
            ExecutionTraceFacet::map_exit_reason(&ExitReason::Normal).0,
            "completed"
        );
        assert_eq!(
            ExecutionTraceFacet::map_exit_reason(&ExitReason::Shutdown).0,
            "completed"
        );
        assert_eq!(
            ExecutionTraceFacet::map_exit_reason(&ExitReason::Error("e".into())).0,
            "error"
        );
        assert_eq!(
            ExecutionTraceFacet::map_exit_reason(&ExitReason::Killed).0,
            "killed"
        );
        assert_eq!(
            ExecutionTraceFacet::map_exit_reason(&ExitReason::Linked {
                actor_id: "x".into(),
                reason: Box::new(ExitReason::Error("r".into())),
            })
            .0,
            "error"
        );
    }
}
