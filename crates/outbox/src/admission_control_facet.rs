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

//! Receiver-side admission control facet.
//!
//! ## Purpose
//! Limits concurrent message processing on an actor to prevent overload.
//! When the actor is at capacity, incoming messages are rejected with
//! `RESOURCE_EXHAUSTED` and a `retry_after_ms` hint so the sender can back off.
//!
//! ## Architecture Context
//! Implements the `Facet` trait (decorator pattern). Attached to actors that
//! need receiver-side flow control, complementing the sender-side
//! `ConcurrencyLimiter` in the `OutboxRelay`.

use async_trait::async_trait;
use plexspaces_facet::{ErrorHandling, Facet, FacetError, FacetFactory, FacetMetadata, InterceptResult};
use plexspaces_proto::outbox::v1::{AdmissionControlConfig, AdmissionStrategy};
use serde_json::{json, Value};
use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

const DEFAULT_MAX_CONCURRENT: u32 = 100;
const DEFAULT_RETRY_AFTER_MS: u64 = 100;

/// Receiver-side admission control facet.
///
/// Tracks in-flight messages via atomic counter. `before_method` rejects
/// when at capacity; `after_method` decrements.
#[derive(Debug)]
pub struct AdmissionControlFacet {
    in_flight: AtomicU32,
    max_concurrent: u32,
    retry_after_ms: u64,
    strategy: AdmissionStrategy,
    actor_id: String,
}

impl AdmissionControlFacet {
    /// Create from proto config.
    pub fn new(config: &AdmissionControlConfig) -> Self {
        let max = if config.max_concurrent == 0 {
            DEFAULT_MAX_CONCURRENT
        } else {
            config.max_concurrent
        };
        let retry_ms = if config.default_retry_after_ms == 0 {
            DEFAULT_RETRY_AFTER_MS
        } else {
            config.default_retry_after_ms
        };
        let strategy = AdmissionStrategy::try_from(config.strategy)
            .unwrap_or(AdmissionStrategy::AdmissionStrategyFixedLimit);
        Self {
            in_flight: AtomicU32::new(0),
            max_concurrent: max,
            retry_after_ms: retry_ms,
            strategy,
            actor_id: String::new(),
        }
    }

    /// Create with default settings.
    pub fn with_defaults() -> Self {
        Self {
            in_flight: AtomicU32::new(0),
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            retry_after_ms: DEFAULT_RETRY_AFTER_MS,
            strategy: AdmissionStrategy::AdmissionStrategyFixedLimit,
            actor_id: String::new(),
        }
    }

    /// Current number of in-flight messages.
    pub fn in_flight(&self) -> u32 {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Configured max concurrent.
    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent
    }

    /// Configured retry-after hint in ms.
    pub fn retry_after_ms(&self) -> u64 {
        self.retry_after_ms
    }

    fn try_admit(&self) -> bool {
        loop {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.max_concurrent {
                return false;
            }
            if self
                .in_flight
                .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn release(&self) {
        self.in_flight.fetch_sub(1, Ordering::Release);
    }
}

#[async_trait]
impl Facet for AdmissionControlFacet {
    fn facet_type(&self) -> &str {
        "admission_control"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        self.actor_id = actor_id.to_string();
        tracing::info!(
            actor_id = actor_id,
            max_concurrent = self.max_concurrent,
            strategy = ?self.strategy,
            "AdmissionControlFacet attached"
        );
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        tracing::info!(
            actor_id = actor_id,
            in_flight = self.in_flight(),
            "AdmissionControlFacet detached"
        );
        Ok(())
    }

    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
        _headers: &HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        if self.try_admit() {
            metrics::counter!("plexspaces_outbox_admission_admitted_total").increment(1);
            metrics::gauge!("plexspaces_outbox_admission_in_flight").set(self.in_flight() as f64);
            Ok(InterceptResult::Continue)
        } else {
            metrics::counter!("plexspaces_outbox_admission_rejected_total").increment(1);
            let rejection = json!({
                "error": crate::RESOURCE_EXHAUSTED,
                "message": "actor at capacity",
                "retry_after_ms": self.retry_after_ms,
                "actor_id": self.actor_id,
                "current_in_flight": self.in_flight(),
                "max_concurrent": self.max_concurrent,
            });
            Ok(InterceptResult::ShortCircuit(
                serde_json::to_vec(&rejection).unwrap_or_default(),
            ))
        }
    }

    async fn after_method(
        &self,
        _method: &str,
        _args: &[u8],
        _result: &[u8],
        _headers: &HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        self.release();
        metrics::gauge!("plexspaces_outbox_admission_in_flight").set(self.in_flight() as f64);
        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        self.release();
        metrics::gauge!("plexspaces_outbox_admission_in_flight").set(self.in_flight() as f64);
        Ok(ErrorHandling::Propagate)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(json!({
            "in_flight": self.in_flight(),
            "max_concurrent": self.max_concurrent,
            "retry_after_ms": self.retry_after_ms,
        }))
    }

    fn get_config(&self) -> Value {
        json!({
            "max_concurrent": self.max_concurrent,
            "retry_after_ms": self.retry_after_ms,
            "strategy": format!("{:?}", self.strategy),
        })
    }

    fn get_priority(&self) -> i32 {
        950
    }
}

/// Factory for creating `AdmissionControlFacet` instances from JSON config.
///
/// Register with `FacetRegistry` under type `"admission_control"`.
///
/// Config JSON: `{ "max_concurrent": 100, "strategy": 0, "default_retry_after_ms": 100 }`
pub struct AdmissionControlFacetFactory;

#[async_trait]
impl FacetFactory for AdmissionControlFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        let max_concurrent = config
            .get("max_concurrent")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_CONCURRENT as u64) as u32;
        let strategy = config
            .get("strategy")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let retry_after_ms = config
            .get("default_retry_after_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_RETRY_AFTER_MS);

        let proto_config = AdmissionControlConfig {
            max_concurrent,
            strategy,
            default_retry_after_ms: retry_after_ms,
        };
        Ok(Box::new(AdmissionControlFacet::new(&proto_config)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "admission_control".to_string(),
            attached_at: std::time::Instant::now(),
            config: Value::Null,
            priority: 950,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(max: u32) -> AdmissionControlConfig {
        AdmissionControlConfig {
            max_concurrent: max,
            strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
            default_retry_after_ms: 200,
        }
    }

    #[test]
    fn test_new_from_config() {
        let facet = AdmissionControlFacet::new(&test_config(50));
        assert_eq!(facet.max_concurrent(), 50);
        assert_eq!(facet.retry_after_ms(), 200);
        assert_eq!(facet.in_flight(), 0);
    }

    #[test]
    fn test_defaults_when_zero() {
        let config = AdmissionControlConfig {
            max_concurrent: 0,
            strategy: 0,
            default_retry_after_ms: 0,
        };
        let facet = AdmissionControlFacet::new(&config);
        assert_eq!(facet.max_concurrent(), 100);
        assert_eq!(facet.retry_after_ms(), 100);
    }

    #[test]
    fn test_try_admit_within_limit() {
        let facet = AdmissionControlFacet::new(&test_config(3));
        assert!(facet.try_admit());
        assert_eq!(facet.in_flight(), 1);
        assert!(facet.try_admit());
        assert_eq!(facet.in_flight(), 2);
    }

    #[test]
    fn test_try_admit_at_capacity_rejected() {
        let facet = AdmissionControlFacet::new(&test_config(2));
        assert!(facet.try_admit());
        assert!(facet.try_admit());
        assert!(!facet.try_admit());
        assert_eq!(facet.in_flight(), 2);
    }

    #[test]
    fn test_release_decrements() {
        let facet = AdmissionControlFacet::new(&test_config(5));
        facet.try_admit();
        facet.try_admit();
        assert_eq!(facet.in_flight(), 2);
        facet.release();
        assert_eq!(facet.in_flight(), 1);
    }

    #[tokio::test]
    async fn test_before_method_admits() {
        let facet = AdmissionControlFacet::new(&test_config(5));
        let headers = HashMap::new();
        let result = facet.before_method("test", b"", &headers).await.unwrap();
        assert!(matches!(result, InterceptResult::Continue));
        assert_eq!(facet.in_flight(), 1);
    }

    #[tokio::test]
    async fn test_before_method_rejects_at_capacity() {
        let facet = AdmissionControlFacet::new(&test_config(1));
        let headers = HashMap::new();
        let _ = facet.before_method("test", b"", &headers).await.unwrap();
        let result = facet.before_method("test", b"", &headers).await.unwrap();
        match result {
            InterceptResult::ShortCircuit(bytes) => {
                let v: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(v["error"], "RESOURCE_EXHAUSTED");
                assert_eq!(v["retry_after_ms"], 200);
            }
            _ => panic!("expected ShortCircuit"),
        }
    }

    #[tokio::test]
    async fn test_after_method_releases() {
        let facet = AdmissionControlFacet::new(&test_config(5));
        let headers = HashMap::new();
        facet.before_method("test", b"", &headers).await.unwrap();
        assert_eq!(facet.in_flight(), 1);
        facet
            .after_method("test", b"", b"", &headers)
            .await
            .unwrap();
        assert_eq!(facet.in_flight(), 0);
    }

    #[tokio::test]
    async fn test_on_error_releases() {
        let facet = AdmissionControlFacet::new(&test_config(5));
        let headers = HashMap::new();
        facet.before_method("test", b"", &headers).await.unwrap();
        assert_eq!(facet.in_flight(), 1);
        facet.on_error("test", "boom").await.unwrap();
        assert_eq!(facet.in_flight(), 0);
    }

    #[test]
    fn test_facet_type() {
        let facet = AdmissionControlFacet::with_defaults();
        assert_eq!(facet.facet_type(), "admission_control");
    }

    #[test]
    fn test_get_state() {
        let facet = AdmissionControlFacet::new(&test_config(50));
        let state = facet.get_state().unwrap();
        assert_eq!(state["max_concurrent"], 50);
        assert_eq!(state["retry_after_ms"], 200);
        assert_eq!(state["in_flight"], 0);
    }

    #[tokio::test]
    async fn test_on_attach() {
        let mut facet = AdmissionControlFacet::with_defaults();
        facet.on_attach("my-actor", Value::Null).await.unwrap();
        assert_eq!(facet.actor_id, "my-actor");
    }

    #[test]
    fn test_priority() {
        let facet = AdmissionControlFacet::with_defaults();
        assert_eq!(facet.get_priority(), 950);
    }

    #[tokio::test]
    async fn test_admit_release_cycle() {
        let facet = AdmissionControlFacet::new(&test_config(2));
        let headers = HashMap::new();
        facet.before_method("a", b"", &headers).await.unwrap();
        facet.before_method("b", b"", &headers).await.unwrap();
        assert_eq!(facet.in_flight(), 2);
        let r = facet.before_method("c", b"", &headers).await.unwrap();
        assert!(matches!(r, InterceptResult::ShortCircuit(_)));
        facet
            .after_method("a", b"", b"ok", &headers)
            .await
            .unwrap();
        assert_eq!(facet.in_flight(), 1);
        let r = facet.before_method("c", b"", &headers).await.unwrap();
        assert!(matches!(r, InterceptResult::Continue));
        assert_eq!(facet.in_flight(), 2);
    }

    #[tokio::test]
    async fn test_factory_creates_facet_from_json() {
        use plexspaces_facet::FacetFactory;
        let factory = AdmissionControlFacetFactory;
        let config = json!({
            "max_concurrent": 25,
            "strategy": 0,
            "default_retry_after_ms": 500
        });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.facet_type(), "admission_control");
        assert_eq!(facet.get_priority(), 950);
        let state = facet.get_config();
        assert_eq!(state["max_concurrent"], 25);
        assert_eq!(state["retry_after_ms"], 500);
    }

    #[tokio::test]
    async fn test_factory_defaults_on_empty_config() {
        use plexspaces_facet::FacetFactory;
        let factory = AdmissionControlFacetFactory;
        let facet = factory.create(json!({})).await.unwrap();
        let state = facet.get_config();
        assert_eq!(state["max_concurrent"], 100);
        assert_eq!(state["retry_after_ms"], 100);
    }

    #[test]
    fn test_factory_metadata() {
        use plexspaces_facet::FacetFactory;
        let factory = AdmissionControlFacetFactory;
        let meta = factory.metadata();
        assert_eq!(meta.facet_type, "admission_control");
        assert_eq!(meta.priority, 950);
    }
}
