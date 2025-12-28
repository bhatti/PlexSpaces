// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
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

//! # Automatic Dependency Registration
//!
//! ## Purpose
//! Automatically registers health checkers for dependencies discovered via
//! object-registry by name/type.
//!
//! ## Architecture Context
//! This module integrates with:
//! - Object-registry for service discovery
//! - Health service for dependency checks
//! - Release configuration for dependency definitions
//!
//! ## Design Notes
//! - Dependencies are registered at node startup
//! - Uses object-registry to discover services by name/type
//! - Supports both critical and non-critical dependencies

use crate::health_checker::{HealthChecker, HealthCheckContext, HealthCheckError, HealthCheckResult};
use crate::health_checker_circuit_breaker::CircuitBreakerHealthChecker;
use crate::external_dependency_checkers::{MinIOHealthChecker, DynamoDBHealthChecker, SQSHealthChecker};
use crate::health_service::PlexSpacesHealthReporter;
use plexspaces_core::ObjectRegistry;
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_proto::system::v1::DependencyRegistrationConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Register built-in dependencies (Redis, PostgreSQL, Kafka) if enabled
///
/// ## Purpose
/// Automatically registers health checkers for database and messaging backends
/// that are enabled via environment variables or configuration.
///
/// ## Arguments
/// * `reporter` - Health reporter to register checkers with
///
/// ## Returns
/// Number of dependencies registered
pub async fn register_builtin_dependencies(
    reporter: Arc<PlexSpacesHealthReporter>,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let mut registered_count = 0;

    // Check for Redis (used by KeyValue store and TupleSpace)
    if let Ok(redis_url) = std::env::var("PLEXSPACES_KV_REDIS_URL")
        .or_else(|_| std::env::var("PLEXSPACES_REDIS_URL"))
    {
        let checker = RedisHealthChecker {
            url: redis_url.clone(),
        };
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        reporter.register_startup_checker(cb_checker.clone()).await;
        registered_count += 1;
        eprintln!("✅ Registered Redis health checker (critical) with circuit breaker");
    }

    // Check for PostgreSQL (used by KeyValue store and TupleSpace)
    if let Ok(postgres_url) = std::env::var("PLEXSPACES_KV_POSTGRES_URL")
        .or_else(|_| std::env::var("PLEXSPACES_TUPLESPACE_POSTGRES_URL"))
        .or_else(|_| std::env::var("PLEXSPACES_POSTGRES_URL"))
    {
        let checker = PostgreSQLHealthChecker {
            connection_string: postgres_url.clone(),
        };
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        reporter.register_startup_checker(cb_checker.clone()).await;
        registered_count += 1;
        eprintln!("✅ Registered PostgreSQL health checker (critical) with circuit breaker");
    }

    // Check for Kafka (messaging)
    if let Ok(kafka_brokers) = std::env::var("PLEXSPACES_KAFKA_BROKERS") {
        let checker = KafkaHealthChecker {
            brokers: kafka_brokers.clone(),
        };
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        reporter.register_startup_checker(cb_checker.clone()).await;
        registered_count += 1;
        eprintln!("✅ Registered Kafka health checker (critical) with circuit breaker");
    }

    // Check for MinIO (blob storage)
    if let Ok(minio_endpoint) = std::env::var("BLOB_ENDPOINT")
        .or_else(|_| std::env::var("PLEXSPACES_MINIO_ENDPOINT"))
    {
        let access_key = std::env::var("BLOB_ACCESS_KEY_ID")
            .or_else(|_| std::env::var("PLEXSPACES_MINIO_ACCESS_KEY"))
            .ok();
        let secret_key = std::env::var("BLOB_SECRET_ACCESS_KEY")
            .or_else(|_| std::env::var("PLEXSPACES_MINIO_SECRET_KEY"))
            .ok();
        
        // MinIO is critical if blob backend is minio
        let is_critical = std::env::var("BLOB_BACKEND")
            .unwrap_or_default()
            .to_lowercase() == "minio";
        
        let checker = MinIOHealthChecker::new(
            minio_endpoint.clone(),
            access_key,
            secret_key,
            is_critical,
        );
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        if is_critical {
            reporter.register_startup_checker(cb_checker.clone()).await;
        }
        registered_count += 1;
        eprintln!("✅ Registered MinIO health checker (critical: {}) with circuit breaker", is_critical);
    }

    // Check for DynamoDB (storage backend)
    if let Ok(aws_region) = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("PLEXSPACES_DYNAMODB_REGION"))
    {
        let table_name = std::env::var("PLEXSPACES_DYNAMODB_TABLE").ok();
        
        // DynamoDB is critical if used as storage backend
        let is_critical = std::env::var("KEYVALUE_BACKEND")
            .or_else(|_| std::env::var("TUPLESPACE_BACKEND"))
            .unwrap_or_default()
            .to_lowercase() == "dynamodb";
        
        let checker = DynamoDBHealthChecker::new(
            aws_region.clone(),
            table_name,
            is_critical,
        );
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        if is_critical {
            reporter.register_startup_checker(cb_checker.clone()).await;
        }
        registered_count += 1;
        eprintln!("✅ Registered DynamoDB health checker (critical: {}) with circuit breaker", is_critical);
    }

    // Check for SQS (channel backend)
    if let Ok(aws_region) = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("PLEXSPACES_SQS_REGION"))
    {
        let queue_url = std::env::var("PLEXSPACES_SQS_QUEUE_URL").ok();
        
        // SQS is critical if used as channel backend
        let is_critical = std::env::var("CHANNEL_BACKEND")
            .unwrap_or_default()
            .to_lowercase() == "sqs";
        
        let checker = SQSHealthChecker::new(
            aws_region.clone(),
            queue_url,
            is_critical,
        );
        let checker = Arc::new(checker);
        // Wrap with circuit breaker (using existing CircuitBreaker implementation)
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        let cb_checker = Arc::new(cb_checker);
        reporter.register_readiness_checker(cb_checker.clone()).await;
        if is_critical {
            reporter.register_startup_checker(cb_checker.clone()).await;
        }
        registered_count += 1;
        eprintln!("✅ Registered SQS health checker (critical: {}) with circuit breaker", is_critical);
    }

    Ok(registered_count)
}

/// Register dependencies automatically based on configuration
///
/// ## Arguments
/// * `reporter` - Health reporter to register checkers with
/// * `registry` - Object registry for service discovery
/// * `config` - Dependency registration configuration
///
/// ## Returns
/// Number of dependencies registered
pub async fn register_dependencies(
    reporter: Arc<PlexSpacesHealthReporter>,
    registry: Arc<dyn ObjectRegistry>,
    config: DependencyRegistrationConfig,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    if !config.enabled {
        return Ok(0);
    }

    let mut registered_count = 0;
    let default_namespace = config.default_namespace.as_str();
    let default_tenant = config.default_tenant.as_str();
    
    // Create RequestContext for dependency lookup (using config defaults)
    use plexspaces_core::RequestContext;
    let ctx = RequestContext::new_without_auth(default_tenant.to_string(), default_namespace.to_string());

    for dep_spec in &config.dependencies {
        // Parse dependency spec: "name:type:critical"
        let parts: Vec<&str> = dep_spec.split(':').collect();
        if parts.len() < 2 {
            eprintln!("Warning: Invalid dependency spec '{}', expected 'name:type:critical'", dep_spec);
            continue;
        }

        let dep_name = parts[0];
        let dep_type_str = parts[1];
        let is_critical = parts.get(2).map(|s| s == &"true").unwrap_or(true);

        // Parse object type
        let object_type = match dep_type_str {
            "service" => ObjectType::ObjectTypeService,
            "actor" => ObjectType::ObjectTypeActor,
            "tuplespace" => ObjectType::ObjectTypeTuplespace,
            _ => {
                eprintln!("Warning: Unknown dependency type '{}' for '{}'", dep_type_str, dep_name);
                continue;
            }
        };

        // Lookup dependency in object-registry using RequestContext
        let registration = match registry
            .lookup_full(&ctx, object_type, dep_name)
            .await
        {
            Ok(Some(reg)) => reg,
            Ok(None) => {
                eprintln!("Warning: Dependency '{}' not found in registry", dep_name);
                continue;
            }
            Err(e) => {
                eprintln!("Error looking up dependency '{}': {}", dep_name, e);
                continue;
            }
        };

        // Create health checker for this dependency
        let checker = DependencyHealthChecker {
            name: dep_name.to_string(),
            grpc_address: registration.grpc_address.clone(),
            is_critical,
        };

        // Register based on criticality
        if is_critical {
            reporter.register_readiness_checker(Arc::new(checker.clone())).await;
            reporter.register_startup_checker(Arc::new(checker.clone())).await;
        } else {
            reporter.register_readiness_checker(Arc::new(checker.clone())).await;
        }

        registered_count += 1;
        eprintln!("✅ Registered dependency checker: {} (critical: {})", dep_name, is_critical);
    }

    Ok(registered_count)
}

/// Health checker for dependencies discovered via object-registry
/// Health checker for dependencies discovered via object-registry
#[derive(Clone)]
pub struct DependencyHealthChecker {
    name: String,
    grpc_address: String,
    is_critical: bool,
}

#[async_trait::async_trait]
impl HealthChecker for DependencyHealthChecker {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_critical(&self) -> bool {
        self.is_critical
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        // Implement actual gRPC health check using tonic-health client
        // Use the standard grpc.health.v1.Health service
        match timeout(timeout_duration, async {
            use tonic_health::pb::health_client::HealthClient;
            use tonic_health::pb::HealthCheckRequest;
            
            // Parse address (remove http:// or https:// if present)
            let address = self.grpc_address
                .strip_prefix("http://")
                .or_else(|| self.grpc_address.strip_prefix("https://"))
                .unwrap_or(&self.grpc_address);
            
            // Create gRPC channel
            let channel = tonic::transport::Channel::from_shared(format!("http://{}", address))
                .map_err(|e| HealthCheckError::CheckFailed(format!("Invalid gRPC address: {}", e)))?
                .connect()
                .await
                .map_err(|e| HealthCheckError::CheckFailed(format!("Failed to connect to {}: {}", self.grpc_address, e)))?;
            
            // Create health client
            let mut client = HealthClient::new(channel);
            
            // Check health (use empty service name for default service)
            let request = tonic::Request::new(HealthCheckRequest {
                service: String::new(), // Empty = check overall service health
            });
            
            let response = client.check(request).await
                .map_err(|e| HealthCheckError::CheckFailed(format!("Health check failed for {}: {}", self.name, e)))?;
            
            // Verify response indicates healthy status
            use tonic_health::pb::health_check_response::ServingStatus;
            match response.into_inner().status() {
                ServingStatus::Serving => Ok(()),
                status => Err(HealthCheckError::CheckFailed(format!(
                    "Service {} is not serving (status: {:?})",
                    self.name, status
                ))),
            }
        }).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "Health check timeout for {}",
                self.name
            ))),
        }
    }
}

/// Redis health checker
#[derive(Clone)]
struct RedisHealthChecker {
    url: String,
}

#[async_trait::async_trait]
impl HealthChecker for RedisHealthChecker {
    fn name(&self) -> &str {
        "redis"
    }

    fn is_critical(&self) -> bool {
        true
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        // Parse Redis URL to extract host:port
        let (host, port) = parse_redis_url(&self.url)?;

        // Try to connect to Redis
        match timeout(
            timeout_duration,
            tokio::net::TcpStream::connect(format!("{}:{}", host, port)),
        )
        .await
        {
            Ok(Ok(_stream)) => Ok(()),
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to Redis at {}:{}: {}",
                host, port, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "Redis health check timeout for {}",
                self.url
            ))),
        }
    }
}

/// PostgreSQL health checker
#[derive(Clone)]
struct PostgreSQLHealthChecker {
    connection_string: String,
}

#[async_trait::async_trait]
impl HealthChecker for PostgreSQLHealthChecker {
    fn name(&self) -> &str {
        "postgresql"
    }

    fn is_critical(&self) -> bool {
        true
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        // Parse PostgreSQL connection string to extract host:port
        let (host, port) = parse_postgres_url(&self.connection_string)?;

        // Try to connect to PostgreSQL
        match timeout(
            timeout_duration,
            tokio::net::TcpStream::connect(format!("{}:{}", host, port)),
        )
        .await
        {
            Ok(Ok(_stream)) => Ok(()),
            Ok(Err(e)) => Err(HealthCheckError::CheckFailed(format!(
                "Failed to connect to PostgreSQL at {}:{}: {}",
                host, port, e
            ))),
            Err(_) => Err(HealthCheckError::Timeout(format!(
                "PostgreSQL health check timeout for {}",
                self.connection_string
            ))),
        }
    }
}

/// Kafka health checker
#[derive(Clone)]
struct KafkaHealthChecker {
    brokers: String,
}

#[async_trait::async_trait]
impl HealthChecker for KafkaHealthChecker {
    fn name(&self) -> &str {
        "kafka"
    }

    fn is_critical(&self) -> bool {
        true
    }

    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        let timeout_duration = ctx.timeout.unwrap_or(Duration::from_secs(5));

        // Parse Kafka brokers (comma-separated host:port)
        let brokers: Vec<&str> = self.brokers.split(',').map(|s| s.trim()).collect();
        if brokers.is_empty() {
            return Err(HealthCheckError::CheckFailed(
                "No Kafka brokers specified".to_string(),
            ));
        }

        // Try to connect to at least one broker
        for broker in brokers {
            let (host, port) = parse_host_port(broker, 9092)?;
            match timeout(
                timeout_duration,
                tokio::net::TcpStream::connect(format!("{}:{}", host, port)),
            )
            .await
            {
                Ok(Ok(_stream)) => return Ok(()),
                Ok(Err(_)) => continue, // Try next broker
                Err(_) => continue,      // Try next broker
            }
        }

        Err(HealthCheckError::CheckFailed(format!(
            "Failed to connect to any Kafka broker: {}",
            self.brokers
        )))
    }
}

/// Parse Redis URL (redis://host:port or host:port)
fn parse_redis_url(url: &str) -> Result<(String, u16), HealthCheckError> {
    let url = url.trim_start_matches("redis://");
    parse_host_port(url, 6379)
}

/// Parse PostgreSQL connection string (postgres://user:pass@host:port/db)
fn parse_postgres_url(conn_str: &str) -> Result<(String, u16), HealthCheckError> {
    // Remove postgres:// or postgresql:// prefix
    let conn_str = conn_str
        .trim_start_matches("postgres://")
        .trim_start_matches("postgresql://");

    // Extract host:port (before /)
    let parts: Vec<&str> = conn_str.split('/').collect();
    let host_port = parts.first().ok_or_else(|| {
        HealthCheckError::CheckFailed("Invalid PostgreSQL connection string".to_string())
    })?;

    // Remove user:pass@ if present
    let host_port = host_port.split('@').last().unwrap_or(host_port);

    parse_host_port(host_port, 5432)
}

/// Parse host:port string
fn parse_host_port(host_port: &str, default_port: u16) -> Result<(String, u16), HealthCheckError> {
    let parts: Vec<&str> = host_port.split(':').collect();
    match parts.len() {
        1 => Ok((parts[0].to_string(), default_port)),
        2 => {
            let port = parts[1].parse().map_err(|_| {
                HealthCheckError::CheckFailed(format!("Invalid port in '{}'", host_port))
            })?;
            Ok((parts[0].to_string(), port))
        }
        _ => Err(HealthCheckError::CheckFailed(format!(
            "Invalid host:port format: '{}'",
            host_port
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::system::v1::HealthProbeConfig;

    #[tokio::test]
    async fn test_dependency_checker() {
        let checker = DependencyHealthChecker {
            name: "test-service".to_string(),
            grpc_address: "127.0.0.1:8000".to_string(),
            is_critical: true,
        };

        let ctx = HealthCheckContext::default();
        // This will fail if 127.0.0.1:8000 is not listening, but that's OK for the test
        let _result = checker.check(&ctx).await;
        assert_eq!(checker.name(), "test-service");
        assert!(checker.is_critical());
    }

    #[test]
    fn test_parse_redis_url() {
        let (host, port) = parse_redis_url("redis://localhost:6379").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 6379);

        let (host, port) = parse_redis_url("localhost:6380").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 6380);

        let (host, port) = parse_redis_url("localhost").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 6379);
    }

    #[test]
    fn test_parse_postgres_url() {
        let (host, port) = parse_postgres_url("postgres://user:pass@localhost:5432/db").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5432);

        let (host, port) = parse_postgres_url("postgresql://localhost:5433/mydb").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5433);

        let (host, port) = parse_postgres_url("localhost").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 5432);
    }

    #[test]
    fn test_parse_host_port() {
        let (host, port) = parse_host_port("localhost:9092", 9092).unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 9092);

        let (host, port) = parse_host_port("kafka-broker", 9092).unwrap();
        assert_eq!(host, "kafka-broker");
        assert_eq!(port, 9092);
    }
}

