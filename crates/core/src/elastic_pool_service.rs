// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! Elastic pool service trait for ServiceLocator.
//!
//! Single unified abstraction for actor pools: checkout/checkin, metrics, scale.
//! Implemented by `plexspaces-elastic-pool` (PoolRegistry over ElasticPool).
//! SDK exposes `ElasticPoolClient::from_service_locator()` for in-process access.
//! gRPC for remote pool access can be added separately.

use async_trait::async_trait;
use std::time::Duration;

/// Errors from elastic pool service operations.
#[derive(Debug, thiserror::Error)]
pub enum PoolServiceError {
    #[error("Pool not found: {0}")]
    PoolNotFound(String),

    #[error("Checkout timeout after {0:?}")]
    CheckoutTimeout(Duration),

    #[error("Pool exhausted: all actors busy")]
    PoolExhausted,

    #[error("Circuit open: too many failures")]
    CircuitOpen,

    #[error("Pool draining")]
    PoolDraining,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Actor/service error: {0}")]
    ActorError(String),
}

/// Elastic pool service trait: checkout/checkin, metrics, scale.
///
/// Implemented by the elastic-pool crate (e.g. a registry of pools).
/// SDK obtains this via ServiceLocator and uses it for in-process pool access.
/// gRPC support can be added later for remote pool access.
#[async_trait]
pub trait ElasticPoolService: Send + Sync {
    /// Create a new pool. Returns pool name/id.
    async fn create_pool(
        &self,
        config: plexspaces_proto::pool::v1::PoolConfig,
    ) -> Result<String, PoolServiceError>;

    /// Checkout an actor from the pool (blocks up to timeout).
    async fn checkout(
        &self,
        pool_name: &str,
        timeout: Duration,
    ) -> Result<plexspaces_proto::pool::v1::ActorHandle, PoolServiceError>;

    /// Checkin an actor to the pool.
    async fn checkin(
        &self,
        pool_name: &str,
        actor_id: &str,
        checkout_id: &str,
        healthy: bool,
    ) -> Result<(), PoolServiceError>;

    /// Get pool metrics.
    async fn get_metrics(
        &self,
        pool_name: &str,
    ) -> Result<plexspaces_proto::pool::v1::PoolMetrics, PoolServiceError>;

    /// Scale pool to absolute size.
    async fn scale_to(&self, pool_name: &str, size: u32) -> Result<(), PoolServiceError>;

    /// Scale pool by relative delta.
    async fn scale_by(&self, pool_name: &str, delta: i32) -> Result<(), PoolServiceError>;

    /// Pause auto-scaling.
    async fn pause_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError>;

    /// Resume auto-scaling.
    async fn resume_scaling(&self, pool_name: &str) -> Result<(), PoolServiceError>;

    /// Drain pool (stop new checkouts, wait up to timeout).
    async fn drain(&self, pool_name: &str, timeout: Duration) -> Result<u32, PoolServiceError>;

    /// Delete pool (optionally force with busy actors).
    async fn delete_pool(&self, pool_name: &str, force: bool) -> Result<(), PoolServiceError>;
}
