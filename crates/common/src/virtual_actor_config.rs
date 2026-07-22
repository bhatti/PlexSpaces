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

//! Virtual Actor Configuration Helpers
//!
//! ## Purpose
//! Provides helper functions to extract default virtual actor configuration
//! from `RuntimeConfig.default_virtual_actor_config` and apply defaults
//! when configuration is not provided.
//!
//! ## Default Values
//! When `default_virtual_actor_config` is `None` or fields are not set:
//! - `idle_timeout`: 5 minutes (300 seconds)
//! - `max_pool_per_actor_type`: 100
//! - `activation_strategy`: LAZY

use crate::ActivationStrategy;
use plexspaces_proto::node::v1::DefaultVirtualActorConfig;
use prost_types::Duration as ProtoDuration;
use std::time::Duration;

/// Default idle timeout: 5 minutes
pub const DEFAULT_IDLE_TIMEOUT_SECONDS: u64 = 300;

/// Default max pool size per actor type: 100
pub const DEFAULT_MAX_POOL_PER_ACTOR_TYPE: u32 = 100;

/// Default activation strategy: LAZY
pub const DEFAULT_ACTIVATION_STRATEGY: ActivationStrategy =
    ActivationStrategy::ActivationStrategyLazy;

/// Get idle timeout from DefaultVirtualActorConfig, applying defaults
///
/// ## Arguments
/// * `config` - Optional DefaultVirtualActorConfig from RuntimeConfig
///
/// ## Returns
/// Duration for idle timeout (defaults to 5 minutes if not set)
pub fn get_idle_timeout(config: Option<&DefaultVirtualActorConfig>) -> Duration {
    config
        .and_then(|c| c.idle_timeout.as_ref())
        .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
        .unwrap_or(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS))
}

/// Get max pool size per actor type from DefaultVirtualActorConfig, applying defaults
///
/// ## Arguments
/// * `config` - Optional DefaultVirtualActorConfig from RuntimeConfig
///
/// ## Returns
/// Max pool size (defaults to 100 if not set or 0)
pub fn get_max_pool_per_actor_type(config: Option<&DefaultVirtualActorConfig>) -> u32 {
    config
        .and_then(|c| {
            if c.max_pool_per_actor_type > 0 {
                Some(c.max_pool_per_actor_type)
            } else {
                None
            }
        })
        .unwrap_or(DEFAULT_MAX_POOL_PER_ACTOR_TYPE)
}

/// Get activation strategy from DefaultVirtualActorConfig, applying defaults
///
/// ## Arguments
/// * `config` - Optional DefaultVirtualActorConfig from RuntimeConfig
///
/// ## Returns
/// ActivationStrategy (defaults to LAZY if not set or UNSPECIFIED)
pub fn get_activation_strategy(config: Option<&DefaultVirtualActorConfig>) -> ActivationStrategy {
    config
        .and_then(|c| ActivationStrategy::try_from(c.activation_strategy).ok())
        .filter(|s| *s != ActivationStrategy::ActivationStrategyUnspecified)
        .unwrap_or(DEFAULT_ACTIVATION_STRATEGY)
}

/// Convert Duration to ProtoDuration
pub fn duration_to_proto(d: Duration) -> ProtoDuration {
    ProtoDuration {
        seconds: d.as_secs() as i64,
        nanos: (d.subsec_nanos() as i32),
    }
}

/// Format Duration as string (e.g., "5m", "300s", "1h")
///
/// ## Arguments
/// * `d` - Duration to format
///
/// ## Returns
/// String representation (e.g., "5m" for 300 seconds, "1h" for 3600 seconds)
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs > 0 && secs.is_multiple_of(3600) {
        format!("{}h", secs / 3600)
    } else if secs > 0 && secs.is_multiple_of(60) {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Duration as ProtoDuration;

    #[test]
    fn test_get_idle_timeout_with_config() {
        let config = Some(DefaultVirtualActorConfig {
            idle_timeout: Some(ProtoDuration {
                seconds: 600, // 10 minutes
                nanos: 0,
            }),
            max_pool_per_actor_type: 0,
            activation_strategy: 0,
        });

        let timeout = get_idle_timeout(config.as_ref());
        assert_eq!(timeout, Duration::from_secs(600));
    }

    #[test]
    fn test_get_idle_timeout_default() {
        let timeout = get_idle_timeout(None);
        assert_eq!(timeout, Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS));
    }

    #[test]
    fn test_get_max_pool_with_config() {
        let config = Some(DefaultVirtualActorConfig {
            idle_timeout: None,
            max_pool_per_actor_type: 200,
            activation_strategy: 0,
        });

        let pool_size = get_max_pool_per_actor_type(config.as_ref());
        assert_eq!(pool_size, 200);
    }

    #[test]
    fn test_get_max_pool_default() {
        let pool_size = get_max_pool_per_actor_type(None);
        assert_eq!(pool_size, DEFAULT_MAX_POOL_PER_ACTOR_TYPE);
    }

    #[test]
    fn test_get_max_pool_zero_uses_default() {
        let config = Some(DefaultVirtualActorConfig {
            idle_timeout: None,
            max_pool_per_actor_type: 0, // Invalid, should use default
            activation_strategy: 0,
        });

        let pool_size = get_max_pool_per_actor_type(config.as_ref());
        assert_eq!(pool_size, DEFAULT_MAX_POOL_PER_ACTOR_TYPE);
    }

    #[test]
    fn test_get_activation_strategy_with_config() {
        let config = Some(DefaultVirtualActorConfig {
            idle_timeout: None,
            max_pool_per_actor_type: 0,
            activation_strategy: ActivationStrategy::ActivationStrategyEager as i32,
        });

        let strategy = get_activation_strategy(config.as_ref());
        assert_eq!(strategy, ActivationStrategy::ActivationStrategyEager);
    }

    #[test]
    fn test_get_activation_strategy_default() {
        let strategy = get_activation_strategy(None);
        assert_eq!(strategy, DEFAULT_ACTIVATION_STRATEGY);
    }

    #[test]
    fn test_get_activation_strategy_unspecified_uses_default() {
        let config = Some(DefaultVirtualActorConfig {
            idle_timeout: None,
            max_pool_per_actor_type: 0,
            activation_strategy: ActivationStrategy::ActivationStrategyUnspecified as i32,
        });

        let strategy = get_activation_strategy(config.as_ref());
        assert_eq!(strategy, DEFAULT_ACTIVATION_STRATEGY);
    }
}
