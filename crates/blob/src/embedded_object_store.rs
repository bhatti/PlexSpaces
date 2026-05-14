// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Embedded S3-compatible object store process manager.
//!
//! # Purpose
//! Starts a local S3-compatible object store subprocess when no external blob endpoint
//! is configured, providing out-of-the-box storage with no external dependencies.
//!
//! # Configuring the implementation
//! Set `EMBEDDED_OBJECT_STORE_BIN` to the path of your preferred S3-compatible server binary.
//! By default uses `rustfs` (https://github.com/rustfs/rustfs), a pure-Rust implementation.
//! Any S3-compatible server binary that accepts `server --address HOST:PORT --data DIR` can be used.
//!
//! # Usage
//! ```rust,no_run
//! use plexspaces_blob::embedded_object_store::EmbeddedObjectStore;
//!
//! # async fn example() -> Result<(), plexspaces_blob::BlobError> {
//! let store = EmbeddedObjectStore::start(9100).await?;
//! println!("S3 endpoint: {}", store.s3_endpoint);
//! // store keeps the subprocess alive; drop it to stop the process
//! # Ok(())
//! # }
//! ```

use std::time::Duration;
use tokio::process::Child;
use tokio::time::{sleep, timeout};

use crate::BlobError;

/// Default binary name for the embedded object store implementation.
pub const DEFAULT_EMBEDDED_BIN: &str = "rustfs";

/// Environment variable to override the embedded object store binary path.
pub const EMBEDDED_BIN_ENV: &str = "EMBEDDED_OBJECT_STORE_BIN";

/// Manages an embedded S3-compatible object store subprocess.
///
/// Spawns the configured server binary with an S3 API endpoint on `s3_port`.
/// Data is stored in a temporary directory that is cleaned up when the manager is dropped.
/// The subprocess is killed when this struct is dropped.
pub struct EmbeddedObjectStore {
    process: Child,
    /// S3-compatible endpoint URL, e.g. "http://127.0.0.1:9100"
    pub s3_endpoint: String,
    /// Access key for S3 API (empty string = no-auth / anonymous access)
    pub access_key: String,
    /// Secret key for S3 API (empty string = no-auth / anonymous access)
    pub secret_key: String,
    _data_dir: tempfile::TempDir,
}

impl EmbeddedObjectStore {
    /// Start an embedded S3-compatible object store on `s3_port`.
    ///
    /// Looks for the server binary via [`EMBEDDED_BIN_ENV`] env var or PATH.
    /// Polls readiness by sending HTTP requests to the S3 port; returns an error
    /// if the binary is missing or the process does not become ready within 15 seconds.
    pub async fn start(s3_port: u16) -> Result<Self, BlobError> {
        let bin =
            std::env::var(EMBEDDED_BIN_ENV).unwrap_or_else(|_| DEFAULT_EMBEDDED_BIN.to_string());

        if which::which(&bin).is_err() {
            return Err(BlobError::ConfigError(format!(
                "Embedded object store binary '{}' not found in PATH. \
                Install via: cargo install rustfs  |  \
                download from https://github.com/rustfs/rustfs/releases. \
                Or set {} env var to the binary path.",
                bin, EMBEDDED_BIN_ENV
            )));
        }

        let data_dir = tempfile::TempDir::new().map_err(|e| {
            BlobError::ConfigError(format!(
                "Failed to create embedded object store temp dir: {}",
                e
            ))
        })?;

        tracing::info!(
            bin = %bin,
            port = s3_port,
            data_dir = %data_dir.path().display(),
            "Starting embedded object store"
        );

        let process = tokio::process::Command::new(&bin)
            .arg("server")
            .arg("--address")
            .arg(format!("127.0.0.1:{}", s3_port))
            .arg("--data")
            .arg(data_dir.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                BlobError::ConfigError(format!(
                    "Failed to spawn embedded object store process '{}': {}",
                    bin, e
                ))
            })?;

        let s3_endpoint = format!("http://127.0.0.1:{}", s3_port);

        Self::wait_until_ready(&s3_endpoint).await?;

        tracing::info!(endpoint = %s3_endpoint, "Embedded object store is ready");

        Ok(Self {
            process,
            s3_endpoint,
            access_key: String::new(),
            secret_key: String::new(),
            _data_dir: data_dir,
        })
    }

    /// Poll the S3 endpoint until it responds or the 15-second deadline is exceeded.
    /// Retries on connection-refused; fails fast on other errors after first attempt.
    async fn wait_until_ready(s3_endpoint: &str) -> Result<(), BlobError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .no_proxy()
            .build()
            .map_err(|e| BlobError::ConfigError(format!("Failed to build HTTP client: {}", e)))?;

        let health_url = format!("{}/", s3_endpoint);
        let deadline_secs: u64 = std::env::var("EMBEDDED_OBJECT_STORE_READY_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15);

        let ready = timeout(Duration::from_secs(deadline_secs), async {
            loop {
                match client.get(&health_url).send().await {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        // Only retry on connection errors; other errors (e.g. bad config) fail fast.
                        if !e.is_connect() && !e.is_timeout() {
                            return Err(e);
                        }
                        tracing::debug!(error = %e, endpoint = %s3_endpoint, "Waiting for embedded object store to start");
                        sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        })
        .await;

        match ready {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(BlobError::ConfigError(format!(
                "Embedded object store at {} returned an unexpected error: {}",
                s3_endpoint, e
            ))),
            Err(_elapsed) => Err(BlobError::ConfigError(format!(
                "Embedded object store did not become ready within {}s at {}",
                deadline_secs, s3_endpoint
            ))),
        }
    }
}

impl Drop for EmbeddedObjectStore {
    fn drop(&mut self) {
        tracing::info!(endpoint = %self.s3_endpoint, "Stopping embedded object store");
        let _ = self.process.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_binary_missing_returns_error() {
        // Override binary path to a non-existent path.
        // We restore the original value after the test to avoid polluting parallel tests.
        let original = std::env::var(EMBEDDED_BIN_ENV).ok();
        std::env::set_var(EMBEDDED_BIN_ENV, "/nonexistent/object_store_binary_xyzzy");
        let result = EmbeddedObjectStore::start(19000).await;
        match original {
            Some(v) => std::env::set_var(EMBEDDED_BIN_ENV, v),
            None => std::env::remove_var(EMBEDDED_BIN_ENV),
        }
        match result {
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("object store") || msg.contains("not found"),
                    "unexpected error: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected error when binary is missing"),
        }
    }
}
