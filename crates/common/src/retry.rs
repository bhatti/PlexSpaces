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

//! Shared retry/backoff utilities for the canonical `RetryPolicy` proto type.
//!
//! ## Purpose
//! Single implementation of exponential backoff with full jitter, used by all
//! PlexSpaces crates (http-client, workflow, grpc-middleware, outbox, etc.).
//!
//! ## Architecture Context
//! `RetryPolicy` is defined in `common.proto` and replaces 6 prior duplicate
//! retry types. This module provides the Rust-side computation that all
//! consumers share.

use plexspaces_proto::common::v1::RetryPolicy;
use rand::Rng;
use std::time::Duration;

const DEFAULT_INITIAL_DELAY_MS: u64 = 100;
const DEFAULT_MAX_DELAY_MS: u64 = 30_000;
const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;

/// Convert a proto Duration to milliseconds.
pub fn proto_duration_to_ms(d: &prost_types::Duration) -> u64 {
    let s = d.seconds.max(0) as u64;
    let ns = d.nanos.max(0) as u64;
    s.saturating_mul(1000).saturating_add(ns / 1_000_000)
}

/// Create a proto Duration from milliseconds.
pub fn ms_to_proto_duration(ms: u64) -> prost_types::Duration {
    prost_types::Duration {
        seconds: (ms / 1000) as i64,
        nanos: ((ms % 1000) * 1_000_000) as i32,
    }
}

/// Compute exponential backoff with full jitter for the given attempt.
///
/// Formula: `cap = min(initial * mult^(attempt-1), max_delay)`, then
/// `sleep = random(0, cap)` when `jitter_ratio > 0`, else `sleep = cap`.
///
/// `attempt` is 1-indexed (first retry = 1).
pub fn compute_backoff(policy: &RetryPolicy, attempt: u32, rng: &mut impl Rng) -> Duration {
    let initial_ms = policy
        .initial_delay
        .as_ref()
        .map(|d| proto_duration_to_ms(d).max(1))
        .unwrap_or(DEFAULT_INITIAL_DELAY_MS);

    let max_ms = policy
        .max_delay
        .as_ref()
        .map(|d| proto_duration_to_ms(d).max(initial_ms))
        .unwrap_or(DEFAULT_MAX_DELAY_MS);

    let mult = if policy.backoff_multiplier.is_finite() && policy.backoff_multiplier >= 1.0 {
        policy.backoff_multiplier
    } else {
        DEFAULT_BACKOFF_MULTIPLIER
    };

    let exp = (attempt as f64).max(1.0);
    let raw = (initial_ms as f64) * mult.powf(exp - 1.0);
    let cap = (raw as u64).min(max_ms).max(1);

    let jitter = policy.jitter_ratio.clamp(0.0, 1.0);
    if jitter <= 0.0 {
        return Duration::from_millis(cap);
    }

    let jitter_window = ((cap as f64) * jitter) as u64;
    let base = cap.saturating_sub(jitter_window);
    let sleep_ms = if jitter_window > 0 {
        base + rng.gen_range(0..=jitter_window)
    } else {
        cap
    };
    Duration::from_millis(sleep_ms.max(1))
}

/// Compute exponential backoff without jitter (deterministic).
pub fn compute_backoff_no_jitter(policy: &RetryPolicy, attempt: u32) -> Duration {
    let initial_ms = policy
        .initial_delay
        .as_ref()
        .map(|d| proto_duration_to_ms(d).max(1))
        .unwrap_or(DEFAULT_INITIAL_DELAY_MS);

    let max_ms = policy
        .max_delay
        .as_ref()
        .map(|d| proto_duration_to_ms(d).max(initial_ms))
        .unwrap_or(DEFAULT_MAX_DELAY_MS);

    let mult = if policy.backoff_multiplier.is_finite() && policy.backoff_multiplier >= 1.0 {
        policy.backoff_multiplier
    } else {
        DEFAULT_BACKOFF_MULTIPLIER
    };

    let exp = (attempt as f64).max(1.0);
    let raw = (initial_ms as f64) * mult.powf(exp - 1.0);
    let cap = (raw as u64).min(max_ms).max(1);
    Duration::from_millis(cap)
}

/// Check whether a retry should be attempted for the given attempt and error.
///
/// Returns `true` if:
/// - `attempt` is less than `policy.max_attempts` (0/1 = no retry)
/// - `error` matches `retryable_errors` (empty list = retry all)
pub fn should_retry(policy: &RetryPolicy, attempt: u32, error: &str) -> bool {
    let max = if policy.max_attempts == 0 { 1 } else { policy.max_attempts };
    if attempt >= max {
        return false;
    }
    if policy.retryable_errors.is_empty() {
        return true;
    }
    policy.retryable_errors.iter().any(|e| {
        e.eq_ignore_ascii_case(error)
    })
}

/// Build a default RetryPolicy with sensible production defaults.
pub fn default_retry_policy() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 3,
        backoff_multiplier: DEFAULT_BACKOFF_MULTIPLIER,
        initial_delay: Some(ms_to_proto_duration(DEFAULT_INITIAL_DELAY_MS)),
        max_delay: Some(ms_to_proto_duration(DEFAULT_MAX_DELAY_MS)),
        jitter_ratio: 0.5,
        retryable_errors: vec![],
        allow_non_idempotent: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Duration as ProtoDuration;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn policy_100ms_30s() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 5,
            backoff_multiplier: 2.0,
            initial_delay: Some(ProtoDuration { seconds: 0, nanos: 100_000_000 }),
            max_delay: Some(ProtoDuration { seconds: 30, nanos: 0 }),
            jitter_ratio: 0.0,
            retryable_errors: vec![],
            allow_non_idempotent: false,
        }
    }

    #[test]
    fn test_compute_backoff_no_jitter_exponential() {
        let policy = policy_100ms_30s();
        let mut rng = StdRng::seed_from_u64(42);
        assert_eq!(compute_backoff(&policy, 1, &mut rng).as_millis(), 100);
        assert_eq!(compute_backoff(&policy, 2, &mut rng).as_millis(), 200);
        assert_eq!(compute_backoff(&policy, 3, &mut rng).as_millis(), 400);
        assert_eq!(compute_backoff(&policy, 4, &mut rng).as_millis(), 800);
    }

    #[test]
    fn test_compute_backoff_caps_at_max() {
        let policy = RetryPolicy {
            max_attempts: 10,
            backoff_multiplier: 2.0,
            initial_delay: Some(ProtoDuration { seconds: 1, nanos: 0 }),
            max_delay: Some(ProtoDuration { seconds: 5, nanos: 0 }),
            jitter_ratio: 0.0,
            ..Default::default()
        };
        let mut rng = StdRng::seed_from_u64(42);
        let d = compute_backoff(&policy, 10, &mut rng);
        assert!(d.as_millis() <= 5000);
    }

    #[test]
    fn test_compute_backoff_with_jitter_bounded() {
        let mut policy = policy_100ms_30s();
        policy.jitter_ratio = 1.0;
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..50 {
            let d = compute_backoff(&policy, 3, &mut rng);
            assert!(d.as_millis() <= 400);
            assert!(d.as_millis() >= 1);
        }
    }

    #[test]
    fn test_compute_backoff_no_jitter_fn() {
        let policy = policy_100ms_30s();
        assert_eq!(compute_backoff_no_jitter(&policy, 1).as_millis(), 100);
        assert_eq!(compute_backoff_no_jitter(&policy, 2).as_millis(), 200);
        assert_eq!(compute_backoff_no_jitter(&policy, 3).as_millis(), 400);
    }

    #[test]
    fn test_compute_backoff_defaults_when_unset() {
        let policy = RetryPolicy::default();
        let mut rng = StdRng::seed_from_u64(42);
        let d = compute_backoff(&policy, 1, &mut rng);
        assert_eq!(d.as_millis(), 100);
    }

    #[test]
    fn test_should_retry_within_max() {
        let policy = RetryPolicy {
            max_attempts: 3,
            ..Default::default()
        };
        assert!(should_retry(&policy, 1, "UNAVAILABLE"));
        assert!(should_retry(&policy, 2, "UNAVAILABLE"));
        assert!(!should_retry(&policy, 3, "UNAVAILABLE"));
    }

    #[test]
    fn test_should_retry_zero_means_no_retry() {
        let policy = RetryPolicy {
            max_attempts: 0,
            ..Default::default()
        };
        assert!(!should_retry(&policy, 1, "any"));
    }

    #[test]
    fn test_should_retry_one_means_no_retry() {
        let policy = RetryPolicy {
            max_attempts: 1,
            ..Default::default()
        };
        assert!(!should_retry(&policy, 1, "any"));
    }

    #[test]
    fn test_should_retry_filters_errors() {
        let policy = RetryPolicy {
            max_attempts: 5,
            retryable_errors: vec!["UNAVAILABLE".to_string(), "429".to_string()],
            ..Default::default()
        };
        assert!(should_retry(&policy, 1, "UNAVAILABLE"));
        assert!(should_retry(&policy, 1, "unavailable"));
        assert!(should_retry(&policy, 1, "429"));
        assert!(!should_retry(&policy, 1, "NOT_FOUND"));
    }

    #[test]
    fn test_should_retry_empty_errors_retries_all() {
        let policy = RetryPolicy {
            max_attempts: 5,
            retryable_errors: vec![],
            ..Default::default()
        };
        assert!(should_retry(&policy, 1, "anything"));
    }

    #[test]
    fn test_proto_duration_conversion_roundtrip() {
        let ms = 1500u64;
        let d = ms_to_proto_duration(ms);
        assert_eq!(d.seconds, 1);
        assert_eq!(d.nanos, 500_000_000);
        assert_eq!(proto_duration_to_ms(&d), ms);
    }

    #[test]
    fn test_default_retry_policy() {
        let p = default_retry_policy();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.backoff_multiplier, 2.0);
        assert_eq!(p.jitter_ratio, 0.5);
        assert!(!p.allow_non_idempotent);
    }

    #[test]
    fn test_partial_jitter_scales_window() {
        let mut policy = policy_100ms_30s();
        policy.jitter_ratio = 0.3;
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let d = compute_backoff(&policy, 3, &mut rng);
            // cap=400, jitter_window=120, base=280, range=[280, 400]
            assert!(d.as_millis() >= 280, "got {}", d.as_millis());
            assert!(d.as_millis() <= 400, "got {}", d.as_millis());
        }
    }

    #[test]
    fn test_attempt_zero_treated_as_one() {
        let policy = policy_100ms_30s();
        let mut rng = StdRng::seed_from_u64(42);
        let d = compute_backoff(&policy, 0, &mut rng);
        assert_eq!(d.as_millis(), 100);
    }

    #[test]
    fn test_compute_backoff_no_jitter_caps_at_max() {
        let policy = RetryPolicy {
            max_attempts: 20,
            backoff_multiplier: 2.0,
            initial_delay: Some(ProtoDuration { seconds: 1, nanos: 0 }),
            max_delay: Some(ProtoDuration { seconds: 5, nanos: 0 }),
            ..Default::default()
        };
        assert_eq!(compute_backoff_no_jitter(&policy, 15).as_millis(), 5000);
    }

    #[test]
    fn test_compute_backoff_no_jitter_defaults() {
        let policy = RetryPolicy::default();
        let d = compute_backoff_no_jitter(&policy, 1);
        assert_eq!(d.as_millis(), 100);
    }
}
