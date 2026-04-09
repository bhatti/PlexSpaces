// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for the simple host elastic pool API (WIT host interface).
// Uses a mock ElasticPoolService for deterministic, timing-independent tests:
// no real pool, no sleeps, no background tasks. Validates that the host layer
// calls the service and returns protobuf responses with result-based errors.

#[cfg(feature = "component-model")]
mod tests {
    use async_trait::async_trait;
    use plexspaces_core::{ActorId, ElasticPoolService, PoolServiceError};
    use plexspaces_proto::pool::v1::{ActorHandle, PoolConfig, PoolMetrics};
    use plexspaces_wasm_runtime::simple_component_host::plexspaces::actor::host::Host;
    use plexspaces_wasm_runtime::simple_component_host::SimpleHostImpl;
    use plexspaces_wasm_runtime::HostFunctions;
    use prost::Message as _;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    /// Mock ElasticPoolService: returns deterministic results immediately.
    /// No timing, no background tasks, no real pool.
    struct MockPoolService {
        /// If Some(name), checkout/get_metrics succeed for that pool; checkout returns fixed handle.
        /// If None, all operations return PoolNotFound.
        pool_name: Option<String>,
    }

    impl MockPoolService {
        fn with_pool(name: &str) -> Self {
            Self {
                pool_name: Some(name.to_string()),
            }
        }
    }

    #[async_trait]
    impl ElasticPoolService for MockPoolService {
        async fn create_pool(&self, _config: PoolConfig) -> Result<String, PoolServiceError> {
            unimplemented!("tests do not call create_pool on host")
        }

        async fn checkout(
            &self,
            pool_name: &str,
            _timeout: Duration,
        ) -> Result<ActorHandle, PoolServiceError> {
            match &self.pool_name {
                Some(name) if name == pool_name => Ok(ActorHandle {
                    actor_id: "mock-actor-1".to_string(),
                    pool_name: pool_name.to_string(),
                    checkout_time: None,
                    checkout_id: "mock-checkout-id".to_string(),
                    metadata: HashMap::new(),
                }),
                _ => Err(PoolServiceError::PoolNotFound(pool_name.to_string())),
            }
        }

        async fn checkin(
            &self,
            pool_name: &str,
            _actor_id: &str,
            _checkout_id: &str,
            _healthy: bool,
        ) -> Result<(), PoolServiceError> {
            match &self.pool_name {
                Some(name) if name == pool_name => Ok(()),
                _ => Err(PoolServiceError::PoolNotFound(pool_name.to_string())),
            }
        }

        async fn get_metrics(&self, pool_name: &str) -> Result<PoolMetrics, PoolServiceError> {
            match &self.pool_name {
                Some(name) if name == pool_name => Ok(PoolMetrics {
                    name: pool_name.to_string(),
                    scaling_state: 0,
                    total_actors: 3,
                    available_actors: 2,
                    busy_actors: 1,
                    idle_actors: 0,
                    failed_actors: 0,
                    waiting_requests: 0,
                    total_checkouts: 0,
                    total_checkins: 0,
                    total_timeouts: 0,
                    current_load: 0.33,
                    avg_load_1m: 0.0,
                    avg_load_5m: 0.0,
                    avg_checkout_latency: 0,
                    p95_checkout_latency: 0,
                    p99_checkout_latency: 0,
                    avg_actor_usage_time: 0,
                    avg_actor_idle_time: 0,
                    circuit_state: "closed".to_string(),
                    last_scale_up: None,
                    last_scale_down: None,
                    custom_metrics: HashMap::new(),
                }),
                _ => Err(PoolServiceError::PoolNotFound(pool_name.to_string())),
            }
        }

        async fn scale_to(&self, _pool_name: &str, _size: u32) -> Result<(), PoolServiceError> {
            unimplemented!("tests do not call scale_to")
        }

        async fn scale_by(&self, _pool_name: &str, _delta: i32) -> Result<(), PoolServiceError> {
            unimplemented!("tests do not call scale_by")
        }

        async fn pause_scaling(&self, _pool_name: &str) -> Result<(), PoolServiceError> {
            unimplemented!("tests do not call pause_scaling")
        }

        async fn resume_scaling(&self, _pool_name: &str) -> Result<(), PoolServiceError> {
            unimplemented!("tests do not call resume_scaling")
        }

        async fn drain(
            &self,
            _pool_name: &str,
            _timeout: Duration,
        ) -> Result<u32, PoolServiceError> {
            unimplemented!("tests do not call drain")
        }

        async fn delete_pool(
            &self,
            _pool_name: &str,
            _force: bool,
        ) -> Result<(), PoolServiceError> {
            unimplemented!("tests do not call delete_pool")
        }
    }

    fn create_host_with_service(svc: Arc<dyn ElasticPoolService>) -> SimpleHostImpl {
        let host_functions = Arc::new(HostFunctions::with_all_services(
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(svc),
            None, // No outbound HTTP client
        ));
        SimpleHostImpl::new(
            ActorId::from("test-actor".to_string()),
            host_functions,
            None,
        )
    }

    #[tokio::test]
    async fn test_simple_host_pool_checkout_returns_protobuf() {
        let svc: Arc<dyn ElasticPoolService> = Arc::new(MockPoolService::with_pool("p1"));
        let mut host = create_host_with_service(svc);

        let out = host
            .pool_checkout("p1".to_string(), 2000)
            .await
            .expect("pool_checkout should succeed");
        let parsed = ActorHandle::decode(out.as_slice())
            .expect("pool_checkout must return valid protobuf");
        assert_eq!(parsed.pool_name, "p1");
        assert_eq!(parsed.actor_id, "mock-actor-1");
        assert_eq!(parsed.checkout_id, "mock-checkout-id");
    }

    #[tokio::test]
    async fn test_simple_host_pool_checkin_success() {
        let svc: Arc<dyn ElasticPoolService> = Arc::new(MockPoolService::with_pool("p2"));
        let mut host = create_host_with_service(svc);

        let checkin_out = host
            .pool_checkin(
                "p2".to_string(),
                "mock-actor-1".to_string(),
                "mock-checkout-id".to_string(),
                true,
            )
            .await;
        assert!(checkin_out.is_ok(), "pool_checkin should succeed: {checkin_out:?}");
    }

    #[tokio::test]
    async fn test_simple_host_pool_get_metrics_returns_protobuf() {
        let svc: Arc<dyn ElasticPoolService> = Arc::new(MockPoolService::with_pool("p3"));
        let mut host = create_host_with_service(svc);

        let out = host
            .pool_get_metrics("p3".to_string())
            .await
            .expect("pool_get_metrics should succeed");
        let parsed =
            PoolMetrics::decode(out.as_slice()).expect("pool_get_metrics must return protobuf");
        assert_eq!(parsed.total_actors, 3);
        assert_eq!(parsed.available_actors, 2);
        assert_eq!(parsed.busy_actors, 1);
        assert_eq!(parsed.current_load, 0.33);
    }

    #[tokio::test]
    async fn test_simple_host_pool_not_configured_returns_error() {
        let host_functions = Arc::new(HostFunctions::new());
        let mut host = SimpleHostImpl::new(
            ActorId::from("test-actor".to_string()),
            host_functions,
            None,
        );

        let out = host.pool_checkout("any-pool".to_string(), 100).await;
        let err = out.expect_err("pool_checkout without service should fail");
        assert!(err.contains("not configured") || err.contains("Elastic pool"));

        let out_checkin = host
            .pool_checkin("any".to_string(), "a".to_string(), "c".to_string(), true)
            .await;
        assert!(out_checkin.is_err());

        let out_metrics = host.pool_get_metrics("any".to_string()).await;
        assert!(out_metrics.is_err());
    }

    #[tokio::test]
    async fn test_simple_host_pool_not_found_returns_error() {
        let svc: Arc<dyn ElasticPoolService> = Arc::new(MockPoolService::with_pool("existing"));
        let mut host = create_host_with_service(svc);

        let out = host.pool_checkout("missing-pool".to_string(), 100).await;
        let err = out.expect_err("pool_checkout for missing pool should fail");
        assert!(err.to_lowercase().contains("not found") || err.to_lowercase().contains("missing"));
    }
}
