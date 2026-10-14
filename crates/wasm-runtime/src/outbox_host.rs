// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Shared outbox host logic for WASM actors.
// Uses the core ConcurrencyLimiter (AIMD) and compute_backoff from the outbox
// crate — the same flow control that embedded Rust actors get.

use crate::host_functions::HostFunctions;
use plexspaces_common::retry::compute_backoff;
use plexspaces_outbox::concurrency_limiter::ConcurrencyLimiter;
use plexspaces_outbox::RESOURCE_EXHAUSTED;
use plexspaces_proto::outbox::v1::ConcurrencyConfig;
use plexspaces_proto::v1::common::ConflictResolutionPolicy;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static OUTBOX_DISPATCHED: AtomicU64 = AtomicU64::new(0);
static OUTBOX_ACCEPTED: AtomicU64 = AtomicU64::new(0);
static OUTBOX_REJECTED: AtomicU64 = AtomicU64::new(0);

const DEFAULT_MAX_RETRIES: u32 = 3;

/// Parse a WASM guest string into the proto wire value for ConflictResolutionPolicy.
pub fn parse_conflict_resolution(s: &str) -> i32 {
    if s == "overwrite" {
        ConflictResolutionPolicy::ConflictResolutionPolicyOverwrite as i32
    } else {
        ConflictResolutionPolicy::ConflictResolutionPolicyKeepOriginal as i32
    }
}

/// Create a shared ConcurrencyLimiter with default AIMD settings for WASM outbox dispatch.
pub fn default_wasm_limiter() -> Arc<ConcurrencyLimiter> {
    Arc::new(ConcurrencyLimiter::new(&ConcurrencyConfig {
        initial_limit: 10,
        min_limit: 1,
        max_limit: 100,
        strategy: plexspaces_proto::outbox::v1::ConcurrencyStrategy::ConcurrencyStrategyAimd
            as i32,
    }))
}

/// Dispatch messages to target actors with AIMD concurrency control.
///
/// Uses the core `ConcurrencyLimiter` from plexspaces-outbox for adaptive rate control.
/// On RESOURCE_EXHAUSTED: halves the concurrency limit and retries with exponential backoff.
/// On success: increments the limit by 1 (additive increase).
pub async fn outbox_dispatch_impl(
    host_functions: &HostFunctions,
    limiter: &ConcurrencyLimiter,
    retry_policy: &plexspaces_proto::common::v1::RetryPolicy,
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
        let delivered = dispatch_single(
            host_functions,
            limiter,
            retry_policy,
            actor_id,
            target_id,
            msg_type,
            payload,
            idempotency_key,
            conflict_resolution_i32,
        )
        .await;
        if delivered {
            accepted += 1;
        }
    }

    let rejected = total - accepted;
    metrics::counter!("plexspaces_wasm_outbox_dispatch_success_total").increment(accepted);

    OUTBOX_DISPATCHED.fetch_add(total, Ordering::Relaxed);
    OUTBOX_ACCEPTED.fetch_add(accepted, Ordering::Relaxed);
    OUTBOX_REJECTED.fetch_add(rejected, Ordering::Relaxed);

    Ok(format!(
        "{{\"accepted\":{},\"rejected\":{},\"aimd_limit\":{}}}",
        accepted,
        rejected,
        limiter.current_limit()
    ))
}

async fn dispatch_single(
    host_functions: &HostFunctions,
    limiter: &ConcurrencyLimiter,
    retry_policy: &plexspaces_proto::common::v1::RetryPolicy,
    actor_id: &str,
    target_id: &str,
    msg_type: &str,
    payload: &[u8],
    idempotency_key: &str,
    conflict_resolution_i32: i32,
) -> bool {
    let max_retries = if retry_policy.max_attempts > 0 {
        retry_policy.max_attempts
    } else {
        DEFAULT_MAX_RETRIES
    };

    for attempt in 0..=max_retries {
        let permit = match limiter.try_acquire() {
            Some(p) => p,
            None => {
                // At AIMD capacity — back off and retry
                if attempt < max_retries {
                    let backoff = {
                        let mut rng = rand::thread_rng();
                        compute_backoff(retry_policy, attempt, &mut rng)
                    };
                    metrics::counter!("plexspaces_wasm_outbox_limiter_wait_total").increment(1);
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                return false;
            }
        };

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
            Ok(()) => {
                limiter.on_success(permit);
                return true;
            }
            Err(e) => {
                if e.contains(RESOURCE_EXHAUSTED) {
                    limiter.on_rejection(permit);
                    metrics::counter!("plexspaces_wasm_outbox_backpressure_total").increment(1);
                    if attempt < max_retries {
                        let backoff = {
                            let mut rng = rand::thread_rng();
                            compute_backoff(retry_policy, attempt, &mut rng)
                        };
                        metrics::histogram!("plexspaces_wasm_outbox_backoff_seconds")
                            .record(backoff.as_secs_f64());
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                } else {
                    // Non-capacity errors: don't reduce AIMD limit (not a congestion signal)
                    limiter.on_success(permit);
                    tracing::warn!(
                        target_id = %target_id,
                        error = %e,
                        "outbox_dispatch: delivery failed"
                    );
                }
                if attempt == max_retries {
                    return false;
                }
            }
        }
    }
    false
}

/// Return real outbox stats as JSON, keyed by `dispatcher_id`.
///
/// Reads node-wide atomic counters that `outbox_dispatch_impl` increments.
/// These counters are cumulative since node startup (monotonic).
pub fn outbox_status_impl(dispatcher_id: &str, limiter: &ConcurrencyLimiter) -> String {
    metrics::counter!("plexspaces_wasm_outbox_status_total").increment(1);

    let dispatched = OUTBOX_DISPATCHED.load(Ordering::Relaxed);
    let accepted = OUTBOX_ACCEPTED.load(Ordering::Relaxed);
    let rejected = OUTBOX_REJECTED.load(Ordering::Relaxed);

    let escaped_id = dispatcher_id.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "{{\"dispatcher_id\":\"{}\",\"dispatched\":{},\"accepted\":{},\"rejected\":{},\"aimd_limit\":{}}}",
        escaped_id, dispatched, accepted, rejected, limiter.current_limit()
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
        let limiter = default_wasm_limiter();
        let status = outbox_status_impl("test-dispatcher", &limiter);
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], "test-dispatcher");
        assert!(parsed["dispatched"].is_number());
        assert!(parsed["accepted"].is_number());
        assert!(parsed["rejected"].is_number());
        assert!(parsed["aimd_limit"].is_number());
    }

    #[test]
    fn outbox_status_escapes_json_injection() {
        let limiter = default_wasm_limiter();
        let status = outbox_status_impl("test\"evil\\id", &limiter);
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], "test\"evil\\id");
    }

    #[test]
    fn outbox_status_escapes_nested_json() {
        let limiter = default_wasm_limiter();
        let status = outbox_status_impl(r#"a","hacked":"true"#, &limiter);
        let parsed: serde_json::Value = serde_json::from_str(&status).unwrap();
        assert_eq!(parsed["dispatcher_id"], r#"a","hacked":"true"#);
        assert!(parsed.get("hacked").is_none());
    }

    #[test]
    fn outbox_counters_are_monotonic() {
        let limiter = default_wasm_limiter();
        let s1 = outbox_status_impl("c1", &limiter);
        let p1: serde_json::Value = serde_json::from_str(&s1).unwrap();
        let d1 = p1["dispatched"].as_u64().unwrap();

        OUTBOX_DISPATCHED.fetch_add(5, Ordering::Relaxed);
        OUTBOX_ACCEPTED.fetch_add(3, Ordering::Relaxed);
        OUTBOX_REJECTED.fetch_add(2, Ordering::Relaxed);

        let s2 = outbox_status_impl("c2", &limiter);
        let p2: serde_json::Value = serde_json::from_str(&s2).unwrap();
        assert_eq!(p2["dispatched"].as_u64().unwrap(), d1 + 5);
        assert!(p2["accepted"].as_u64().unwrap() >= 3);
        assert!(p2["rejected"].as_u64().unwrap() >= 2);
    }

    #[test]
    fn default_limiter_has_aimd_config() {
        let limiter = default_wasm_limiter();
        assert_eq!(limiter.current_limit(), 10);
    }
}
