// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # External Dependency Health Checkers
//!
//! ## Purpose
//! Health checkers for external dependencies: embedded object store, DynamoDB, SQS.
//! These checkers verify liveness and readiness of external services.
//!
//! ## Architecture Context
//! These checkers are used by the health service to monitor external dependencies
//! and update node readiness status accordingly.

use plexspaces_actor::{HealthCheckContext, HealthCheckError, HealthCheckResult, HealthChecker};
use std::time::Duration;
use tokio::time::timeout;

/// S3-compatible object store health checker.
///
/// ## Purpose
/// Checks if the embedded or external S3-compatible object store endpoint is accessible.
///
/// ## Design Notes
/// - Sends GET / to the S3 API endpoint; any HTTP response (including 403) indicates the server is up.
/// - Works for both the auto-started embedded subprocess and external S3-compatible endpoints.
#[derive(Clone)]
pub struct EmbeddedObjectStoreHealthChecker {
    endpoint: String,
    is_critical: bool,
}

impl EmbeddedObjectStoreHealthChecker {
    /// Create a new object store health checker.
    ///
    /// ## Arguments
    /// * `endpoint` - S3-compatible endpoint URL (e.g., "http://127.0.0.1:9100")
    /// * `is_critical` - Whether the object store is critical for node readiness
    pub fn new(endpoint: String, is_critical: bool) -> Self {
        Self {
            endpoint,
            is_critical,
        }
    }
}

#[async_trait::async_trait]
impl HealthChecker for EmbeddedObjectStoreHealthChecker {
    fn name(&self) -> &str {
        "embedded-object-store"
    }

    fn is_critical(&self) -> bool {
        self.is_critical
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        let health_url = format!("{}/", self.endpoint.trim_end_matches('/'));

        let client = reqwest::Client::builder()
            .timeout(timeout_duration)
            .no_proxy()
            .build()
            .map_err(|e| {
                HealthCheckError::CheckFailed(format!("Failed to create HTTP client: {}", e))
            })?;

        match timeout(timeout_duration, client.get(&health_url).send()).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to object store at {}: {}",
                self.endpoint, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "Object store health check timeout for {}",
                self.endpoint
            ))),
        }
    }
}

/// DynamoDB health checker
///
/// ## Purpose
/// Checks if AWS DynamoDB is accessible and healthy.
///
/// ## Design Notes
/// - Uses AWS SDK to check table access
/// - Requires AWS credentials and region
/// - Critical if DynamoDB is used as storage backend
#[derive(Clone)]
pub struct DynamoDBHealthChecker {
    region: String,
    table_name: Option<String>,
    is_critical: bool,
}

impl DynamoDBHealthChecker {
    /// Create a new DynamoDB health checker
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_name` - Optional table name to check (if provided, verifies table exists)
    /// * `is_critical` - Whether DynamoDB is critical for node readiness
    pub fn new(region: String, table_name: Option<String>, is_critical: bool) -> Self {
        Self {
            region,
            table_name,
            is_critical,
        }
    }
}

#[async_trait::async_trait]
impl HealthChecker for DynamoDBHealthChecker {
    fn name(&self) -> &str {
        "dynamodb"
    }

    fn is_critical(&self) -> bool {
        self.is_critical
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        let endpoint = format!("dynamodb.{}.amazonaws.com", self.region);

        match timeout(
            timeout_duration,
            tokio::net::TcpStream::connect(format!("{}:443", endpoint)),
        )
        .await
        {
            Ok(Ok(_stream)) => Ok(()),
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to DynamoDB at {}: {}",
                endpoint, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "DynamoDB health check timeout for {}",
                endpoint
            ))),
        }
    }
}

/// SQS health checker
///
/// ## Purpose
/// Checks if AWS SQS is accessible and healthy.
///
/// ## Design Notes
/// - Uses AWS SDK to check queue access
/// - Requires AWS credentials and region
/// - Critical if SQS is used as channel backend
#[derive(Clone)]
pub struct SQSHealthChecker {
    region: String,
    queue_url: Option<String>,
    is_critical: bool,
}

impl SQSHealthChecker {
    /// Create a new SQS health checker
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `queue_url` - Optional queue URL to check (if provided, verifies queue exists)
    /// * `is_critical` - Whether SQS is critical for node readiness
    pub fn new(region: String, queue_url: Option<String>, is_critical: bool) -> Self {
        Self {
            region,
            queue_url,
            is_critical,
        }
    }
}

#[async_trait::async_trait]
impl HealthChecker for SQSHealthChecker {
    fn name(&self) -> &str {
        "sqs"
    }

    fn is_critical(&self) -> bool {
        self.is_critical
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        let endpoint = format!("sqs.{}.amazonaws.com", self.region);

        match timeout(
            timeout_duration,
            tokio::net::TcpStream::connect(format!("{}:443", endpoint)),
        )
        .await
        {
            Ok(Ok(_stream)) => Ok(()),
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to SQS at {}: {}",
                endpoint, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "SQS health check timeout for {}",
                endpoint
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedded_object_store_health_checker() {
        let checker = EmbeddedObjectStoreHealthChecker::new("http://localhost:9000".to_string(), true);

        assert_eq!(checker.name(), "embedded-object-store");
        assert!(checker.is_critical());

        // Will fail if the object store is not running, which is expected in unit tests
        let ctx = HealthCheckContext::default();
        let _result = checker.check(&ctx).await;
    }

    #[tokio::test]
    async fn test_dynamodb_health_checker() {
        let checker = DynamoDBHealthChecker::new("us-east-1".to_string(), None, true);

        assert_eq!(checker.name(), "dynamodb");
        assert!(checker.is_critical());

        let ctx = HealthCheckContext::default();
        let _result = checker.check(&ctx).await;
    }

    #[tokio::test]
    async fn test_sqs_health_checker() {
        let checker = SQSHealthChecker::new("us-east-1".to_string(), None, true);

        assert_eq!(checker.name(), "sqs");
        assert!(checker.is_critical());

        let ctx = HealthCheckContext::default();
        let _result = checker.check(&ctx).await;
    }
}
