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

//! Outbox dispatch helpers: parsing, building, and validating outbox messages.
//!
//! ## Purpose
//! Pure functions for working with outbox proto types (`DispatchRequest`,
//! `DispatchResponse`, `DispatcherStatus`, `OutboxConfig`). Used by the
//! WASM host bridge and `OutboxRelay` infrastructure.

use plexspaces_proto::outbox::v1::{
    DispatchRequest, DispatchResponse, DispatcherStatus, OutboxConfig,
};
use std::sync::atomic::Ordering;

use crate::concurrency_limiter::ConcurrencyLimiter;
use crate::outbox_relay::RelayStats;

/// Parse a DispatchRequest from a message payload.
pub fn parse_dispatch_request(payload: &[u8]) -> Result<DispatchRequest, String> {
    use prost::Message;
    DispatchRequest::decode(payload).map_err(|e| format!("invalid DispatchRequest: {e}"))
}

/// Build a DispatchResponse.
pub fn build_dispatch_response(
    accepted: u32,
    rejected: u32,
    dispatcher_id: &str,
) -> DispatchResponse {
    DispatchResponse {
        accepted,
        rejected,
        dispatcher_id: dispatcher_id.to_string(),
    }
}

/// Build a DispatcherStatus from relay stats and limiter.
pub fn build_dispatcher_status(
    dispatcher_id: &str,
    stats: &RelayStats,
    limiter: &ConcurrencyLimiter,
) -> DispatcherStatus {
    let delivered = stats.delivered.load(Ordering::Relaxed);
    let failed = stats.failed.load(Ordering::Relaxed);
    let total = delivered + failed;
    let success_rate = if total > 0 {
        delivered as f64 / total as f64
    } else {
        1.0
    };

    DispatcherStatus {
        dispatcher_id: dispatcher_id.to_string(),
        pending_count: stats.pending.load(Ordering::Relaxed),
        in_flight_count: limiter.in_flight() as u64,
        delivered_count: delivered,
        failed_count: failed,
        dead_letter_count: stats.dead_lettered.load(Ordering::Relaxed),
        current_concurrency_limit: limiter.current_limit(),
        success_rate,
    }
}

/// Validate an OutboxConfig, returning an error string if invalid.
pub fn validate_config(config: &OutboxConfig) -> Result<(), String> {
    if let Some(ref cc) = config.concurrency {
        if cc.min_limit > cc.max_limit && cc.max_limit > 0 {
            return Err("concurrency.min_limit > max_limit".to_string());
        }
    }
    if config.relay_batch_size > 10_000 {
        return Err("relay_batch_size exceeds 10,000".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::outbox::v1::ConcurrencyConfig;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn test_build_dispatch_response() {
        let resp = build_dispatch_response(10, 2, "disp-1");
        assert_eq!(resp.accepted, 10);
        assert_eq!(resp.rejected, 2);
        assert_eq!(resp.dispatcher_id, "disp-1");
    }

    #[test]
    fn test_build_dispatcher_status() {
        let stats = RelayStats {
            delivered: AtomicU64::new(90),
            failed: AtomicU64::new(10),
            dead_lettered: AtomicU64::new(5),
            pending: AtomicU64::new(20),
        };
        let limiter = ConcurrencyLimiter::new(&ConcurrencyConfig {
            initial_limit: 50,
            min_limit: 1,
            max_limit: 100,
            strategy: 0,
        });

        let status = build_dispatcher_status("disp-1", &stats, &limiter);
        assert_eq!(status.dispatcher_id, "disp-1");
        assert_eq!(status.delivered_count, 90);
        assert_eq!(status.failed_count, 10);
        assert_eq!(status.dead_letter_count, 5);
        assert_eq!(status.pending_count, 20);
        assert_eq!(status.current_concurrency_limit, 50);
        assert!((status.success_rate - 0.9).abs() < 0.001);
    }

    #[test]
    fn test_build_dispatcher_status_zero_total() {
        let stats = RelayStats::default();
        let limiter = ConcurrencyLimiter::with_defaults();
        let status = build_dispatcher_status("disp-1", &stats, &limiter);
        assert_eq!(status.success_rate, 1.0);
    }

    #[test]
    fn test_validate_config_ok() {
        let config = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 100,
                strategy: 0,
            }),
            relay_batch_size: 100,
            ..Default::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_min_gt_max() {
        let config = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 200,
                max_limit: 100,
                strategy: 0,
            }),
            ..Default::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_validate_config_batch_too_large() {
        let config = OutboxConfig {
            relay_batch_size: 20_000,
            ..Default::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn test_parse_dispatch_request() {
        use prost::Message;
        let req = DispatchRequest {
            target_actor_ids: vec!["actor-1".to_string()],
            payload: b"hello".to_vec(),
            idempotency_key: "key-1".to_string(),
            priority: 5,
            headers: Default::default(),
            message_type: "cast".to_string(),
        };
        let bytes = req.encode_to_vec();
        let parsed = parse_dispatch_request(&bytes).unwrap();
        assert_eq!(parsed.target_actor_ids, vec!["actor-1"]);
        assert_eq!(parsed.payload, b"hello");
        assert_eq!(parsed.priority, 5);
    }

    #[test]
    fn test_parse_dispatch_request_invalid() {
        assert!(parse_dispatch_request(b"garbage").is_err());
    }
}
