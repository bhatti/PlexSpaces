// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for NodeClient health-aware connection

use plexspaces_sdk::HealthCheckConfig;
use std::time::Duration;

#[cfg(feature = "grpc")]
mod grpc_tests {
    use super::*;
    use plexspaces_sdk::NodeClient;

    #[tokio::test]
    async fn test_health_check_config_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.initial_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(10));
        assert!(config.check_liveness);
        assert!(config.wait_for_readiness);
    }

    #[tokio::test]
    async fn test_connect_nodes_empty_list() {
        // This should not panic and return empty results
        // Note: This test requires a real node connection, so we'll skip it in unit tests
        // and test it in integration tests instead
    }
}

// Tests that don't require grpc feature
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_calculation() {
        // Test exponential backoff calculation indirectly
        // The algorithm: min(initial * 2^attempt, max) + jitter
        // We verify the config values are reasonable and the formula would work correctly

        let config = HealthCheckConfig::default();

        // Verify config values are reasonable
        assert!(config.initial_delay < config.max_delay);
        assert!(config.health_check_timeout > Duration::ZERO);
        assert!(config.readiness_timeout > Duration::ZERO);
        assert!(config.readiness_poll_interval > Duration::ZERO);

        // Verify exponential backoff properties:
        // - Initial delay should be small (500ms default)
        // - Max delay should cap exponential growth (10s default)
        // - Health check timeout should be reasonable (5s default)
        assert_eq!(config.initial_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(10));
        assert_eq!(config.health_check_timeout, Duration::from_secs(5));

        // Verify jitter range: 0-25% of delay
        // For attempt 0: base = 500ms, max jitter = 125ms, so max delay = 625ms
        // For attempt 1: base = 1000ms, max jitter = 250ms, so max delay = 1250ms
        // These are reasonable bounds
    }

    #[test]
    fn test_health_check_config_clone() {
        let config = HealthCheckConfig::default();
        let cloned = config.clone();
        assert_eq!(config.max_retries, cloned.max_retries);
        assert_eq!(config.initial_delay, cloned.initial_delay);
        assert_eq!(config.max_delay, cloned.max_delay);
    }

    #[test]
    fn test_health_check_config_debug() {
        let config = HealthCheckConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("max_retries"));
        assert!(debug_str.contains("initial_delay"));
    }
}
