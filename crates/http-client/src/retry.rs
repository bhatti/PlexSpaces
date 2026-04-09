// SPDX-License-Identifier: LGPL-2.1-or-later

//! Pure retry backoff (full jitter) and idempotency helpers — unit-tested without timers.

use plexspaces_proto::node::v1::HttpRetryPolicy;
use rand::Rng;
use std::time::Duration;

/// HTTP methods safe to retry by default without `allow_non_idempotent_retry`.
pub fn is_idempotent_method(method: &str) -> bool {
    matches!(
        method.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "OPTIONS" | "TRACE"
    )
}

/// Whether this attempt may be retried for the given method and policy.
pub fn method_allows_retry(method: &str, policy: &HttpRetryPolicy) -> bool {
    if is_idempotent_method(method) {
        return true;
    }
    policy.allow_non_idempotent_retry
}

/// HTTP status codes that are reasonable to retry (server/transient).
pub fn status_is_retriable(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// Caps for exponential backoff: `min(cap, initial * mult^(attempt-1))` then full jitter in [0, cap].
pub fn backoff_duration_for_attempt(
    policy: &HttpRetryPolicy,
    attempt_after_failure: u32,
    rng: &mut impl Rng,
) -> Duration {
    let initial_ms = policy
        .initial_backoff
        .as_ref()
        .map(|d| duration_proto_to_ms(d).max(1))
        .unwrap_or(100);
    let max_ms = policy
        .max_backoff
        .as_ref()
        .map(|d| duration_proto_to_ms(d).max(initial_ms))
        .unwrap_or(30_000);
    let mult = policy.backoff_multiplier;
    let mult = if mult.is_finite() && mult >= 1.0 {
        mult
    } else {
        2.0
    };

    let exp = (attempt_after_failure as f64).max(1.0);
    let raw = (initial_ms as f64) * mult.powf(exp - 1.0);
    let cap = (raw as u64).min(max_ms).max(1);

    let jitter = policy.jitter_ratio.clamp(0.0, 1.0);
    if jitter <= 0.0 {
        return Duration::from_millis(cap);
    }
    let top = cap.max(1);
    let sleep_ms = rng.gen_range(0..=top);
    Duration::from_millis(sleep_ms.max(1))
}

fn duration_proto_to_ms(d: &prost_types::Duration) -> u64 {
    let s = d.seconds.max(0) as u64;
    let ns = d.nanos.max(0) as u64;
    s.saturating_mul(1000).saturating_add(ns / 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Duration as ProtoDuration;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn idempotent_methods() {
        assert!(is_idempotent_method("get"));
        assert!(is_idempotent_method("HEAD"));
        assert!(!is_idempotent_method("POST"));
    }

    #[test]
    fn method_allows_retry_respects_flag() {
        let mut p = HttpRetryPolicy {
            max_attempts: 3,
            initial_backoff: None,
            max_backoff: None,
            backoff_multiplier: 0.0,
            jitter_ratio: 0.0,
            allow_non_idempotent_retry: false,
        };
        assert!(!method_allows_retry("POST", &p));
        p.allow_non_idempotent_retry = true;
        assert!(method_allows_retry("POST", &p));
    }

    #[test]
    fn backoff_bounded_with_full_jitter() {
        let policy = HttpRetryPolicy {
            max_attempts: 5,
            initial_backoff: Some(ProtoDuration {
                seconds: 0,
                nanos: 10_000_000,
            }),
            max_backoff: Some(ProtoDuration {
                seconds: 1,
                nanos: 0,
            }),
            backoff_multiplier: 2.0,
            jitter_ratio: 1.0,
            allow_non_idempotent_retry: false,
        };
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..20 {
            let d = backoff_duration_for_attempt(&policy, 3, &mut rng);
            assert!(d <= Duration::from_secs(1));
            assert!(d >= Duration::from_millis(1));
        }
    }

    #[test]
    fn retriable_statuses() {
        assert!(status_is_retriable(503));
        assert!(!status_is_retriable(404));
        assert!(status_is_retriable(429));
    }
}
