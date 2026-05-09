// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! AWS configuration extension traits.
//!
//! ## Purpose
//! Extension traits that add Rust-specific behavior (from_env, helper methods)
//! to proto-generated AWS config types.
//!
//! ## Design
//! - Proto types (`AwsConfig`, `DynamoDbConfig`, `SqsConfig`, `S3Config`) are the data contract
//! - Extension traits add behavior: environment loading, name helpers
//!
//! ## Configuration Precedence
//! 1. Environment variables (highest priority)
//! 2. Defaults (lowest priority)
//!
//! ## Environment Variables
//! - `AWS_REGION` / `PLEXSPACES_AWS_REGION` - AWS region (e.g., "us-east-1")
//! - `DYNAMODB_ENDPOINT_URL` / `PLEXSPACES_DDB_ENDPOINT_URL` - DynamoDB endpoint URL
//! - `SQS_ENDPOINT_URL` / `PLEXSPACES_SQS_ENDPOINT_URL` - SQS endpoint URL
//! - `S3_ENDPOINT_URL` / `PLEXSPACES_S3_ENDPOINT_URL` - S3 endpoint URL

use std::env;

pub use plexspaces_proto::config::v1::{AwsConfig, DlqConfig, DynamoDbConfig, S3Config, SqsConfig};

// Type aliases preserving backwards-compatible names
/// Alias for [`AwsConfig`] (proto-generated type).
pub type AWSConfig = AwsConfig;
/// Alias for [`SqsConfig`] (proto-generated type).
pub type SQSConfig = SqsConfig;
/// Alias for [`DlqConfig`] (proto-generated type).
pub type DLQConfig = DlqConfig;
/// Alias for [`DynamoDbConfig`] (proto-generated type).
pub type DynamoDBConfig = DynamoDbConfig;

/// Extension trait for [`AwsConfig`] with environment loading and utility methods.
pub trait AwsConfigExt {
    /// Load unified AWS configuration from environment variables.
    ///
    /// Reads `AWS_REGION` (or `PLEXSPACES_AWS_REGION`) and applies the same
    /// region to all sub-configs to ensure consistency.
    fn from_env() -> AwsConfig;

    /// Returns true if AWS is enabled (region is non-empty).
    fn is_enabled(&self) -> bool;

    /// Returns true if AWS should be used as the default backend.
    ///
    /// True when `AWS_REGION` is set and `PLEXSPACES_AWS_ENABLED` is not `"false"`.
    fn should_use_as_default() -> bool;
}

/// Extension trait for [`DynamoDbConfig`] with environment loading and name helpers.
pub trait DynamoDbConfigExt {
    /// Load DynamoDB configuration from environment variables.
    fn from_env() -> DynamoDbConfig;

    /// Get full table name: `{table_prefix}{component}`.
    fn table_name(&self, component: &str) -> String;
}

/// Extension trait for [`SqsConfig`] with environment loading and name helpers.
pub trait SqsConfigExt {
    /// Load SQS configuration from environment variables.
    fn from_env() -> SqsConfig;

    /// Get full queue name: `{queue_prefix}{name}`.
    fn queue_name(&self, name: &str) -> String;

    /// Get DLQ name: `{queue_prefix}{name}-dlq`.
    fn dlq_name(&self, name: &str) -> String;
}

/// Extension trait for [`S3Config`] with environment loading and key helpers.
pub trait S3ConfigExt {
    /// Load S3 configuration from environment variables.
    fn from_env() -> S3Config;

    /// Get object key, ensuring a leading `/`.
    fn object_key(&self, path: &str) -> String;
}

impl DynamoDbConfigExt for DynamoDbConfig {
    fn from_env() -> DynamoDbConfig {
        DynamoDbConfig {
            region: env::var("AWS_REGION")
                .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
            table_prefix: env::var("PLEXSPACES_DDB_TABLE_PREFIX")
                .unwrap_or_else(|_| "plexspaces-".to_string()),
            endpoint_url: env::var("DYNAMODB_ENDPOINT_URL")
                .or_else(|_| env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
                .unwrap_or_default(),
        }
    }

    fn table_name(&self, component: &str) -> String {
        format!("{}{}", self.table_prefix, component)
    }
}

impl SqsConfigExt for SqsConfig {
    fn from_env() -> SqsConfig {
        let enable_dlq = env::var("PLEXSPACES_SQS_DLQ_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true);
        let max_receive = env::var("PLEXSPACES_SQS_DLQ_MAX_RECEIVE_COUNT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(3);
        SqsConfig {
            region: env::var("AWS_REGION")
                .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
            queue_prefix: env::var("PLEXSPACES_SQS_QUEUE_PREFIX")
                .unwrap_or_else(|_| "plexspaces-".to_string()),
            endpoint_url: env::var("SQS_ENDPOINT_URL")
                .or_else(|_| env::var("PLEXSPACES_SQS_ENDPOINT_URL"))
                .unwrap_or_default(),
            visibility_timeout_seconds: env::var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            message_retention_seconds: env::var("PLEXSPACES_SQS_MESSAGE_RETENTION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(86400),
            dlq: Some(DlqConfig {
                enabled: enable_dlq,
                max_receive_count: max_receive,
            }),
            receive_message_wait_time_seconds: env::var("PLEXSPACES_SQS_RECEIVE_WAIT_TIME")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
        }
    }

    fn queue_name(&self, name: &str) -> String {
        format!("{}{}", self.queue_prefix, name)
    }

    fn dlq_name(&self, name: &str) -> String {
        format!("{}{}-dlq", self.queue_prefix, name)
    }
}

impl S3ConfigExt for S3Config {
    fn from_env() -> S3Config {
        S3Config {
            region: env::var("AWS_REGION")
                .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
            bucket: env::var("PLEXSPACES_S3_BUCKET")
                .or_else(|_| env::var("S3_BUCKET"))
                .unwrap_or_else(|_| "plexspaces".to_string()),
            endpoint_url: env::var("S3_ENDPOINT_URL")
                .or_else(|_| env::var("PLEXSPACES_S3_ENDPOINT_URL"))
                .unwrap_or_default(),
            use_path_style: env::var("PLEXSPACES_S3_USE_PATH_STYLE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
        }
    }

    fn object_key(&self, path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{}", path)
        }
    }
}

impl AwsConfigExt for AwsConfig {
    fn from_env() -> AwsConfig {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());

        let mut dynamodb = DynamoDbConfig::from_env();
        if dynamodb.region != region {
            dynamodb.region = region.clone();
        }
        let mut sqs = SqsConfig::from_env();
        if sqs.region != region {
            sqs.region = region.clone();
        }
        let mut s3 = S3Config::from_env();
        if s3.region != region {
            s3.region = region.clone();
        }

        AwsConfig {
            dynamodb: Some(dynamodb),
            sqs: Some(sqs),
            s3: Some(s3),
        }
    }

    fn is_enabled(&self) -> bool {
        self.dynamodb
            .as_ref()
            .map(|d| !d.region.is_empty())
            .unwrap_or(false)
    }

    fn should_use_as_default() -> bool {
        let region = env::var("AWS_REGION")
            .or_else(|_| env::var("PLEXSPACES_AWS_REGION"))
            .unwrap_or_default();

        if region.is_empty() {
            return false;
        }

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
        let config = DynamoDbConfig::from_env();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.table_prefix, "plexspaces-");
        assert_eq!(config.table_name("locks"), "plexspaces-locks");
    }

    #[test]
    fn test_sqs_config_defaults() {
        let config = SqsConfig::from_env();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.queue_prefix, "plexspaces-");
        assert_eq!(config.queue_name("my-channel"), "plexspaces-my-channel");
        assert_eq!(config.dlq_name("my-channel"), "plexspaces-my-channel-dlq");
    }

    #[test]
    fn test_s3_config_defaults() {
        let config = S3Config::from_env();
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.bucket, "plexspaces");
    }

    #[test]
    fn test_aws_config_from_env() {
        let config = AwsConfig::from_env();
        assert!(config.dynamodb.is_some());
        assert!(config.sqs.is_some());
        assert!(config.s3.is_some());
    }

    #[test]
    fn test_s3_object_key() {
        let config = S3Config::from_env();
        assert_eq!(config.object_key("foo/bar"), "/foo/bar");
        assert_eq!(config.object_key("/foo/bar"), "/foo/bar");
    }
}
