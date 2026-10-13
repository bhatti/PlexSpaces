// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Shared outbox host logic for WASM actors.
// Both component_host.rs and simple_component_host.rs delegate here
// to avoid duplicating dispatch/status implementations.

use crate::host_functions::HostFunctions;
use plexspaces_proto::v1::common::ConflictResolutionPolicy;
use std::sync::atomic::{AtomicU64, Ordering};

static OUTBOX_DISPATCHED: AtomicU64 = AtomicU64::new(0);
static OUTBOX_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static OUTBOX_REJECTED: AtomicU64 = AtomicU64::new(0);

/// Parse a WASM guest string into the proto wire value for ConflictResolutionPolicy.
pub fn parse_conflict_resolution(s: &str) -> i32 {
    if s == "overwrite" {
        ConflictResolutionPolicy::ConflictResolutionPolicyOverwrite as i32
    } else {
        ConflictResolutionPolicy::ConflictResolutionPolicyKeepOriginal as i32
    }
}

/// Dispatch messages to a list of target actors, returning JSON `{"accepted":N,"rejected":M}`.
///
/// Increments both `metrics` facade counters (for Prometheus) and local atomics
/// (for `outbox_status_impl` to read without a metrics exporter).
pub async fn outbox_dispatch_impl(
    host_functions: &HostFunctions,
    actor_id: &str,
    target_actor_ids: &[String],
    payload: &[u8],
    message_type: &str,
    idempotency_key: &str,
    conflict_resolution: &str,
) -> Result<String, String> {
    metrics::counter!("plexspaces_wasm_outbox_dispatch_total").increment(1);

    let msg_type = if message_type.is_empty() {
        "cast"
    } else {
        message_type
    };
    let conflict_resolution_i32 = parse_conflict_resolution(conflict_resolution);

    let total = target_actor_ids.len() as u64;
    let mut accepted: u64 = 0;
    for target_id in target_actor_ids {
        match host_functions
            .send_message_ex(
                actor_id,
                target_id,
                msg_type,
                payload,
                idempotency_key,
                conflict_resolution_i32,
            )
            .await
        {
            Ok(()) => accepted += 1,
            Err(e) => {
                tracing::warn!(
                    target_id = %target_id,
                    error = %e,
                    "outbox_dispatch: target delivery failed"
                );
            }
        }
    }

    let rejected = total - accepted;
    metrics::counter!("plexspaces_wasm_outbox_dispatch_success_total").increment(accepted);

    OUTBOX_DISPATCHED.fetch_add(total, Ordering::Relaxed);
    OUTBOX_ACCEPTED.fetch_add(accepted, Ordering::Relaxed);
    OUTBOX_REJECTED.fetch_add(rejected, Ordering::Relaxed);

    Ok(format!(
        "{{\"accepted\":{},\"rejected\":{}}}",
        accepted, rejected
    ))
}

/// Return real outbox stats as JSON, keyed by `dispatcher_id`.
///
/// Reads node-wide atomic counters that `outbox_dispatch_impl` increments.
/// These counters are cumulative since node startup (monotonic).
pub fn outbox_status_impl(dispatcher_id: &str) -> String {
    metrics::counter!("plexspaces_wasm_outbox_status_total").increment(1);

    let dispatched = OUTBOX_DISPATCHED.load(Ordering::Relaxed);
    let accepted = OUTBOX_ACCEPTED.load(Ordering::Relaxed);
    let rejected = OUTBOX_REJECTED.load(Ordering::Relaxed);

    let escaped_id = dispatcher_id.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"dispatcher_id\":\"{}\",\"dispatched\":{},\"accepted\":{},\"rejected\":{}}}",
        escaped_id, dispatched, accepted, rejected
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_conflict_resolution_overwrite() {
        assert_eq!(
            parse_conflict_resolution("overwrite"),
            ConflictResolutionPolicy::ConflictResolutionPolicyOverwrite as i32
        );
    }

    #[test]
    fn parse_conflict_resolution_keep_original_explicit() {
        assert_eq!(
            parse_conflict_resolution("keep_original"),
            ConflictResolutionPolicy::ConflictResolutionPolicyKeepOriginal as i32
        );
    }

    #[test]
    fn parse_conflict_resolution_empty_defaults_to_keep_original() {
        assert_eq!(
            parse_conflict_resolution(""),
            ConflictResolutionPolicy::ConflictResolutionPolicyKeepOriginal as i32
        );
    }

    #[test]
    fn parse_conflict_resolution_unknown_defaults_to_keep_original() {
        assert_eq!(
            parse_conflict_resolution("unknown_value"),
            ConflictResolutionPolicy::ConflictResolutionPolicyKeepOriginal as i32
        );
    }

    #[test]
    fn outbox_status_returns_valid_json() {
        let status = outbox_status_impl("test-dispatcher");
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], "test-dispatcher");
        assert!(parsed["dispatched"].is_number());
        assert!(parsed["accepted"].is_number());
        assert!(parsed["rejected"].is_number());
    }

    #[test]
    fn outbox_status_escapes_json_injection() {
        let status = outbox_status_impl("test\"evil\\id");
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], "test\"evil\\id");
    }

    #[test]
    fn outbox_status_escapes_nested_json() {
        let status = outbox_status_impl(r#"a","hacked":"true"#);
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], r#"a","hacked":"true"#);
        assert!(parsed.get("hacked").is_none());
    }

    #[test]
    fn outbox_counters_are_monotonic() {
        let s1 = outbox_status_impl("c1");
        let p1: serde_json::Value = serde_json::from_str(&s1).unwrap();
        let d1 = p1["dispatched"].as_u64().unwrap();

        OUTBOX_DISPATCHED.fetch_add(5, Ordering::Relaxed);
        OUTBOX_ACCEPTED.fetch_add(3, Ordering::Relaxed);
        OUTBOX_REJECTED.fetch_add(2, Ordering::Relaxed);

        let s2 = outbox_status_impl("c2");
        let p2: serde_json::Value = serde_json::from_str(&s2).unwrap();
        assert_eq!(p2["dispatched"].as_u64().unwrap(), d1 + 5);
        assert!(p2["accepted"].as_u64().unwrap() >= 3);
        assert!(p2["rejected"].as_u64().unwrap() >= 2);
    }
}
