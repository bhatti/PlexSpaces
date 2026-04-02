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

//! Test helpers for integration tests with Docker services
//!
//! Provides utilities to check if Docker services (DynamoDB Local, LocalStack, NATS, Kafka, etc.)
//! are available before running integration tests.
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_common::test_helpers::*;
//!
//! #[tokio::test]
//! async fn test_with_redis() {
//!     skip_if_unavailable!(redis_available().await, "Redis");
//!     // ... test code that requires Redis
//! }
//! ```
//!
//! ## Available Guards
//! - `dynamodb_local_available()` - DynamoDB Local (localhost:8000)
//! - `localstack_available()` / `sqs_simulator_available()` - LocalStack (localhost:4566)
//! - `redis_available()` - Redis (localhost:6379)
//! - `nats_available()` - NATS (localhost:4222)
//! - `kafka_available()` - Kafka (localhost:9092)
//! - `postgres_available()` - PostgreSQL (localhost:5432)
//! - `firecracker_available()` - Firecracker binary + kernel + rootfs
//! - `minio_available()` - MinIO/S3 (localhost:9000)

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::timeout;

/// Macro to skip a test if a service is unavailable.
/// This provides a consistent way to handle service availability checks.
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_common::test_helpers::*;
///
/// #[tokio::test]
/// async fn test_with_redis() {
///     skip_if_unavailable!(redis_available().await, "Redis");
///     // ... test code
/// }
/// ```
#[macro_export]
macro_rules! skip_if_unavailable {
    ($guard:expr, $service:expr) => {
        if !$guard {
            eprintln!("Skipping test: {} not available", $service);
            return;
        }
    };
}

/// Check if Docker is available
pub fn docker_available() -> bool {
    Command::new("docker").arg("--version").output().is_ok()
}

/// Check if docker-compose is available (tries both `docker-compose` and `docker compose`)
pub fn docker_compose_available() -> bool {
    // Try docker-compose first (older versions)
    if Command::new("docker-compose")
        .arg("--version")
        .output()
        .is_ok()
    {
        return true;
    }
    // Try docker compose (newer versions)
    Command::new("docker")
        .args(&["compose", "version"])
        .output()
        .is_ok()
}

/// Static variables to cache service availability checks
/// This avoids checking services for every test, improving test performance
static DDB_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<(String, bool)>>> = OnceLock::new();
static SQS_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<(String, bool)>>> = OnceLock::new();
static REDIS_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();
static NATS_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();
static KAFKA_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();
static POSTGRES_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();
static MINIO_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();

/// Check if the configured DynamoDB endpoint accepts TCP connections (DynamoDB Local or compatible).
///
/// Uses the same URL resolution as [`get_dynamodb_endpoint`], caches per endpoint string, and
/// uses a short TCP connect so tests skip quickly when nothing is listening. Does not use HTTP
/// `GET /` because DynamoDB Local often responds with non-success statuses on arbitrary paths.
pub async fn dynamodb_local_available() -> bool {
    let endpoint = get_dynamodb_endpoint();
    let tcp_addr = http_endpoint_to_tcp_addr(&endpoint);
    let cache = DDB_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cache.lock().await;

    if let Some((ref cached_ep, available)) = *guard {
        if cached_ep == &endpoint {
            return available;
        }
    }

    let available = check_tcp_port(&tcp_addr, Duration::from_millis(500)).await;
    *guard = Some((endpoint, available));
    available
}

/// Check if the configured LocalStack / SQS endpoint accepts TCP connections.
///
/// TCP is enough to decide whether to run integration tests; avoids HTTP overhead and does not
/// depend on LocalStack's health JSON path. Cached per [`get_sqs_endpoint`] value.
pub async fn localstack_available() -> bool {
    let endpoint = get_sqs_endpoint();
    let tcp_addr = http_endpoint_to_tcp_addr(&endpoint);
    let cache = SQS_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cache.lock().await;

    if let Some((ref cached_ep, available)) = *guard {
        if cached_ep == &endpoint {
            return available;
        }
    }

    let available = check_tcp_port(&tcp_addr, Duration::from_millis(500)).await;
    *guard = Some((endpoint, available));
    available
}

/// Check if SQS simulator (LocalStack) is available
/// Alias for localstack_available() for clarity
pub async fn sqs_simulator_available() -> bool {
    localstack_available().await
}

/// Check if Redis is running on the default port (6379)
/// Uses a static cache to avoid checking for every test
/// Fast timeout (500ms) for quick failure when service is not available
pub async fn redis_available() -> bool {
    let cache = REDIS_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    // Fast async Redis check using TCP connection
    let available = check_redis_port("localhost:6379", Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Check if Redis is available by attempting a TCP connection
/// Fast timeout for quick failure when service is not available
async fn check_redis_port(addr: &str, timeout_duration: Duration) -> bool {
    use tokio::net::TcpStream;

    match timeout(timeout_duration, TcpStream::connect(addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

/// Get DynamoDB endpoint URL (from env or default)
pub fn get_dynamodb_endpoint() -> String {
    std::env::var("DYNAMODB_ENDPOINT_URL")
        .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
        .unwrap_or_else(|_| "http://localhost:8000".to_string())
}

/// Converts an HTTP(S) base URL to a `host:port` string for [`check_tcp_port`].
///
/// DynamoDB Local does not reliably return HTTP 2xx on `GET /`, so integration tests
/// use a short TCP connect to the configured endpoint instead of an HTTP health probe.
pub fn http_endpoint_to_tcp_addr(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    let lower = trimmed.to_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .unwrap_or(trimmed);
    let host_part = rest.split('/').next().unwrap_or(rest).trim();
    if host_part.is_empty() {
        return "localhost:8000".to_string();
    }
    if host_part.contains(':') {
        host_part.to_string()
    } else if lower.starts_with("https://") {
        format!("{host_part}:443")
    } else {
        format!("{host_part}:80")
    }
}

/// Get SQS endpoint URL (from env or default)
pub fn get_sqs_endpoint() -> String {
    std::env::var("SQS_ENDPOINT_URL")
        .or_else(|_| std::env::var("PLEXSPACES_SQS_ENDPOINT_URL"))
        .unwrap_or_else(|_| "http://localhost:4566".to_string())
}

/// Get S3 endpoint URL (from env or default)
pub fn get_s3_endpoint() -> String {
    std::env::var("S3_ENDPOINT_URL")
        .or_else(|_| std::env::var("PLEXSPACES_S3_ENDPOINT_URL"))
        .unwrap_or_else(|_| "http://localhost:4566".to_string())
}

/// Setup AWS environment variables for local testing
pub fn setup_aws_local_env() {
    std::env::set_var(
        "AWS_REGION",
        std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
    );
    std::env::set_var(
        "AWS_ACCESS_KEY_ID",
        std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "test".to_string()),
    );
    std::env::set_var(
        "AWS_SECRET_ACCESS_KEY",
        std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "test".to_string()),
    );

    // Avoid long hangs in `aws_config::load()` when no cloud metadata is reachable (CI / laptops).
    if std::env::var("AWS_EC2_METADATA_DISABLED").is_err() {
        std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
    }

    // Set endpoint URLs if not already set
    if std::env::var("DYNAMODB_ENDPOINT_URL").is_err() {
        std::env::set_var("DYNAMODB_ENDPOINT_URL", "http://localhost:8000");
    }
    if std::env::var("SQS_ENDPOINT_URL").is_err() {
        std::env::set_var("SQS_ENDPOINT_URL", "http://localhost:4566");
    }
    if std::env::var("S3_ENDPOINT_URL").is_err() {
        std::env::set_var("S3_ENDPOINT_URL", "http://localhost:4566");
    }
}

// ============================================================================
// NATS Service Guard
// ============================================================================

/// Check if NATS is running on the default port (4222)
/// Uses a static cache to avoid checking for every test
/// Fast timeout (500ms) for quick failure when service is not available
///
/// ## Environment Variables
/// - `NATS_URL`: Override default URL (default: nats://localhost:4222)
pub async fn nats_available() -> bool {
    let cache = NATS_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    // Check NATS by attempting TCP connection to default port
    let nats_host = std::env::var("NATS_URL")
        .unwrap_or_else(|_| "localhost:4222".to_string())
        .replace("nats://", "");
    let available = check_tcp_port(&nats_host, Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Get NATS URL (from env or default)
pub fn get_nats_url() -> String {
    std::env::var("NATS_URL")
        .or_else(|_| std::env::var("PLEXSPACES_NATS_URL"))
        .unwrap_or_else(|_| "nats://localhost:4222".to_string())
}

// ============================================================================
// Kafka Service Guard
// ============================================================================

/// Check if Kafka is running on the default port (9092)
/// Uses a static cache to avoid checking for every test
/// Fast timeout (500ms) for quick failure when service is not available
///
/// ## Environment Variables
/// - `KAFKA_BROKERS`: Override default brokers (default: localhost:9092)
pub async fn kafka_available() -> bool {
    let cache = KAFKA_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    // Check Kafka by attempting TCP connection to default port
    let kafka_host =
        std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".to_string());
    // Take only the first broker if multiple are specified
    let first_broker = kafka_host.split(',').next().unwrap_or("localhost:9092");
    let available = check_tcp_port(first_broker, Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Get Kafka brokers (from env or default)
pub fn get_kafka_brokers() -> String {
    std::env::var("KAFKA_BROKERS")
        .or_else(|_| std::env::var("PLEXSPACES_KAFKA_BROKERS"))
        .unwrap_or_else(|_| "localhost:9092".to_string())
}

// ============================================================================
// PostgreSQL Service Guard
// ============================================================================

/// Check if PostgreSQL is running on the default port (5432)
/// Uses a static cache to avoid checking for every test
/// Fast timeout (500ms) for quick failure when service is not available
///
/// ## Environment Variables
/// - `TEST_POSTGRES_URL`: Override default connection string
/// - `DATABASE_URL`: Alternative environment variable
pub async fn postgres_available() -> bool {
    let cache = POSTGRES_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    // Check PostgreSQL by attempting TCP connection to default port
    // Parse host:port from connection string if available
    let pg_addr = if let Ok(url) =
        std::env::var("TEST_POSTGRES_URL").or_else(|_| std::env::var("DATABASE_URL"))
    {
        // Try to extract host:port from postgresql://user:pass@host:port/db
        if let Some(at_pos) = url.find('@') {
            let after_at = &url[at_pos + 1..];
            if let Some(slash_pos) = after_at.find('/') {
                after_at[..slash_pos].to_string()
            } else {
                after_at.to_string()
            }
        } else {
            "localhost:5432".to_string()
        }
    } else {
        "localhost:5432".to_string()
    };

    let available = check_tcp_port(&pg_addr, Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Get PostgreSQL URL (from env or default)
pub fn get_postgres_url() -> String {
    std::env::var("TEST_POSTGRES_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .or_else(|_| std::env::var("PLEXSPACES_POSTGRES_URL"))
        .unwrap_or_else(|_| {
            "postgresql://postgres:postgres@localhost:5432/plexspaces_test".to_string()
        })
}

// ============================================================================
// MinIO/S3 Service Guard
// ============================================================================

/// Check if MinIO/S3 is running on the default port (9000)
/// Uses a static cache to avoid checking for every test
/// Fast timeout (500ms) for quick failure when service is not available
///
/// ## Environment Variables
/// - `MINIO_ENDPOINT`: Override default endpoint (default: http://localhost:9000)
pub async fn minio_available() -> bool {
    let cache = MINIO_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    // Check MinIO by attempting TCP connection to default port
    let minio_addr = std::env::var("MINIO_ENDPOINT")
        .unwrap_or_else(|_| "localhost:9000".to_string())
        .replace("http://", "")
        .replace("https://", "");
    let available = check_tcp_port(&minio_addr, Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Get MinIO endpoint URL (from env or default)
pub fn get_minio_endpoint() -> String {
    std::env::var("MINIO_ENDPOINT")
        .or_else(|_| std::env::var("PLEXSPACES_MINIO_ENDPOINT"))
        .unwrap_or_else(|_| "http://localhost:9000".to_string())
}

// ============================================================================
// Firecracker Service Guard
// ============================================================================

/// Check if Firecracker environment is available
/// This checks for:
/// 1. Firecracker binary (in PATH or /usr/bin/firecracker)
/// 2. Kernel image (/var/lib/firecracker/vmlinux)
/// 3. Root filesystem (/var/lib/firecracker/rootfs.ext4)
///
/// Note: This is a synchronous check (file system operations)
pub fn firecracker_available() -> bool {
    // Check Firecracker binary
    let has_binary = Path::new("/usr/bin/firecracker").exists()
        || Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok();

    if !has_binary {
        return false;
    }

    // Check kernel image
    let has_kernel = Path::new("/var/lib/firecracker/vmlinux").exists();
    if !has_kernel {
        return false;
    }

    // Check rootfs image
    let has_rootfs = Path::new("/var/lib/firecracker/rootfs.ext4").exists();
    if !has_rootfs {
        return false;
    }

    true
}

/// Get detailed error message for missing Firecracker prerequisites
pub fn firecracker_prerequisites_error() -> Option<String> {
    let mut errors = Vec::new();

    let has_binary = Path::new("/usr/bin/firecracker").exists()
        || Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok();
    if !has_binary {
        errors.push("Firecracker binary not found (install from: https://github.com/firecracker-microvm/firecracker/releases)");
    }

    if !Path::new("/var/lib/firecracker/vmlinux").exists() {
        errors.push("Kernel image not found at /var/lib/firecracker/vmlinux");
    }

    if !Path::new("/var/lib/firecracker/rootfs.ext4").exists() {
        errors.push("Rootfs image not found at /var/lib/firecracker/rootfs.ext4");
    }

    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

// ============================================================================
// Generic TCP Port Check Helper
// ============================================================================

/// Check if a TCP port is available by attempting a connection
/// Fast timeout for quick failure when service is not available
async fn check_tcp_port(addr: &str, timeout_duration: Duration) -> bool {
    use tokio::net::TcpStream;

    match timeout(timeout_duration, TcpStream::connect(addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod http_endpoint_parse_tests {
    use super::http_endpoint_to_tcp_addr;

    #[test]
    fn parses_http_with_port() {
        assert_eq!(
            http_endpoint_to_tcp_addr("http://localhost:8000"),
            "localhost:8000"
        );
    }

    #[test]
    fn parses_host_port_without_scheme() {
        assert_eq!(
            http_endpoint_to_tcp_addr("127.0.0.1:8000"),
            "127.0.0.1:8000"
        );
    }

    #[test]
    fn adds_default_http_port() {
        assert_eq!(
            http_endpoint_to_tcp_addr("http://ddb.local"),
            "ddb.local:80"
        );
    }

    #[test]
    fn adds_default_https_port() {
        assert_eq!(
            http_endpoint_to_tcp_addr("https://ddb.local"),
            "ddb.local:443"
        );
    }
}
