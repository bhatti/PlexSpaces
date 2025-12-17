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

//! Config Loader with Environment Variable Precedence and Security Validation
//!
//! ## Purpose
//! Loads ReleaseSpec from YAML files with:
//! - Environment variable substitution (${VAR_NAME} or ${VAR_NAME:-default})
//! - Environment variable precedence over file config
//! - Security validation (secrets must be in env vars, not config files)
//!
//! ## Design Principles
//! 1. **Env Var Precedence**: Environment variables override file config
//! 2. **Security First**: Secrets must be in env vars, validated at load time
//! 3. **Flexible**: Supports defaults in env var syntax
//! 4. **Production-Ready**: Comprehensive error handling and validation

use plexspaces_proto::node::v1::ReleaseSpec;
use regex::Regex;
use std::env;
use thiserror::Error;

#[path = "config_loader_yaml.rs"]
mod config_loader_yaml;
#[path = "config_loader_convert.rs"]
mod config_loader_convert;

use config_loader_yaml::ReleaseYaml;
use config_loader_convert::convert_yaml_to_proto;

/// Config loader errors
#[derive(Debug, Error)]
pub enum ConfigLoaderError {
    /// File I/O error
    #[error("Failed to read config file '{path}': {source}")]
    IoError {
        path: String,
        source: std::io::Error,
    },
    /// YAML parsing error
    #[error("Failed to parse YAML: {0}")]
    YamlError(#[from] serde_yaml::Error),
    /// Security validation error
    #[error("Security validation failed: {0}")]
    SecurityError(String),
    /// Environment variable substitution error
    #[error("Environment variable substitution failed: {0}")]
    EnvSubstitutionError(String),
}

/// Config loader with environment variable precedence and security validation
pub struct ConfigLoader {
    /// Whether to validate security (default: true)
    validate_security: bool,
}

impl ConfigLoader {
    /// Create a new config loader
    pub fn new() -> Self {
        Self {
            validate_security: true,
        }
    }

    /// Create a config loader with security validation disabled (for testing)
    pub fn without_security_validation() -> Self {
        Self {
            validate_security: false,
        }
    }

    /// Load ReleaseSpec from YAML file with environment variable substitution
    ///
    /// ## Arguments
    /// * `path` - Path to YAML config file
    ///
    /// ## Returns
    /// Parsed ReleaseSpec with env vars substituted
    ///
    /// ## Errors
    /// - `ConfigLoaderError::IoError` if file cannot be read
    /// - `ConfigLoaderError::YamlError` if YAML parsing fails
    /// - `ConfigLoaderError::SecurityError` if security validation fails
    pub async fn load_release_spec(&self, path: &str) -> Result<ReleaseSpec, ConfigLoaderError> {
        // Read file
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ConfigLoaderError::IoError {
                path: path.to_string(),
                source: e,
            })?;

        // Validate security in original YAML content (before substitution)
        // This ensures we catch hardcoded secrets in the config file
        if self.validate_security {
            self.validate_security_in_yaml(&content)?;
        }

        // Substitute environment variables
        let substituted = self.substitute_env_vars(&content)?;

        // Parse YAML into intermediate representation
        let yaml_release: ReleaseYaml = serde_yaml::from_str(&substituted)?;

        // Convert to proto ReleaseSpec
        let mut spec = convert_yaml_to_proto(yaml_release)
            .map_err(|e| ConfigLoaderError::EnvSubstitutionError(e))?;

        Ok(spec)
    }

    /// Load ReleaseSpec with environment variable precedence
    ///
    /// ## Purpose
    /// Loads config from file, then applies environment variable overrides.
    /// Environment variables take precedence over file config.
    ///
    /// ## Environment Variable Naming
    /// Uses dot notation for nested fields:
    /// - `PLEXSPACES_NODE_ID` → `node.id`
    /// - `PLEXSPACES_RUNTIME_GRPC_ADDRESS` → `runtime.grpc.address`
    pub async fn load_release_spec_with_env_precedence(
        &self,
        path: &str,
    ) -> Result<ReleaseSpec, ConfigLoaderError> {
        // Load base config from file
        let mut spec = self.load_release_spec(path).await?;

        // Apply environment variable overrides
        self.apply_env_overrides(&mut spec)?;

        Ok(spec)
    }

    /// Substitute environment variables in YAML content
    ///
    /// Supports:
    /// - `${VAR_NAME}` - Required env var
    /// - `${VAR_NAME:-default}` - Optional env var with default
    fn substitute_env_vars(&self, content: &str) -> Result<String, ConfigLoaderError> {
        // Regex to match ${VAR_NAME} or ${VAR_NAME:-default}
        let re = Regex::new(r"\$\{([^}:]+)(?::-([^}]+))?\}").unwrap();
        
        let mut result = content.to_string();
        let mut replacements = Vec::new();

        for cap in re.captures_iter(content) {
            let var_name = cap.get(1).unwrap().as_str();
            let default = cap.get(2).map(|m| m.as_str());

            let value = if let Ok(env_value) = env::var(var_name) {
                env_value
            } else if let Some(default_value) = default {
                default_value.to_string()
            } else {
                return Err(ConfigLoaderError::EnvSubstitutionError(format!(
                    "Environment variable '{}' is not set and no default provided",
                    var_name
                )));
            };

            let full_match = cap.get(0).unwrap().as_str();
            replacements.push((full_match.to_string(), value));
        }

        // Apply replacements
        for (pattern, replacement) in replacements {
            result = result.replace(&pattern, &replacement);
        }

        Ok(result)
    }

    /// Apply environment variable overrides to ReleaseSpec
    ///
    /// Environment variables override file config values.
    fn apply_env_overrides(&self, spec: &mut ReleaseSpec) -> Result<(), ConfigLoaderError> {
        // Node ID override
        if let Ok(node_id) = env::var("PLEXSPACES_NODE_ID") {
            if let Some(ref mut node) = spec.node {
                node.id = node_id;
            }
        }

        // Node listen address override
        if let Ok(listen_addr) = env::var("PLEXSPACES_LISTEN_ADDR") {
            if let Some(ref mut node) = spec.node {
                node.listen_address = listen_addr;
            }
        }

        // gRPC address override
        if let Ok(grpc_addr) = env::var("PLEXSPACES_GRPC_ADDRESS") {
            if let Some(ref mut runtime) = spec.runtime {
                if let Some(ref mut grpc) = runtime.grpc {
                    grpc.address = grpc_addr;
                }
            }
        }

        // TODO: Add more overrides as needed
        // This is a simplified implementation - in production, use a more generic approach

        Ok(())
    }

    /// Validate security configuration in YAML content (before substitution)
    ///
    /// Ensures secrets are not hardcoded in config files.
    /// Checks the original YAML content for hardcoded secrets.
    fn validate_security_in_yaml(&self, content: &str) -> Result<(), ConfigLoaderError> {
        // Parse YAML to check for hardcoded secrets
        // We'll do a simple string check for common secret patterns
        let re = Regex::new(r#"(?i)(secret|password|key|token)\s*:\s*["']([^"']+)["']"#).unwrap();
        
        for cap in re.captures_iter(content) {
            let field_name = cap.get(1).unwrap().as_str().to_lowercase();
            let value = cap.get(2).unwrap().as_str();
            
            // Skip if it's an env var reference
            if value.starts_with("${") && value.ends_with("}") {
                continue;
            }
            
            // Check if it looks like a hardcoded secret (not empty, not a path, not a URL)
            if !value.is_empty() 
                && !value.starts_with("/") 
                && !value.starts_with("http://") 
                && !value.starts_with("https://")
                && value.len() > 5  // Likely a secret if longer than 5 chars
            {
                // Check if it's in the security section
                if content.contains("security:") || content.contains("jwt_config:") || content.contains("authn_config:") {
                    return Err(ConfigLoaderError::SecurityError(
                        format!("Hardcoded {} found in security configuration. Use environment variable reference (e.g., ${{{}}}) instead.", 
                            field_name, field_name.to_uppercase())
                    ));
                }
            }
        }
        
        Ok(())
    }

    /// Validate security configuration in parsed spec (after substitution)
    ///
    /// This is a fallback check, but primary validation should be in YAML.
    fn validate_security(&self, spec: &ReleaseSpec) -> Result<(), ConfigLoaderError> {
        if let Some(ref runtime) = spec.runtime {
            if let Some(ref security) = runtime.security {
                // Check JWT config (direct in SecurityConfig)
                if let Some(ref jwt) = security.jwt {
                    // Validate JWT secret
                    if !jwt.secret.is_empty() {
                        // Check if it's an env var reference
                        if !jwt.secret.starts_with("${") || !jwt.secret.ends_with("}") {
                            return Err(ConfigLoaderError::SecurityError(
                                "JWT secret must be an environment variable reference (e.g., ${JWT_SECRET}), not a hardcoded value".to_string(),
                            ));
                        }
                    }
                }

                // Check mTLS config
                if let Some(ref mtls) = security.mtls {
                    // Validate mTLS cert paths (should be env vars or file paths, not hardcoded secrets)
                    // For now, we just check that they're not empty if mTLS is enabled
                    if mtls.enable_mtls && !mtls.auto_generate {
                        if mtls.ca_certificate_path.is_empty() || mtls.server_certificate_path.is_empty() || mtls.server_key_path.is_empty() {
                            return Err(ConfigLoaderError::SecurityError(
                                "mTLS is enabled but certificate paths are not configured".to_string(),
                            ));
                        }
                    }
                }

            }
        }

        Ok(())
    }
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_substitute_env_vars_simple() {
        env::set_var("TEST_VAR", "test-value");
        let loader = ConfigLoader::new();
        let content = "value: ${TEST_VAR}";
        let result = loader.substitute_env_vars(content).unwrap();
        assert_eq!(result, "value: test-value");
        env::remove_var("TEST_VAR");
    }

    #[test]
    fn test_substitute_env_vars_with_default() {
        env::remove_var("MISSING_VAR");
        let loader = ConfigLoader::new();
        let content = "value: ${MISSING_VAR:-default-value}";
        let result = loader.substitute_env_vars(content).unwrap();
        assert_eq!(result, "value: default-value");
    }

    #[test]
    fn test_substitute_env_vars_missing_required() {
        env::remove_var("REQUIRED_VAR");
        let loader = ConfigLoader::new();
        let content = "value: ${REQUIRED_VAR}";
        let result = loader.substitute_env_vars(content);
        assert!(result.is_err());
    }
}

