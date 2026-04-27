// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Pool registry: implements ElasticPoolService for ServiceLocator.
// Holds named ElasticPool instances so the SDK can access them via ServiceLocator.

use async_trait::async_trait;
use plexspaces_core::{ElasticPoolService, PoolServiceError};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::{ElasticPool, ElasticPoolError};

fn map_error(e: ElasticPoolError) -> PoolServiceError {
    use ElasticPoolError as E;
    use PoolServiceError as P;
    match e {
        E::PoolNotFound(s) => P::PoolNotFound(s),
        E::CheckoutTimeout(d) => P::CheckoutTimeout(d),
        E::PoolExhausted => P::PoolExhausted,
        E::CircuitOpen => P::CircuitOpen,
        E::PoolDraining => P::PoolDraining,
        E::InvalidConfig(s) => P::InvalidConfig(s),
        E::ActorError(s) => P::ActorError(s),
    }
}

/// Registry of named elastic pools. Implements ElasticPoolService for use via ServiceLocator.
pub struct PoolRegistry {
    pools: Arc<RwLock<HashMap<String, ElasticPool>>>,
}

impl PoolRegistry {
    /// Create an empty registry. Register pools with `create_pool` or `register_pool`.
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an existing pool under a name (e.g. created elsewhere).
    pub async fn register_pool(
        &self,
        name: String,
        pool: ElasticPool,
    ) -> Result<(), PoolServiceError> {
        let mut pools = self.pools.write().await;
        if pools.contains_key(&name) {
            return Err(PoolServiceError::InvalidConfig(format!(
                "Pool already exists: {}",
                name
            )));
        }
        pools.insert(name, pool);
        Ok(())
    }
}

impl Default for PoolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ElasticPoolService for PoolRegistry {
    async fn create_pool(
        &self,
        config: plexspaces_proto::pool::v1::PoolConfig,
    ) -> Result<String, PoolServiceError> {
        let name = config.name.clone();
        let pool = ElasticPool::new(config).await.map_err(map_error)?;
        let mut pools = self.pools.write().await;
        if pools.contains_key(&name) {
            return Err(PoolServiceError::InvalidConfig(format!(
                "Pool already exists: {}",
                name
            )));
        }
        pools.insert(name.clone(), pool);
        Ok(name)
    }

    async fn checkout(
        &self,
        pool_name: &str,
        timeout: Duration,
    ) -> Result<plexspaces_proto::pool::v1::ActorHandle, PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.checkout(timeout).await.map_err(map_error)
    }

    async fn checkin(
        &self,
        pool_name: &str,
        actor_id: &str,
        checkout_id: &str,
        healthy: bool,
    ) -> Result<(), PoolServiceError> {
        let _ = (checkout_id, healthy);
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        let handle = plexspaces_proto::pool::v1::ActorHandle {
            actor_id: actor_id.to_string(),
            pool_name: pool_name.to_string(),
            checkout_time: None,
            checkout_id: String::new(),
            metadata: std::collections::HashMap::new(),
        };
        pool.checkin(handle).await.map_err(map_error)
    }

    async fn get_metrics(
        &self,
        pool_name: &str,
    ) -> Result<plexspaces_proto::pool::v1::PoolMetrics, PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.get_metrics().await.map_err(map_error)
    }

    async fn scale_to(&self, pool_name: &str, size: u32) -> Result<(), PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.scale_to(size).await.map_err(map_error)
    }

    async fn scale_by(&self, pool_name: &str, delta: i32) -> Result<(), PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.scale_by(delta).await.map_err(map_error)
    }

    async fn pause_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.pause_scaling().await.map_err(map_error)
    }

    async fn resume_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.resume_scaling().await.map_err(map_error)
    }

    async fn drain(&self, pool_name: &str, timeout: Duration) -> Result<u32, PoolServiceError> {
        let pools = self.pools.read().await;
        let pool = pools
            .get(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.drain(timeout).await.map_err(map_error)
    }

    async fn delete_pool(&self, pool_name: &str, force: bool) -> Result<(), PoolServiceError> {
        let mut pools = self.pools.write().await;
        let pool = pools
            .remove(pool_name)
            .ok_or_else(|| PoolServiceError::PoolNotFound(pool_name.to_string()))?;
        pool.delete(force).await.map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::pool::v1::PoolConfig;
    use prost_types::Duration as ProtoDuration;

    fn test_config(name: &str) -> PoolConfig {
        PoolConfig {
            name: name.to_string(),
            min_size: 2,
            max_size: 10,
            initial_size: 3,
            scaling_threshold: 0.8,
            scale_down_threshold: 0.3,
            scaling_check_interval: Some(ProtoDuration {
                seconds: 0,
                nanos: 100_000_000,
            }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn registry_create_pool_and_checkout_checkin() {
        let registry = PoolRegistry::new();
        let name = registry.create_pool(test_config("p1")).await.unwrap();
        assert_eq!(name, "p1");

        let handle = registry
            .checkout("p1", Duration::from_secs(1))
            .await
            .unwrap();
        assert!(!handle.actor_id.is_empty());

        let metrics = registry.get_metrics("p1").await.unwrap();
        assert_eq!(metrics.busy_actors, 1);
        assert_eq!(metrics.available_actors, 2);

        registry
            .checkin("p1", &handle.actor_id, &handle.checkout_id, true)
            .await
            .unwrap();
        let metrics = registry.get_metrics("p1").await.unwrap();
        assert_eq!(metrics.busy_actors, 0);
    }

    #[tokio::test]
    async fn registry_pool_not_found() {
        let registry = PoolRegistry::new();
        let err = registry
            .checkout("missing", Duration::from_secs(1))
            .await
            .unwrap_err();
        match &err {
            PoolServiceError::PoolNotFound(n) => assert_eq!(n, "missing"),
            _ => panic!("expected PoolNotFound"),
        }
    }
}
