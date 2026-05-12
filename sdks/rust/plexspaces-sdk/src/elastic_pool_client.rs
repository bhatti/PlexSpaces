// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Elastic pool client using ServiceLocator (direct services, no gRPC).
// gRPC support can be added later for remote pool access.

use anyhow::Context;
use plexspaces_actor::{ElasticPoolService, PoolServiceError};
use plexspaces_proto::pool::v1::{ActorHandle, PoolConfig, PoolMetrics};
use std::sync::Arc;
use std::time::Duration;

/// Client for elastic pool operations via ServiceLocator.
///
/// Uses direct service calls (no gRPC). Obtain from ServiceLocator with
/// `ElasticPoolClient::from_service_locator(service_locator)`.
///
/// ## Abstractions
/// - **Checkout**: Obtain an actor handle from the pool (blocks up to timeout).
/// - **Checkin**: Return the actor to the pool.
/// - **Metrics**: Get pool stats (size, busy, available, load).
/// - **Scale**: Manually scale pool size.
pub struct ElasticPoolClient {
    service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
}

impl ElasticPoolClient {
    /// Create a client that uses the given ServiceLocator for pool operations.
    pub fn from_service_locator(
        service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    ) -> Self {
        Self { service_locator }
    }

    /// Get the ElasticPoolService from ServiceLocator (errors if not registered).
    async fn pool_service(&self) -> anyhow::Result<Arc<dyn ElasticPoolService>> {
        self.service_locator
            .get_elastic_pool_service()
            .await
            .context("ElasticPoolService not registered in ServiceLocator")
    }

    /// Create a new pool. Returns pool name/id.
    pub async fn create_pool(&self, config: PoolConfig) -> Result<String, PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.create_pool(config).await
    }

    /// Checkout an actor from the pool (blocks up to `timeout`).
    pub async fn checkout(
        &self,
        pool_name: &str,
        timeout: Duration,
    ) -> Result<ActorHandle, PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.checkout(pool_name, timeout).await
    }

    /// Checkin an actor to the pool.
    pub async fn checkin(
        &self,
        pool_name: &str,
        actor_id: &str,
        checkout_id: &str,
        healthy: bool,
    ) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.checkin(pool_name, actor_id, checkout_id, healthy).await
    }

    /// Get pool metrics (size, available, busy, load, etc.).
    pub async fn get_metrics(&self, pool_name: &str) -> Result<PoolMetrics, PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.get_metrics(pool_name).await
    }

    /// Scale pool to an absolute size.
    pub async fn scale_to(&self, pool_name: &str, size: u32) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.scale_to(pool_name, size).await
    }

    /// Scale pool by a relative delta (positive = add, negative = remove).
    pub async fn scale_by(&self, pool_name: &str, delta: i32) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.scale_by(pool_name, delta).await
    }

    /// Pause auto-scaling for the pool.
    pub async fn pause_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.pause_scaling(pool_name).await
    }

    /// Resume auto-scaling for the pool.
    pub async fn resume_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.resume_scaling(pool_name).await
    }

    /// Drain the pool (stop accepting new checkouts, wait up to timeout).
    pub async fn drain(&self, pool_name: &str, timeout: Duration) -> Result<u32, PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.drain(pool_name, timeout).await
    }

    /// Delete the pool (optionally force with busy actors).
    pub async fn delete_pool(&self, pool_name: &str, force: bool) -> Result<(), PoolServiceError> {
        let svc = self
            .pool_service()
            .await
            .map_err(|e| PoolServiceError::ActorError(e.to_string()))?;
        svc.delete_pool(pool_name, force).await
    }
}
