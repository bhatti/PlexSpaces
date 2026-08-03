// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Environment helpers for tests
//!
//! Single canonical copy of AWS local environment setup, previously duplicated
//! in `common/src/test_helpers.rs` and `actor/src/test_helpers.rs`.

/// Configure environment variables for AWS local testing (LocalStack / DynamoDB Local).
///
/// Sets sensible defaults for AWS credentials and endpoint URLs when not already set.
/// Safe to call multiple times — only sets variables that aren't already present.
pub fn setup_aws_local_env() {
    // Credentials — use existing env if already set (e.g. in real CI)
    if std::env::var("AWS_REGION").is_err() {
        std::env::set_var("AWS_REGION", "us-east-1");
    }
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        std::env::set_var("AWS_ACCESS_KEY_ID", "test");
    }
    if std::env::var("AWS_SECRET_ACCESS_KEY").is_err() {
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "test");
    }

    // Avoid long hangs in `aws_config::load()` when no cloud metadata is reachable.
    if std::env::var("AWS_EC2_METADATA_DISABLED").is_err() {
        std::env::set_var("AWS_EC2_METADATA_DISABLED", "true");
    }

    // Endpoint URLs
    if std::env::var("DYNAMODB_ENDPOINT_URL").is_err() {
        std::env::set_var("DYNAMODB_ENDPOINT_URL", "http://localhost:8000");
    }
    if std::env::var("SQS_ENDPOINT_URL").is_err() {
        std::env::set_var("SQS_ENDPOINT_URL", "http://localhost:4566");
    }
    if std::env::var("AWS_ENDPOINT_URL").is_err() {
        std::env::set_var("AWS_ENDPOINT_URL", "http://localhost:4566");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_aws_local_env_is_idempotent() {
        setup_aws_local_env();
        setup_aws_local_env(); // second call must not panic or overwrite
        assert_eq!(std::env::var("AWS_EC2_METADATA_DISABLED").unwrap(), "true");
    }
}
