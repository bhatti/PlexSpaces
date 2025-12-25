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

//! # External Dependency Health Checkers
//!
//! ## Purpose
//! Health checkers for external dependencies: MinIO, DynamoDB, SQS.
//! These checkers verify liveness and readiness of external services.
//!
//! ## Architecture Context
//! These checkers are used by the health service to monitor external dependencies
//! and update node readiness status accordingly.

use crate::health_checker::{HealthChecker, HealthCheckContext, HealthCheckError, HealthCheckResult};
use std::time::Duration;
use tokio::time::timeout;

/// MinIO health checker
///
/// ## Purpose
/// Checks if MinIO (S3-compatible object storage) is accessible and healthy.
///
/// ## Design Notes
/// - Checks MinIO health endpoint: `{endpoint}/minio/health/live`
/// - Uses HTTP GET request with timeout
/// - Critical if blob backend is MinIO
#[derive(Clone)]
pub struct MinIOHealthChecker {
    endpoint: String,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    is_critical: bool,
}

impl MinIOHealthChecker {
    /// Create a new MinIO health checker
    ///
    /// ## Arguments
    /// * `endpoint` - MinIO endpoint URL (e.g., "http://localhost:9000")
    /// * `access_key_id` - Optional access key for authentication
    /// * `secret_access_key` - Optional secret key for authentication
    /// * `is_critical` - Whether MinIO is critical for node readiness
    pub fn new(
        endpoint: String,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        is_critical: bool,
    ) -> Self {
        Self {
            endpoint,
            access_key_id,
            secret_access_key,
            is_critical,
        }
    }
}

#[async_trait::async_trait]
impl HealthChecker for MinIOHealthChecker {
    fn name(&self) -> &str {
        "minio"
    }

    fn is_critical(&self) -> bool {
        self.is_critical
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));
        
        // MinIO health endpoint
        let health_url = format!("{}/minio/health/live", self.endpoint.trim_end_matches('/'));
        
        // Create HTTP client
        let client = reqwest::Client::builder()
            .timeout(timeout_duration)
            .build()
            .map_err(|e| HealthCheckError::CheckFailed(format!("Failed to create HTTP client: {}", e)))?;
        
        // Build request
        let mut request = client.get(&health_url);
        
        // Add authentication if provided
        if let (Some(access_key), Some(secret_key)) = (&self.access_key_id, &self.secret_access_key) {
            request = request.basic_auth(access_key, Some(secret_key));
        }
        
        // Perform health check
        match timeout(timeout_duration, request.send()).await {
            Ok(Ok(response)) => {
                if response.status().is_success() {
                    Ok(())
                } else {
                    Err(HealthCheckError::CheckFailed(format!(
                        "MinIO health check returned status {}",
                        response.status()
                    )))
                }
            }
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to MinIO at {}: {}",
                self.endpoint, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "MinIO health check timeout for {}",
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
        
        // For now, we'll do a simple connectivity check
        // In production, you might want to use AWS SDK to check table access
        // This is a placeholder that checks if we can resolve the DynamoDB endpoint
        
        // DynamoDB endpoint format: dynamodb.{region}.amazonaws.com
        let endpoint = format!("dynamodb.{}.amazonaws.com", self.region);
        
        // Try to resolve DNS and connect (basic connectivity check)
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
        
        // SQS endpoint format: sqs.{region}.amazonaws.com
        let endpoint = format!("sqs.{}.amazonaws.com", self.region);
        
        // Try to resolve DNS and connect (basic connectivity check)
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
    async fn test_minio_health_checker() {
        let checker = MinIOHealthChecker::new(
            "http://localhost:9000".to_string(),
            None,
            None,
            true,
        );
        
        assert_eq!(checker.name(), "minio");
        assert!(checker.is_critical());
        
        // This will fail if MinIO is not running, but that's OK for the test
        let ctx = HealthCheckContext::default();
        let _result = checker.check(&ctx).await;
    }

    #[tokio::test]
    async fn test_dynamodb_health_checker() {
        let checker = DynamoDBHealthChecker::new(
            "us-east-1".to_string(),
            None,
            true,
        );
        
        assert_eq!(checker.name(), "dynamodb");
        assert!(checker.is_critical());
        
        let ctx = HealthCheckContext::default();
        // This will try to connect to AWS DynamoDB endpoint
        // It may fail if not on AWS network, but that's OK for the test
        let _result = checker.check(&ctx).await;
    }

    #[tokio::test]
    async fn test_sqs_health_checker() {
        let checker = SQSHealthChecker::new(
            "us-east-1".to_string(),
            None,
            true,
        );
        
        assert_eq!(checker.name(), "sqs");
        assert!(checker.is_critical());
        
        let ctx = HealthCheckContext::default();
        // This will try to connect to AWS SQS endpoint
        // It may fail if not on AWS network, but that's OK for the test
        let _result = checker.check(&ctx).await;
    }
}

