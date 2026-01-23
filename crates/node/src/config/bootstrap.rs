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

//! Configuration Bootstrap (Erlang/OTP-inspired)
//!
//! ## Purpose
//! Provides Erlang/OTP-style configuration loading with environment variable overrides.
//! Reduces boilerplate in examples and applications.
//!
//! ## Design
//! - Load from release.toml (application config)
//! - Override with environment variables (12-factor app)
//! - Fallback to Default trait
//!
//! ## Precedence (Highest to Lowest)
//! 1. Environment variables
//! 2. release.toml (if exists)
//! 3. Default::default()

use serde::de::DeserializeOwned;
use std::env;
use std::fs;
use std::path::Path;
use toml;

/// Configuration loading error
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    
    #[error("Environment variable error: {0}")]
    Env(String),
    
    /// Configuration not found
    ///
    /// ## Context
    /// The requested configuration file or key was not found.
    /// Check that the file exists and the path is correct.
    ///
    /// ## Suggestions
    /// - Verify the configuration file path
    /// - Check that environment variables are set correctly
    /// - Ensure the configuration key exists in the file
    #[error("Config not found: {0}. Hint: Check that the file exists and the path is correct.")]
    NotFound(String),
}

/// Configuration bootstrap helper
///
/// ## Usage
/// ```rust
/// #[derive(Deserialize, Default)]
/// struct MyConfig {
///     port: u16,
///     host: String,
/// }
///
/// let config: MyConfig = ConfigBootstrap::load().unwrap_or_default();
/// ```
pub struct ConfigBootstrap;

impl ConfigBootstrap {
    /// Load configuration with precedence: env > release.toml > defaults
    ///
    /// ## Precedence
    /// 1. Environment variables (highest priority)
    /// 2. release.toml (if exists in current directory or APP_CONFIG_PATH)
    /// 3. Default::default() (lowest priority)
    ///
    /// ## Arguments
    /// * `T` - Type that implements DeserializeOwned and Default
    ///
    /// ## Returns
    /// Loaded config or error
    pub fn load<T: DeserializeOwned + Default>() -> Result<T, ConfigError> {
        // Try environment variables first (12-factor app)
        if let Ok(config) = Self::load_from_env::<T>() {
            return Ok(config);
        }
        
        // Try release.toml
        if let Ok(config) = Self::load_from_release_toml::<T>() {
            return Ok(config);
        }
        
        // Fallback to defaults
        Ok(T::default())
    }
    
    /// Load configuration from release.toml
    ///
    /// ## File Locations (checked in order)
    /// 1. Path from APP_CONFIG_PATH environment variable
    /// 2. ./release.toml (current directory)
    /// 3. ../release.toml (parent directory)
    ///
    /// ## Returns
    /// Config or error if file not found or invalid
    pub fn load_from_release_toml<T: DeserializeOwned>() -> Result<T, ConfigError> {
        // Check APP_CONFIG_PATH first
        let config_path = env::var("APP_CONFIG_PATH")
            .ok()
            .or_else(|| {
                // Try current directory
                if Path::new("release.toml").exists() {
                    Some("release.toml".to_string())
                } else if Path::new("../release.toml").exists() {
                    Some("../release.toml".to_string())
                } else {
                    None
                }
            });
        
        if let Some(path) = config_path {
            Self::load_from_file::<T>(&path)
        } else {
            Err(ConfigError::NotFound("release.toml not found".to_string()))
        }
    }
    
    /// Load configuration from specific file
    ///
    /// ## Arguments
    /// * `path` - Path to TOML config file
    ///
    /// ## Returns
    /// Parsed config or error
    pub fn load_from_file<T: DeserializeOwned>(path: &str) -> Result<T, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config: T = toml::from_str(&content)?;
        Ok(config)
    }
    
    /// Load configuration from environment variables
    ///
    /// ## How It Works
    /// Uses `config` crate to load from environment variables with automatic type conversion.
    /// Environment variables are converted to config keys by:
    /// - Converting to lowercase
    /// - Replacing `_` with `.` (nested keys)
    /// - Using `__` for literal underscores
    ///
    /// ## Examples
    /// - `HTTP_PORT=8080` → `http.port = 8080`
    /// - `DB_HOST=localhost` → `db.host = "localhost"`
    /// - `DB_POOL_SIZE=20` → `db.pool_size = 20`
    /// - `ENABLE_METRICS=true` → `enable_metrics = true`
    ///
    /// ## Returns
    /// Config or error if required env vars missing
    pub fn load_from_env<T: DeserializeOwned>() -> Result<T, ConfigError> {
        use config::{Config, Environment};
        
        // Build config from environment variables
        // Environment variables are automatically converted to config keys
        let config = Config::builder()
            .add_source(Environment::with_prefix("")
                .separator("__")  // Use __ for literal underscores
                .try_parsing(true)  // Try to parse values (numbers, booleans, etc.)
                .list_separator(",")  // Comma-separated lists
            )
            .build()
            .map_err(|e| ConfigError::Env(format!("Failed to load from environment: {}", e)))?;
        
        config.try_deserialize::<T>()
            .map_err(|e| ConfigError::Env(format!("Failed to deserialize config: {}", e)))
    }
    
    /// Load configuration with environment variable prefix
    ///
    /// ## Arguments
    /// * `prefix` - Prefix for environment variables (e.g., "MY_APP" for `MY_APP_PORT`)
    ///
    /// ## Examples
    /// ```rust
    /// // Load from MY_APP_PORT, MY_APP_HOST, etc.
    /// let config = ConfigBootstrap::load_from_env_with_prefix::<MyConfig>("MY_APP")?;
    /// ```
    pub fn load_from_env_with_prefix<T: DeserializeOwned>(prefix: &str) -> Result<T, ConfigError> {
        use config::{Config, Environment};
        
        let config = Config::builder()
            .add_source(Environment::with_prefix(prefix)
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
            )
            .build()
            .map_err(|e| ConfigError::Env(format!("Failed to load from environment with prefix {}: {}", prefix, e)))?;
        
        config.try_deserialize::<T>()
            .map_err(|e| ConfigError::Env(format!("Failed to deserialize config: {}", e)))
    }
    
    /// Load configuration with explicit precedence
    ///
    /// ## Arguments
    /// * `file_path` - Optional path to config file
    /// * `use_env` - Whether to check environment variables
    /// * `use_defaults` - Whether to fallback to Default
    ///
    /// ## Returns
    /// Config loaded according to specified precedence
    pub fn load_with_options<T: DeserializeOwned + Default>(
        file_path: Option<&str>,
        use_env: bool,
        use_defaults: bool,
    ) -> Result<T, ConfigError> {
        // 1. Try file if provided
        if let Some(path) = file_path {
            if let Ok(config) = Self::load_from_file::<T>(path) {
                return Ok(config);
            }
        }
        
        // 2. Try environment if enabled
        if use_env {
            if let Ok(config) = Self::load_from_env::<T>() {
                return Ok(config);
            }
        }
        
        // 3. Try release.toml
        if let Ok(config) = Self::load_from_release_toml::<T>() {
            return Ok(config);
        }
        
        // 4. Use defaults if enabled
        if use_defaults {
            Ok(T::default())
        } else {
            Err(ConfigError::NotFound("No config found and defaults disabled".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Default, PartialEq)]
    struct TestConfig {
        port: u16,
        host: String,
    }

    #[test]
    fn test_load_defaults() {
        let config: TestConfig = ConfigBootstrap::load().unwrap();
        assert_eq!(config, TestConfig::default());
    }

    #[test]
    fn test_load_from_file() {
        // Create temporary config file
        let temp_file = std::env::temp_dir().join("test_config.toml");
        fs::write(&temp_file, "port = 8080\nhost = \"localhost\"").unwrap();
        
        let config: TestConfig = ConfigBootstrap::load_from_file(
            temp_file.to_str().unwrap()
        ).unwrap();
        
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "localhost");
        
        // Cleanup
        fs::remove_file(&temp_file).ok();
    }
}

