// SPDX-License-Identifier: LGPL-2.1-or-later
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

//! Stateless helpers for parallel / collective shard-group operations.
//!
//! These functions are pure: no service state, no gRPC, no async. They operate
//! on proto types and JSON values so that the actor-service (and any other
//! orchestrator) can remain a thin controller.

use plexspaces_proto::actor::v1::{
    CollectiveReduction, CollectiveTargetField, DataParallelConfig, ScatterGatherStats, ShardGroup,
    ShardQueryResponse,
};
use plexspaces_proto::common::v1::Message;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use ulid::Ulid;

/// Default timeout in seconds for shard/parallel operations.
/// Used as a fallback when the request does not specify a timeout.
pub const DEFAULT_SHARD_TIMEOUT_SECS: u64 = 30;

/// Return the default shard operation timeout as a `Duration`.
pub fn default_shard_timeout() -> Duration {
    Duration::from_secs(DEFAULT_SHARD_TIMEOUT_SECS)
}

/// Convert an optional proto `prost_types::Duration` to a `std::time::Duration`,
/// falling back to [`default_shard_timeout`] when `None`.
pub fn resolve_timeout(proto: Option<&prost_types::Duration>) -> Duration {
    proto
        .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
        .unwrap_or_else(default_shard_timeout)
}

/// Result of a single shard in a parallel operation.
/// Fields: (shard_id, actor_id, latency, success, error, response).
pub type ParallelShardResult = (u32, String, Duration, bool, String, Option<Message>);

/// Extract the `DataParallelConfig` from a `ShardGroup`, panicking if absent.
pub fn shard_group_config(group: &ShardGroup) -> &DataParallelConfig {
    group.config.as_ref().expect("ShardGroup.config required")
}

/// Build a proto `Message` for collective operations.
pub fn build_collective_message(
    message_type: &str,
    payload: Vec<u8>,
    headers: HashMap<String, String>,
) -> Message {
    Message {
        id: format!("collective-{}", Ulid::new()),
        sender_id: "collective".to_string(),
        receiver_id: String::new(),
        channel: String::new(),
        message_type: message_type.to_string(),
        payload,
        timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
        headers,
        priority: 0,
        ttl: None,
        delivery_count: 0,
        idempotency_key: String::new(),
        correlation_id: String::new(),
        reply_to: String::new(),
        partition_key: String::new(),
        uri_path: String::new(),
        uri_method: String::new(),
    }
}

/// Compute `ScatterGatherStats` from parallel shard results.
pub fn scatter_stats_from_results(
    shard_count: u32,
    results: &[ParallelShardResult],
) -> ScatterGatherStats {
    let shards_responded = results.iter().filter(|item| item.3).count() as u32;
    let shards_failed = results.len() as u32 - shards_responded;
    let max_latency = results
        .iter()
        .map(|item| item.2)
        .max()
        .unwrap_or(Duration::ZERO);
    ScatterGatherStats {
        shards_queried: shard_count,
        shards_responded,
        shards_failed,
        max_latency: Some(prost_types::Duration {
            seconds: max_latency.as_secs() as i64,
            nanos: max_latency.subsec_nanos() as i32,
        }),
    }
}

/// Convert raw parallel shard results into `ShardQueryResponse` entries.
pub fn shard_query_responses_from_results(
    results: Vec<ParallelShardResult>,
) -> Vec<ShardQueryResponse> {
    results
        .into_iter()
        .map(
            |(shard_id, shard_actor_id, latency, success, error, response)| ShardQueryResponse {
                shard_id,
                shard_actor_id,
                response,
                latency: Some(prost_types::Duration {
                    seconds: latency.as_secs() as i64,
                    nanos: latency.subsec_nanos() as i32,
                }),
                success,
                error,
            },
        )
        .collect()
}

/// Extract a JSON value from a message payload, optionally drilling into a
/// dot-separated path specified by `CollectiveTargetField`.
pub fn select_collective_value(
    response: &Message,
    target: Option<&CollectiveTargetField>,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let value: serde_json::Value = serde_json::from_slice(&response.payload)?;
    let Some(target) = target else {
        return Ok(value);
    };
    if target.value_path.is_empty() {
        return Ok(value);
    }
    let mut current = &value;
    for segment in target.value_path.split('.') {
        current = current.get(segment).ok_or_else(|| {
            format!(
                "Collective target path '{}' not found",
                target.value_path
            )
        })?;
    }
    Ok(current.clone())
}

/// Apply a built-in collective reduction over a vector of JSON values.
///
/// The `reduction` parameter is the i32 wire value of `CollectiveReduction`.
pub fn reduce_values(
    values: Vec<serde_json::Value>,
    reduction: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    if values.is_empty() {
        return Err("No values available for reduction".into());
    }
    let reduction = CollectiveReduction::try_from(reduction)
        .unwrap_or(CollectiveReduction::CollectiveReductionUnspecified);
    match reduction {
        CollectiveReduction::CollectiveReductionSum => {
            let mut sum = 0.0f64;
            for value in values {
                sum += value
                    .as_f64()
                    .ok_or("SUM reduction requires numeric values")?;
            }
            Ok(serde_json::Value::from(sum))
        }
        CollectiveReduction::CollectiveReductionMin => {
            let mut current = values[0]
                .as_f64()
                .ok_or("MIN reduction requires numeric values")?;
            for value in values.into_iter().skip(1) {
                let next = value
                    .as_f64()
                    .ok_or("MIN reduction requires numeric values")?;
                if next < current {
                    current = next;
                }
            }
            Ok(serde_json::Value::from(current))
        }
        CollectiveReduction::CollectiveReductionMax => {
            let mut current = values[0]
                .as_f64()
                .ok_or("MAX reduction requires numeric values")?;
            for value in values.into_iter().skip(1) {
                let next = value
                    .as_f64()
                    .ok_or("MAX reduction requires numeric values")?;
                if next > current {
                    current = next;
                }
            }
            Ok(serde_json::Value::from(current))
        }
        CollectiveReduction::CollectiveReductionProduct => {
            let mut product = 1.0f64;
            for value in values {
                product *= value
                    .as_f64()
                    .ok_or("PRODUCT reduction requires numeric values")?;
            }
            Ok(serde_json::Value::from(product))
        }
        CollectiveReduction::CollectiveReductionConcat => {
            let mut merged = Vec::new();
            for value in values {
                match value {
                    serde_json::Value::Array(items) => merged.extend(items),
                    other => merged.push(other),
                }
            }
            Ok(serde_json::Value::Array(merged))
        }
        CollectiveReduction::CollectiveReductionBoolAnd => {
            let mut all_true = true;
            for value in values {
                all_true &= value
                    .as_bool()
                    .ok_or("BOOL_AND reduction requires boolean values")?;
            }
            Ok(serde_json::Value::from(all_true))
        }
        CollectiveReduction::CollectiveReductionBoolOr => {
            let mut any_true = false;
            for value in values {
                any_true |= value
                    .as_bool()
                    .ok_or("BOOL_OR reduction requires boolean values")?;
            }
            Ok(serde_json::Value::from(any_true))
        }
        CollectiveReduction::CollectiveReductionUnspecified => {
            Err("Collective reduction must be specified".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::actor::v1::CollectiveReduction;
    use serde_json::json;

    // --- reduce_values ---

    #[test]
    fn reduce_sum() {
        let values = vec![json!(1.0), json!(2.0), json!(3.0)];
        let result = reduce_values(values, CollectiveReduction::CollectiveReductionSum as i32)
            .unwrap();
        assert_eq!(result.as_f64().unwrap(), 6.0);
    }

    #[test]
    fn reduce_product() {
        let values = vec![json!(2.0), json!(3.0), json!(4.0)];
        let result =
            reduce_values(values, CollectiveReduction::CollectiveReductionProduct as i32).unwrap();
        assert_eq!(result.as_f64().unwrap(), 24.0);
    }

    #[test]
    fn reduce_min() {
        let values = vec![json!(5.0), json!(2.0), json!(8.0)];
        let result = reduce_values(values, CollectiveReduction::CollectiveReductionMin as i32)
            .unwrap();
        assert_eq!(result.as_f64().unwrap(), 2.0);
    }

    #[test]
    fn reduce_max() {
        let values = vec![json!(5.0), json!(2.0), json!(8.0)];
        let result = reduce_values(values, CollectiveReduction::CollectiveReductionMax as i32)
            .unwrap();
        assert_eq!(result.as_f64().unwrap(), 8.0);
    }

    #[test]
    fn reduce_concat_arrays() {
        let values = vec![json!([1, 2]), json!([3, 4])];
        let result =
            reduce_values(values, CollectiveReduction::CollectiveReductionConcat as i32).unwrap();
        assert_eq!(result, json!([1, 2, 3, 4]));
    }

    #[test]
    fn reduce_concat_scalars() {
        let values = vec![json!("a"), json!("b")];
        let result =
            reduce_values(values, CollectiveReduction::CollectiveReductionConcat as i32).unwrap();
        assert_eq!(result, json!(["a", "b"]));
    }

    #[test]
    fn reduce_bool_and() {
        let values = vec![json!(true), json!(true), json!(false)];
        let result =
            reduce_values(values, CollectiveReduction::CollectiveReductionBoolAnd as i32).unwrap();
        assert_eq!(result, json!(false));
    }

    #[test]
    fn reduce_bool_or() {
        let values = vec![json!(false), json!(false), json!(true)];
        let result =
            reduce_values(values, CollectiveReduction::CollectiveReductionBoolOr as i32).unwrap();
        assert_eq!(result, json!(true));
    }

    #[test]
    fn reduce_empty_values_errors() {
        let result = reduce_values(vec![], CollectiveReduction::CollectiveReductionSum as i32);
        assert!(result.is_err());
    }

    #[test]
    fn reduce_unspecified_errors() {
        let values = vec![json!(1.0)];
        let result = reduce_values(
            values,
            CollectiveReduction::CollectiveReductionUnspecified as i32,
        );
        assert!(result.is_err());
    }

    #[test]
    fn reduce_sum_non_numeric_errors() {
        let values = vec![json!("not a number")];
        let result = reduce_values(values, CollectiveReduction::CollectiveReductionSum as i32);
        assert!(result.is_err());
    }

    // --- select_collective_value ---

    #[test]
    fn select_value_no_target() {
        let msg = Message {
            payload: serde_json::to_vec(&json!({"a": 1})).unwrap(),
            ..Default::default()
        };
        let result = select_collective_value(&msg, None).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn select_value_empty_path() {
        let msg = Message {
            payload: serde_json::to_vec(&json!({"a": 1})).unwrap(),
            ..Default::default()
        };
        let target = CollectiveTargetField {
            value_path: String::new(),
        };
        let result = select_collective_value(&msg, Some(&target)).unwrap();
        assert_eq!(result, json!({"a": 1}));
    }

    #[test]
    fn select_value_nested_path() {
        let msg = Message {
            payload: serde_json::to_vec(&json!({"outer": {"inner": 42}})).unwrap(),
            ..Default::default()
        };
        let target = CollectiveTargetField {
            value_path: "outer.inner".to_string(),
        };
        let result = select_collective_value(&msg, Some(&target)).unwrap();
        assert_eq!(result, json!(42));
    }

    #[test]
    fn select_value_missing_path_errors() {
        let msg = Message {
            payload: serde_json::to_vec(&json!({"a": 1})).unwrap(),
            ..Default::default()
        };
        let target = CollectiveTargetField {
            value_path: "b.c".to_string(),
        };
        let result = select_collective_value(&msg, Some(&target));
        assert!(result.is_err());
    }

    // --- build_collective_message ---

    #[test]
    fn build_message_has_correct_fields() {
        let headers = HashMap::from([("k".to_string(), "v".to_string())]);
        let msg = build_collective_message("info", b"payload".to_vec(), headers.clone());
        assert!(msg.id.starts_with("collective-"));
        assert_eq!(msg.sender_id, "collective");
        assert_eq!(msg.message_type, "info");
        assert_eq!(msg.payload, b"payload");
        assert_eq!(msg.headers, headers);
        assert!(msg.timestamp.is_some());
    }

    // --- scatter_stats_from_results ---

    #[test]
    fn stats_counts_successes_and_failures() {
        let results: Vec<ParallelShardResult> = vec![
            (0, "a".into(), Duration::from_millis(10), true, String::new(), None),
            (1, "b".into(), Duration::from_millis(20), false, "err".into(), None),
            (2, "c".into(), Duration::from_millis(5), true, String::new(), None),
        ];
        let stats = scatter_stats_from_results(3, &results);
        assert_eq!(stats.shards_queried, 3);
        assert_eq!(stats.shards_responded, 2);
        assert_eq!(stats.shards_failed, 1);
        let max = stats.max_latency.unwrap();
        assert_eq!(max.seconds, 0);
        assert_eq!(max.nanos, 20_000_000);
    }

    #[test]
    fn stats_empty_results() {
        let stats = scatter_stats_from_results(0, &[]);
        assert_eq!(stats.shards_responded, 0);
        assert_eq!(stats.shards_failed, 0);
    }

    // --- shard_query_responses_from_results ---

    #[test]
    fn converts_results_to_responses() {
        let results: Vec<ParallelShardResult> = vec![
            (0, "actor-0".into(), Duration::from_secs(1), true, String::new(), None),
            (1, "actor-1".into(), Duration::from_millis(500), false, "timeout".into(), None),
        ];
        let responses = shard_query_responses_from_results(results);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].shard_id, 0);
        assert!(responses[0].success);
        assert_eq!(responses[1].error, "timeout");
        assert!(!responses[1].success);
    }

    // --- shard_group_config ---

    #[test]
    fn extracts_config() {
        let group = ShardGroup {
            config: Some(DataParallelConfig {
                group_id: "g1".to_string(),
                shard_count: 4,
                ..Default::default()
            }),
            ..Default::default()
        };
        let config = shard_group_config(&group);
        assert_eq!(config.group_id, "g1");
        assert_eq!(config.shard_count, 4);
    }

    #[test]
    #[should_panic(expected = "ShardGroup.config required")]
    fn config_panics_when_missing() {
        let group = ShardGroup::default();
        shard_group_config(&group);
    }

    // --- default_shard_timeout / resolve_timeout ---

    #[test]
    fn default_shard_timeout_is_30s() {
        assert_eq!(default_shard_timeout(), Duration::from_secs(30));
        assert_eq!(DEFAULT_SHARD_TIMEOUT_SECS, 30);
    }

    #[test]
    fn resolve_timeout_uses_default_when_none() {
        let timeout = resolve_timeout(None);
        assert_eq!(timeout, Duration::from_secs(DEFAULT_SHARD_TIMEOUT_SECS));
    }

    #[test]
    fn resolve_timeout_uses_provided_duration() {
        let proto_dur = prost_types::Duration {
            seconds: 10,
            nanos: 500_000_000,
        };
        let timeout = resolve_timeout(Some(&proto_dur));
        assert_eq!(timeout, Duration::from_secs(10) + Duration::from_nanos(500_000_000));
    }
}
