// SPDX-License-Identifier: AGPL-3.0-or-later
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
//! - `object_store_available()` - S3-compatible object store (BLOB_ENDPOINT or localhost:9000)

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
static OBJECT_STORE_AVAILABLE: OnceLock<tokio::sync::Mutex<Option<bool>>> = OnceLock::new();

/// Check if DynamoDB Local (or compatible) is running at the configured endpoint.
///
/// Sends a raw HTTP `POST` with the `X-Amz-Target: DynamoDB_20120810.ListTables` header and
/// checks that the response body contains DynamoDB-specific JSON (e.g. `"TableNames"` or
/// `"__type"`). A plain TCP probe is insufficient because other services (e.g. the node's HTTP
/// server) may be listening on the same port and produce false positives.
pub async fn dynamodb_local_available() -> bool {
    let endpoint = get_dynamodb_endpoint();
    let cache = DDB_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cache.lock().await;

    if let Some((ref cached_ep, available)) = *guard {
        if cached_ep == &endpoint {
            return available;
        }
    }

    let available = check_dynamodb_endpoint(&endpoint).await;
    *guard = Some((endpoint, available));
    available
}

/// Send a minimal DynamoDB `ListTables` HTTP request and verify the response looks like DynamoDB.
async fn check_dynamodb_endpoint(endpoint: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let tcp_addr = http_endpoint_to_tcp_addr(endpoint);
    let body = b"{\"Limit\":1}";
    let request = format!(
        "POST / HTTP/1.1\r\n\
         Host: {}\r\n\
         X-Amz-Target: DynamoDB_20120810.ListTables\r\n\
         Content-Type: application/x-amz-json-1.0\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        tcp_addr,
        body.len()
    );

    let mut stream = match timeout(Duration::from_millis(500), TcpStream::connect(&tcp_addr)).await
    {
        Ok(Ok(s)) => s,
        _ => return false,
    };

    if stream
        .write_all(request.as_bytes())
        .await
        .is_err()
        || stream.write_all(body).await.is_err()
    {
        return false;
    }

    let mut buf = vec![0u8; 512];
    let n = match timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
        Ok(Ok(n)) => n,
        _ => return false,
    };

    // DynamoDB Local responds with JSON containing "TableNames" (success) or "__type" (auth error).
    // Any non-DynamoDB service (HTML, gRPC, etc.) will not contain these strings.
    let response = std::str::from_utf8(&buf[..n]).unwrap_or("");
    response.contains("TableNames") || response.contains("__type") || response.contains("DynamoDB")
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
// Object Store Service Guard
// ============================================================================

/// Check if the configured S3-compatible object store is reachable at BLOB_ENDPOINT.
/// Uses a static cache to avoid checking for every test.
///
/// ## Environment Variables
/// - `BLOB_ENDPOINT`: Object store endpoint URL (default: http://localhost:9000)
pub async fn object_store_available() -> bool {
    let cache = OBJECT_STORE_AVAILABLE.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut cached = cache.lock().await;

    if let Some(available) = *cached {
        return available;
    }

    let addr = std::env::var("BLOB_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string())
        .replace("http://", "")
        .replace("https://", "");
    let available = check_tcp_port(&addr, Duration::from_millis(500)).await;
    *cached = Some(available);
    available
}

/// Get object store endpoint URL.
///
/// Returns `BLOB_ENDPOINT` if set, otherwise the default port used by the embedded object
/// store auto-started by the node (`http://127.0.0.1:9000`).
pub fn get_object_store_endpoint() -> String {
    std::env::var("BLOB_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string())
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
