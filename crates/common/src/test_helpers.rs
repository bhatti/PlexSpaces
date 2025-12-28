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
//! Provides utilities to check if Docker services (DynamoDB Local, LocalStack)
//! are available before running integration tests.

use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

/// Check if Docker is available
pub fn docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .is_ok()
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

/// Check if DynamoDB Local is running on the default port
pub async fn dynamodb_local_available() -> bool {
    check_service_health("http://localhost:8000", Duration::from_secs(2)).await
}

/// Check if LocalStack is running on the default port
pub async fn localstack_available() -> bool {
    check_service_health("http://localhost:4566/_localstack/health", Duration::from_secs(2)).await
}

/// Check if a service is available by making an HTTP request
async fn check_service_health(url: &str, timeout_duration: Duration) -> bool {
    let client = match reqwest::Client::builder()
        .timeout(timeout_duration)
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match timeout(timeout_duration, client.get(url).send()).await {
        Ok(Ok(resp)) => resp.status().is_success(),
        _ => false,
    }
}

/// Get DynamoDB endpoint URL (from env or default)
pub fn get_dynamodb_endpoint() -> String {
    std::env::var("DYNAMODB_ENDPOINT_URL")
        .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
        .unwrap_or_else(|_| "http://localhost:8000".to_string())
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
    std::env::set_var("AWS_REGION", std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()));
    std::env::set_var("AWS_ACCESS_KEY_ID", std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "test".to_string()));
    std::env::set_var("AWS_SECRET_ACCESS_KEY", std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "test".to_string()));
    
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

