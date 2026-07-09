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

//! Trace exporter trait and built-in implementations.
//!
//! ## Purpose
//! Provides the `TraceExporter` trait and two implementations:
//! - `KvTraceExporter` — writes traces to a `KeyValueStore` under `trace:{trace_id}`,
//!   with a per-actor index at `trace_index:{actor_id}` (append-only list of trace IDs)
//! - `NoopTraceExporter` — discards all traces, useful for tests
//!
//! ## Storage layout
//! ```text
//! trace:{trace_id}          → JSON-serialised ExecutionTrace (full)
//! trace_index:{actor_id}    → JSON array of trace_id strings (ordered, append-only)
//! ```
//!
//! This layout mirrors how distributed tracing systems (Jaeger, Zipkin) work:
//! individual spans/traces are stored by ID, and a per-service index enables
//! "find all traces for actor X" without full-table scan.

use async_trait::async_trait;
use plexspaces_common::{KeyValueStore, RequestContext};
use plexspaces_facet::FacetError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ─── Data types ──────────────────────────────────────────────────────────────

/// A single step within an execution trace.
///
/// Recorded by `ExecutionTraceFacet` for every method call on the actor.
/// The `label` field lets callers attach semantic meaning without baking
/// vocabulary (OODA, saga, etc.) into the framework.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceStep {
    /// Unique step ID (ULID — lexicographically sortable)
    pub step_id: String,
    /// Method name called on the actor
    pub method: String,
    /// Caller-supplied semantic label (from `x-trace-label` header, defaults to method)
    pub label: String,
    /// Input bytes (present only when `include_payloads = true`)
    #[serde(default)]
    pub input: Vec<u8>,
    /// Output bytes (present only when `include_payloads = true`)
    #[serde(default)]
    pub output: Vec<u8>,
    /// Wall-clock start time (milliseconds since Unix epoch)
    pub started_at_ms: i64,
    /// Wall-clock completion time (milliseconds since Unix epoch)
    pub completed_at_ms: i64,
    /// Whether the method completed without error
    pub success: bool,
    /// Error message when `success = false`
    #[serde(default)]
    pub error: String,
    /// Arbitrary key-value metadata (token counts, model names, etc.)
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl TraceStep {
    /// Duration in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        self.completed_at_ms.saturating_sub(self.started_at_ms)
    }
}

/// A complete execution trace for one actor lifecycle.
///
/// Produced by `ExecutionTraceFacet` on actor termination.
/// Written to KV storage by `KvTraceExporter`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionTrace {
    /// Unique trace ID (ULID)
    pub trace_id: String,
    /// Actor that produced this trace
    pub actor_id: String,
    /// Ordered steps in method-invocation order
    pub steps: Vec<TraceStep>,
    /// Outcome: "completed", "error", "killed", "timeout"
    pub outcome: String,
    /// Human-readable detail about the outcome (e.g. error message)
    #[serde(default)]
    pub outcome_detail: String,
    /// Wall-clock start time (ms since epoch, set on `on_attach`)
    pub started_at_ms: i64,
    /// Wall-clock end time (ms since epoch, set on `on_terminate_start`)
    pub completed_at_ms: i64,
    /// Arbitrary key-value metadata (eval_run_id, scenario_id, etc.)
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl ExecutionTrace {
    /// Total duration from attach to termination, in milliseconds.
    pub fn duration_ms(&self) -> i64 {
        self.completed_at_ms.saturating_sub(self.started_at_ms)
    }

    /// Number of successful steps.
    pub fn success_count(&self) -> usize {
        self.steps.iter().filter(|s| s.success).count()
    }

    /// Number of failed steps.
    pub fn error_count(&self) -> usize {
        self.steps.iter().filter(|s| !s.success).count()
    }
}

// ─── TraceExporter trait ─────────────────────────────────────────────────────

/// Export a completed `ExecutionTrace` to a backend.
///
/// Implemented by `KvTraceExporter` (default) and `NoopTraceExporter` (testing).
/// Custom exporters can write to any sink: message queues, observability pipelines,
/// databases, etc.
#[async_trait]
pub trait TraceExporter: Send + Sync {
    /// Export the completed trace.
    ///
    /// Called exactly once per actor lifecycle, from `on_terminate_start`.
    /// Errors are logged but do not affect actor termination.
    async fn export(
        &self,
        ctx: &RequestContext,
        trace: &ExecutionTrace,
    ) -> Result<(), FacetError>;
}

// ─── NoopTraceExporter ───────────────────────────────────────────────────────

/// A trace exporter that discards all traces.  Use in tests or when observability
/// is intentionally disabled.
pub struct NoopTraceExporter;

#[async_trait]
impl TraceExporter for NoopTraceExporter {
    async fn export(
        &self,
        _ctx: &RequestContext,
        _trace: &ExecutionTrace,
    ) -> Result<(), FacetError> {
        Ok(())
    }
}

// ─── KvTraceExporter ─────────────────────────────────────────────────────────

/// Writes traces to a `KeyValueStore`.
///
/// ## Storage layout
/// ```text
/// trace:{trace_id}        → JSON-serialised ExecutionTrace (full)
/// trace_index:{actor_id}  → JSON-serialised Vec<String> of trace IDs (append-only)
/// ```
///
/// This enables two access patterns:
/// 1. Retrieve a specific trace: `kv.get("trace:{id}")`
/// 2. Retrieve all traces for an actor: `kv.get("trace_index:{actor_id}")` → list of IDs
pub struct KvTraceExporter {
    kv: Arc<dyn KeyValueStore>,
}

impl KvTraceExporter {
    /// Create a new `KvTraceExporter` backed by `kv`.
    pub fn new(kv: Arc<dyn KeyValueStore>) -> Self {
        Self { kv }
    }

    /// KV key for the full trace blob.
    fn trace_key(trace_id: &str) -> String {
        format!("trace:{}", trace_id)
    }

    /// KV key for the per-actor index of trace IDs.
    fn index_key(actor_id: &str) -> String {
        format!("trace_index:{}", actor_id)
    }
}

#[async_trait]
impl TraceExporter for KvTraceExporter {
    async fn export(
        &self,
        ctx: &RequestContext,
        trace: &ExecutionTrace,
    ) -> Result<(), FacetError> {
        // 1. Serialise and write the full trace.
        let trace_json = serde_json::to_vec(trace).map_err(|e| {
            FacetError::InterceptionFailed(format!(
                "KvTraceExporter: failed to serialise trace '{}': {}",
                trace.trace_id, e
            ))
        })?;

        self.kv
            .put(ctx, &Self::trace_key(&trace.trace_id), trace_json)
            .await
            .map_err(|e| {
                FacetError::InterceptionFailed(format!(
                    "KvTraceExporter: kv.put trace '{}' failed: {}",
                    trace.trace_id, e
                ))
            })?;

        // 2. Append to the per-actor index (read-modify-write; not atomic, acceptable
        //    here because the actor is the only writer for its own index and actors
        //    process one message at a time).
        let index_key = Self::index_key(&trace.actor_id);
        let mut ids: Vec<String> = match self.kv.get(ctx, &index_key).await {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Ok(None) => Vec::new(),
            Err(e) => {
                tracing::warn!(
                    actor_id = %trace.actor_id,
                    error = %e,
                    "KvTraceExporter: could not read existing index, starting fresh"
                );
                Vec::new()
            }
        };

        ids.push(trace.trace_id.clone());

        let index_json = serde_json::to_vec(&ids).map_err(|e| {
            FacetError::InterceptionFailed(format!(
                "KvTraceExporter: failed to serialise index for '{}': {}",
                trace.actor_id, e
            ))
        })?;

        self.kv
            .put(ctx, &index_key, index_json)
            .await
            .map_err(|e| {
                FacetError::InterceptionFailed(format!(
                    "KvTraceExporter: kv.put index for '{}' failed: {}",
                    trace.actor_id, e
                ))
            })?;

        tracing::debug!(
            trace_id = %trace.trace_id,
            actor_id = %trace.actor_id,
            steps = trace.steps.len(),
            outcome = %trace.outcome,
            duration_ms = trace.duration_ms(),
            "KvTraceExporter: exported trace"
        );

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_common::{KeyValueStoreError, RequestContext, RequestContextExt};
    use std::collections::HashMap;
    use std::sync::Mutex;

    // Minimal in-memory KV store for testing.
    struct MemKv {
        data: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemKv {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                data: Mutex::new(HashMap::new()),
            })
        }

        fn get_json<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Option<T> {
            let data = self.data.lock().unwrap();
            data.get(key)
                .and_then(|b| serde_json::from_slice(b).ok())
        }
    }

    #[async_trait]
    impl KeyValueStore for MemKv {
        async fn get(&self, _: &RequestContext, key: &str) -> Result<Option<Vec<u8>>, KeyValueStoreError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }
        async fn put(&self, _: &RequestContext, key: &str, value: Vec<u8>) -> Result<(), KeyValueStoreError> {
            self.data.lock().unwrap().insert(key.to_string(), value);
            Ok(())
        }
        async fn put_with_ttl(&self, ctx: &RequestContext, key: &str, value: Vec<u8>, _: std::time::Duration) -> Result<(), KeyValueStoreError> {
            self.put(ctx, key, value).await
        }
        async fn delete(&self, _: &RequestContext, key: &str) -> Result<(), KeyValueStoreError> {
            self.data.lock().unwrap().remove(key);
            Ok(())
        }
        async fn exists(&self, _: &RequestContext, key: &str) -> Result<bool, KeyValueStoreError> {
            Ok(self.data.lock().unwrap().contains_key(key))
        }
        async fn list_keys(&self, _: &RequestContext, prefix: &str) -> Result<Vec<String>, KeyValueStoreError> {
            Ok(self.data.lock().unwrap().keys().filter(|k| k.starts_with(prefix)).cloned().collect())
        }
        async fn cas(&self, _: &RequestContext, key: &str, expected: Option<Vec<u8>>, new_value: Vec<u8>) -> Result<bool, KeyValueStoreError> {
            let mut data = self.data.lock().unwrap();
            if data.get(key).cloned() == expected {
                data.insert(key.to_string(), new_value);
                Ok(true)
            } else {
                Ok(false)
            }
        }
        async fn increment(&self, _: &RequestContext, key: &str, delta: i64) -> Result<i64, KeyValueStoreError> {
            let mut data = self.data.lock().unwrap();
            let v: i64 = data.get(key).and_then(|b| std::str::from_utf8(b).ok()).and_then(|s| s.parse().ok()).unwrap_or(0) + delta;
            data.insert(key.to_string(), v.to_string().into_bytes());
            Ok(v)
        }
    }

    fn ctx() -> RequestContext {
        RequestContext::new_without_auth("tenant".to_string(), "ns".to_string())
    }

    fn make_trace(trace_id: &str, actor_id: &str, steps: usize) -> ExecutionTrace {
        let step_list: Vec<TraceStep> = (0..steps)
            .map(|i| TraceStep {
                step_id: format!("step-{}", i),
                method: "handle_request".to_string(),
                label: "handle_request".to_string(),
                input: vec![],
                output: vec![],
                started_at_ms: 1000 + i as i64 * 10,
                completed_at_ms: 1005 + i as i64 * 10,
                success: true,
                error: String::new(),
                metadata: HashMap::new(),
            })
            .collect();

        ExecutionTrace {
            trace_id: trace_id.to_string(),
            actor_id: actor_id.to_string(),
            steps: step_list,
            outcome: "completed".to_string(),
            outcome_detail: String::new(),
            started_at_ms: 1000,
            completed_at_ms: 2000,
            metadata: HashMap::new(),
        }
    }

    // ── TraceStep ─────────────────────────────────────────────────────────────

    #[test]
    fn test_trace_step_duration() {
        let step = TraceStep {
            step_id: "s1".into(),
            method: "m".into(),
            label: "m".into(),
            input: vec![],
            output: vec![],
            started_at_ms: 1000,
            completed_at_ms: 1050,
            success: true,
            error: String::new(),
            metadata: HashMap::new(),
        };
        assert_eq!(step.duration_ms(), 50);
    }

    #[test]
    fn test_execution_trace_counts() {
        let mut trace = make_trace("t1", "a1", 3);
        trace.steps[1].success = false;
        trace.steps[1].error = "oops".into();

        assert_eq!(trace.success_count(), 2);
        assert_eq!(trace.error_count(), 1);
        assert_eq!(trace.duration_ms(), 1000);
    }

    // ── NoopTraceExporter ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_noop_exporter_always_succeeds() {
        let exporter = NoopTraceExporter;
        let trace = make_trace("t1", "actor-1", 2);
        let result = exporter.export(&ctx(), &trace).await;
        assert!(result.is_ok());
    }

    // ── KvTraceExporter ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_kv_exporter_writes_trace() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());
        let trace = make_trace("trace-001", "actor-abc", 3);

        exporter.export(&ctx(), &trace).await.unwrap();

        // Trace should be stored at "trace:trace-001"
        let stored: ExecutionTrace = kv.get_json("trace:trace-001").expect("trace not found");
        assert_eq!(stored.trace_id, "trace-001");
        assert_eq!(stored.steps.len(), 3);
        assert_eq!(stored.outcome, "completed");
    }

    #[tokio::test]
    async fn test_kv_exporter_writes_index() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());
        let trace = make_trace("trace-001", "actor-abc", 1);

        exporter.export(&ctx(), &trace).await.unwrap();

        // Index should be at "trace_index:actor-abc"
        let index: Vec<String> = kv.get_json("trace_index:actor-abc").expect("index not found");
        assert_eq!(index, vec!["trace-001"]);
    }

    #[tokio::test]
    async fn test_kv_exporter_appends_to_index() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());

        // Export two traces for the same actor
        exporter.export(&ctx(), &make_trace("t1", "actor-abc", 1)).await.unwrap();
        exporter.export(&ctx(), &make_trace("t2", "actor-abc", 2)).await.unwrap();

        let index: Vec<String> = kv.get_json("trace_index:actor-abc").unwrap();
        assert_eq!(index.len(), 2);
        assert!(index.contains(&"t1".to_string()));
        assert!(index.contains(&"t2".to_string()));
    }

    #[tokio::test]
    async fn test_kv_exporter_different_actors_separate_indexes() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());

        exporter.export(&ctx(), &make_trace("t1", "actor-A", 1)).await.unwrap();
        exporter.export(&ctx(), &make_trace("t2", "actor-B", 1)).await.unwrap();

        let idx_a: Vec<String> = kv.get_json("trace_index:actor-A").unwrap();
        let idx_b: Vec<String> = kv.get_json("trace_index:actor-B").unwrap();
        assert_eq!(idx_a, vec!["t1"]);
        assert_eq!(idx_b, vec!["t2"]);
    }

    #[tokio::test]
    async fn test_kv_exporter_preserves_metadata() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());

        let mut trace = make_trace("t1", "actor-x", 1);
        trace.metadata.insert("eval_run_id".into(), "run-007".into());
        trace.metadata.insert("scenario_id".into(), "sc-math-01".into());

        exporter.export(&ctx(), &trace).await.unwrap();

        let stored: ExecutionTrace = kv.get_json("trace:t1").unwrap();
        assert_eq!(stored.metadata.get("eval_run_id").map(|s| s.as_str()), Some("run-007"));
        assert_eq!(stored.metadata.get("scenario_id").map(|s| s.as_str()), Some("sc-math-01"));
    }

    #[tokio::test]
    async fn test_kv_exporter_preserves_error_steps() {
        let kv = MemKv::new();
        let exporter = KvTraceExporter::new(kv.clone());

        let mut trace = make_trace("t1", "actor-x", 2);
        trace.steps[0].success = false;
        trace.steps[0].error = "tool_call schema validation failed".into();
        trace.outcome = "error".into();
        trace.outcome_detail = "step 0 failed".into();

        exporter.export(&ctx(), &trace).await.unwrap();

        let stored: ExecutionTrace = kv.get_json("trace:t1").unwrap();
        assert_eq!(stored.outcome, "error");
        assert!(!stored.steps[0].success);
        assert_eq!(stored.steps[0].error, "tool_call schema validation failed");
    }
}
