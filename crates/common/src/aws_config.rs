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

//! AWS configuration helpers.
//!
//! ## Purpose
//! Provides utilities for loading AWS configuration from environment variables
//! and config files with proper precedence and defaults.
//!
//! ## Configuration Precedence
//! 1. Environment variables (highest priority)
//! 2. Config file (YAML/TOML)
//! 3. Defaults (lowest priority)
//!
//! ## Environment Variables
//! - `AWS_REGION` - AWS region (e.g., "us-east-1")
//! - `AWS_ACCESS_KEY_ID` - AWS access key ID
//! - `AWS_SECRET_ACCESS_KEY` - AWS secret access key
//! - `AWS_SESSION_TOKEN` - AWS session token (for temporary credentials)
//! - `DYNAMODB_ENDPOINT_URL` - DynamoDB endpoint URL (for local testing)
//! - `SQS_ENDPOINT_URL` - SQS endpoint URL (for local testing)
//! - `S3_ENDPOINT_URL` - S3 endpoint URL (for local testing)

use std::env;

/// DynamoDB configuration.
#[derive(Debug, Clone)]
pub struct DynamoDBConfig {
    /// AWS region
    pub region: String,
    /// Table name prefix (default: "plexspaces-")
    pub table_prefix: String,
    /// Endpoint URL (for local testing)
    pub endpoint_url: Option<String>,
}

impl Default for DynamoDBConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            table_prefix: "plexspaces-".to_string(),
            endpoint_url: None,
        }
    }
}

impl DynamoDBConfig {
    /// Load DynamoDB configuration from environment variables.
    ///
    /// ## Environment Variables
    /// - `AWS_REGION` - AWS region (default: "us-east-1")
    /// - `DYNAMODB_ENDPOINT_URL` - Endpoint URL for local testing
    /// - `PLEXSPACES_DDB_TABLE_PREFIX` - Table name prefix (default: "plexspaces-")
    pub fn from_env() -> Self {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let table_prefix = env::var("PLEXSPACES_DDB_TABLE_PREFIX")
            .unwrap_or_else(|_| "plexspaces-".to_string());

        let endpoint_url = env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .ok()
            .filter(|s| !s.is_empty());

        Self {
            region,
            table_prefix,
            endpoint_url,
        }
    }

    /// Get full table name for a component.
    ///
    /// ## Arguments
    /// * `component_name` - Component name (e.g., "locks", "scheduler")
    ///
    /// ## Returns
    /// Full table name: `{table_prefix}{component_name}`
    pub fn table_name(&self, component_name: &str) -> String {
        format!("{}{}", self.table_prefix, component_name)
    }
}

/// SQS configuration.
#[derive(Debug, Clone)]
pub struct SQSConfig {
    /// AWS region
    pub region: String,
    /// Queue name prefix (default: "plexspaces-")
    pub queue_prefix: String,
    /// Endpoint URL (for local testing)
    pub endpoint_url: Option<String>,
    /// Visibility timeout in seconds (default: 30)
    pub visibility_timeout_seconds: u32,
    /// Message retention period in seconds (default: 345600 = 4 days)
    pub message_retention_period_seconds: u32,
    /// Dead Letter Queue configuration
    pub dlq: DLQConfig,
}

/// Dead Letter Queue configuration.
#[derive(Debug, Clone)]
pub struct DLQConfig {
    /// Enable DLQ (default: true)
    pub enabled: bool,
    /// Max receive count before sending to DLQ (default: 3)
    pub max_receive_count: u32,
}

impl Default for DLQConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_receive_count: 3,
        }
    }
}

impl Default for SQSConfig {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            queue_prefix: "plexspaces-".to_string(),
            endpoint_url: None,
            visibility_timeout_seconds: 30,
            message_retention_period_seconds: 345600, // 4 days
            dlq: DLQConfig::default(),
        }
    }
}

impl SQSConfig {
    /// Load SQS configuration from environment variables.
    ///
    /// ## Environment Variables
    /// - `AWS_REGION` - AWS region (default: "us-east-1")
    /// - `SQS_ENDPOINT_URL` - Endpoint URL for local testing
    /// - `PLEXSPACES_SQS_QUEUE_PREFIX` - Queue name prefix (default: "plexspaces-")
    /// - `PLEXSPACES_SQS_VISIBILITY_TIMEOUT` - Visibility timeout in seconds (default: 30)
    /// - `PLEXSPACES_SQS_MESSAGE_RETENTION` - Message retention in seconds (default: 345600)
    /// - `PLEXSPACES_SQS_DLQ_ENABLED` - Enable DLQ (default: true)
    /// - `PLEXSPACES_SQS_DLQ_MAX_RECEIVE_COUNT` - Max receive count (default: 3)
    pub fn from_env() -> Self {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let queue_prefix = env::var("PLEXSPACES_SQS_QUEUE_PREFIX")
            .unwrap_or_else(|_| "plexspaces-".to_string());

        let endpoint_url = env::var("SQS_ENDPOINT_URL")
            .or_else(|_| env::var("PLEXSPACES_SQS_ENDPOINT_URL"))
            .ok()
            .filter(|s| !s.is_empty());

        let visibility_timeout_seconds = env::var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30);

        let message_retention_period_seconds = env::var("PLEXSPACES_SQS_MESSAGE_RETENTION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(345600);

        let dlq_enabled = env::var("PLEXSPACES_SQS_DLQ_ENABLED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(true);

        let dlq_max_receive_count = env::var("PLEXSPACES_SQS_DLQ_MAX_RECEIVE_COUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);

        Self {
            region,
            queue_prefix,
            endpoint_url,
            visibility_timeout_seconds,
            message_retention_period_seconds,
            dlq: DLQConfig {
                enabled: dlq_enabled,
                max_receive_count: dlq_max_receive_count,
            },
        }
    }

    /// Get full queue name for a channel.
    ///
    /// ## Arguments
    /// * `channel_name` - Channel name
    ///
    /// ## Returns
    /// Full queue name: `{queue_prefix}{channel_name}`
    pub fn queue_name(&self, channel_name: &str) -> String {
        format!("{}{}", self.queue_prefix, channel_name)
    }

    /// Get Dead Letter Queue name for a channel.
    ///
    /// ## Arguments
    /// * `channel_name` - Channel name
    ///
    /// ## Returns
    /// DLQ name: `{queue_prefix}{channel_name}-dlq`
    pub fn dlq_name(&self, channel_name: &str) -> String {
        format!("{}{}-dlq", self.queue_prefix, channel_name)
    }
}

/// S3 configuration.
#[derive(Debug, Clone)]
pub struct S3Config {
    /// AWS region
    pub region: String,
    /// Bucket name
    pub bucket: String,
    /// Endpoint URL (for local testing)
    pub endpoint_url: Option<String>,
    /// Use path-style addressing (for MinIO and S3 Local)
    pub use_path_style: bool,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            region: "us-east-1".to_string(),
            bucket: "plexspaces".to_string(),
            endpoint_url: None,
            use_path_style: false,
        }
    }
}

impl S3Config {
    /// Load S3 configuration from environment variables.
    ///
    /// ## Environment Variables
    /// - `AWS_REGION` - AWS region (default: "us-east-1")
    /// - `S3_ENDPOINT_URL` - Endpoint URL for local testing
    /// - `PLEXSPACES_S3_BUCKET` - Bucket name (default: "plexspaces")
    /// - `PLEXSPACES_S3_USE_PATH_STYLE` - Use path-style addressing (default: false)
    pub fn from_env() -> Self {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let bucket = env::var("PLEXSPACES_S3_BUCKET")
            .or_else(|_| env::var("S3_BUCKET"))
            .unwrap_or_else(|_| "plexspaces".to_string());

        let endpoint_url = env::var("S3_ENDPOINT_URL")
            .or_else(|_| env::var("PLEXSPACES_S3_ENDPOINT_URL"))
            .ok()
            .filter(|s| !s.is_empty());

        let use_path_style = env::var("PLEXSPACES_S3_USE_PATH_STYLE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false);

        Self {
            region,
            bucket,
            endpoint_url,
            use_path_style,
        }
    }
}

/// AWS configuration helper.
///
/// Provides a unified way to load AWS configuration for all services.
#[derive(Debug, Clone)]
pub struct AWSConfig {
    /// DynamoDB configuration
    pub dynamodb: DynamoDBConfig,
    /// SQS configuration
    pub sqs: SQSConfig,
    /// S3 configuration
    pub s3: S3Config,
}

impl Default for AWSConfig {
    fn default() -> Self {
        Self {
            dynamodb: DynamoDBConfig::default(),
            sqs: SQSConfig::default(),
            s3: S3Config::default(),
        }
    }
}

impl AWSConfig {
    /// Load AWS configuration from environment variables.
    ///
    /// ## Environment Variables
    /// See individual config structs for details:
    /// - `DynamoDBConfig::from_env()`
    /// - `SQSConfig::from_env()`
    /// - `S3Config::from_env()`
    pub fn from_env() -> Self {
        // Use same region for all services
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let mut dynamodb = DynamoDBConfig::from_env();
        if dynamodb.region != region {
            dynamodb.region = region.clone();
        }

        let mut sqs = SQSConfig::from_env();
        if sqs.region != region {
            sqs.region = region.clone();
        }

        let mut s3 = S3Config::from_env();
        if s3.region != region {
            s3.region = region.clone();
        }

        Self {
            dynamodb,
            sqs,
            s3,
        }
    }

    /// Check if AWS is enabled (region is set and not empty).
    pub fn is_enabled(&self) -> bool {
        !self.dynamodb.region.is_empty()
    }

    /// Check if AWS should be used as default backend.
    ///
    /// ## Logic
    /// Returns true if:
    /// - AWS_REGION is set (not empty)
    /// - PLEXSPACES_AWS_ENABLED is not explicitly set to "false"
    ///
    /// ## Usage
    /// When AWS is enabled, components should default to AWS backends:
    /// - locks: DynamoDB
    /// - scheduler: DynamoDB
    /// - keyvalue: DynamoDB
    /// - channel: SQS
    /// - workflow: DynamoDB
    /// - journaling: DynamoDB
    /// - blob: DynamoDB (metadata) + S3 (storage)
    /// - tuplespace: DynamoDB
    pub fn should_use_as_default() -> bool {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_default();

        if region.is_empty() {
            return false;
        }

        // Check if explicitly disabled
        let aws_enabled = env::var("PLEXSPACES_AWS_ENABLED")
            .unwrap_or_else(|_| "true".to_string())
            .to_lowercase();

        aws_enabled != "false"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamodb_config_defaults() {
        let config = DynamoDBConfig::default();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.table_prefix, "plexspaces-");
        assert_eq!(config.table_name("locks"), "plexspaces-locks");
    }

    #[test]
    fn test_sqs_config_defaults() {
        let config = SQSConfig::default();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.queue_prefix, "plexspaces-");
        assert_eq!(config.queue_name("my-channel"), "plexspaces-my-channel");
        assert_eq!(config.dlq_name("my-channel"), "plexspaces-my-channel-dlq");
    }

    #[test]
    fn test_s3_config_defaults() {
        let config = S3Config::default();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.bucket, "plexspaces");
    }
}

