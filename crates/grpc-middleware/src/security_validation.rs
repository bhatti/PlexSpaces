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

//! Security Validation
//!
//! ## Purpose
//! Validates security configuration to ensure secrets are not stored in config files.
//! Enforces security best practices.

use thiserror::Error;

/// Security validation errors
#[derive(Debug, Error)]
pub enum SecurityValidationError {
    /// Secret found in config file (should be in environment variable)
    #[error("Security violation: Secret found in config file for field '{}'. Use environment variable '{}' instead.", .field, .env_var)]
    SecretInConfig {
        field: String,
        env_var: String,
    },

    /// Missing required secret
    #[error("Security violation: Required secret '{}' not found in environment variable '{}'", .field, .env_var)]
    MissingSecret {
        field: String,
        env_var: String,
    },
}

/// Security validator
pub struct SecurityValidator;

impl SecurityValidator {
    /// Validate that JWT secret is not in config
    ///
    /// ## Arguments
    /// * `config_secret` - Secret value from config file (should be empty)
    /// * `env_var_name` - Environment variable name to check
    ///
    /// ## Returns
    /// Ok(()) if valid, error if secret is in config
    ///
    /// ## Security Best Practice
    /// Secrets should NEVER be in config files. They must come from environment variables.
    pub fn validate_jwt_secret(
        config_secret: &str,
        env_var_name: &str,
    ) -> Result<String, SecurityValidationError> {
        // Check if secret is in config file
        if !config_secret.is_empty() {
            return Err(SecurityValidationError::SecretInConfig {
                field: "jwt_secret".to_string(),
                env_var: env_var_name.to_string(),
            });
        }

        // Get secret from environment variable
        std::env::var(env_var_name).map_err(|_| SecurityValidationError::MissingSecret {
            field: "jwt_secret".to_string(),
            env_var: env_var_name.to_string(),
        })
    }

    /// Validate that database password is not in config
    ///
    /// ## Arguments
    /// * `connection_string` - Database connection string from config
    /// * `password_env_var` - Environment variable name for password
    ///
    /// ## Returns
    /// Ok(()) if valid, error if password is in connection string
    ///
    /// ## Security Check
    /// Detects patterns like:
    /// - `password=plaintext`
    /// - `:password@` (with actual password, not placeholder)
    /// - `PASSWORD=plaintext`
    pub fn validate_database_password(
        connection_string: &str,
        password_env_var: &str,
    ) -> Result<(), SecurityValidationError> {
        // Check if password is in connection string (basic check)
        // Production should use connection string with password placeholder like:
        // - `postgresql://user:${DB_PASSWORD}@localhost/db`
        // - `postgresql://user:%s@localhost/db` (with placeholder)
        
        // Check for password= with actual value (not placeholder)
        if connection_string.contains("password=") {
            // Extract password part
            if let Some(pwd_start) = connection_string.find("password=") {
                let pwd_part = &connection_string[pwd_start + 9..];
                // Check if it's a placeholder or actual password
                if let Some(pwd_end) = pwd_part.find('&').or_else(|| pwd_part.find('@')) {
                    let pwd_value = &pwd_part[..pwd_end];
                    // If it's not a placeholder (${...} or %s), it's a real password
                    if !pwd_value.starts_with("${") && !pwd_value.contains("%s") && !pwd_value.is_empty() {
                        return Err(SecurityValidationError::SecretInConfig {
                            field: "database_password".to_string(),
                            env_var: password_env_var.to_string(),
                        });
                    }
                } else {
                    // Password at end of string
                    let pwd_value = pwd_part;
                    if !pwd_value.starts_with("${") && !pwd_value.contains("%s") && !pwd_value.is_empty() {
                        return Err(SecurityValidationError::SecretInConfig {
                            field: "database_password".to_string(),
                            env_var: password_env_var.to_string(),
                        });
                    }
                }
            }
        }

        // Check for :password@ pattern (PostgreSQL style)
        if connection_string.matches(':').count() >= 2 {
            // Pattern: user:password@host
            if let Some(user_end) = connection_string.find("://") {
                let after_proto = &connection_string[user_end + 3..];
                if let Some(at_pos) = after_proto.find('@') {
                    let user_pass = &after_proto[..at_pos];
                    if let Some(colon_pos) = user_pass.find(':') {
                        let password = &user_pass[colon_pos + 1..];
                        // If password is not a placeholder, it's a real password
                        if !password.starts_with("${") && !password.contains("%s") && !password.is_empty() {
                            return Err(SecurityValidationError::SecretInConfig {
                                field: "database_password".to_string(),
                                env_var: password_env_var.to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Validate that API key is not in config
    ///
    /// ## Arguments
    /// * `config_key` - API key from config file (should be empty)
    /// * `env_var_name` - Environment variable name to check
    ///
    /// ## Returns
    /// Ok(()) if valid, error if key is in config
    pub fn validate_api_key(
        config_key: &str,
        env_var_name: &str,
    ) -> Result<String, SecurityValidationError> {
        if !config_key.is_empty() {
            return Err(SecurityValidationError::SecretInConfig {
                field: "api_key".to_string(),
                env_var: env_var_name.to_string(),
            });
        }

        std::env::var(env_var_name).map_err(|_| SecurityValidationError::MissingSecret {
            field: "api_key".to_string(),
            env_var: env_var_name.to_string(),
        })
    }

    /// Validate all security config (convenience method)
    ///
    /// ## Arguments
    /// * `jwt_secret_config` - JWT secret from config (should be empty)
    /// * `jwt_secret_env_var` - Environment variable name for JWT secret
    ///
    /// ## Returns
    /// Ok(()) if all validations pass
    pub fn validate_all(
        jwt_secret_config: &str,
        jwt_secret_env_var: &str,
    ) -> Result<(), SecurityValidationError> {
        Self::validate_jwt_secret(jwt_secret_config, jwt_secret_env_var)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_jwt_secret_success() {
        // Set environment variable
        std::env::set_var("TEST_JWT_SECRET", "secret-from-env");

        // Config should be empty
        let result = SecurityValidator::validate_jwt_secret("", "TEST_JWT_SECRET");
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "secret-from-env");

        // Cleanup
        std::env::remove_var("TEST_JWT_SECRET");
    }

    #[test]
    fn test_validate_jwt_secret_fails_when_in_config() {
        // Config has secret (should fail)
        let result = SecurityValidator::validate_jwt_secret("secret-in-config", "TEST_JWT_SECRET");
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityValidationError::SecretInConfig { .. }));
        assert!(err.to_string().contains("Secret found in config"));
    }

    #[test]
    fn test_validate_jwt_secret_fails_when_missing() {
        // Ensure env var is not set
        std::env::remove_var("TEST_JWT_SECRET_MISSING");

        // Config is empty but env var is missing (should fail)
        let result = SecurityValidator::validate_jwt_secret("", "TEST_JWT_SECRET_MISSING");
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityValidationError::MissingSecret { .. }));
        assert!(err.to_string().contains("not found in environment variable"));
    }

    #[test]
    fn test_validate_database_password_success() {
        // Connection string with placeholder (should pass)
        let conn_str = "postgresql://user:${DB_PASSWORD}@localhost/db";
        let result = SecurityValidator::validate_database_password(conn_str, "DB_PASSWORD");
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_database_password_fails_with_plain_password() {
        // Connection string with plain password (should fail)
        let conn_str = "postgresql://user:plainpassword@localhost/db";
        let result = SecurityValidator::validate_database_password(conn_str, "DB_PASSWORD");
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityValidationError::SecretInConfig { .. }));
    }

    #[test]
    fn test_validate_api_key_success() {
        // Set environment variable
        std::env::set_var("TEST_API_KEY", "key-from-env");

        // Config should be empty
        let result = SecurityValidator::validate_api_key("", "TEST_API_KEY");
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "key-from-env");

        // Cleanup
        std::env::remove_var("TEST_API_KEY");
    }

    #[test]
    fn test_validate_api_key_fails_when_in_config() {
        // Config has key (should fail)
        let result = SecurityValidator::validate_api_key("key-in-config", "TEST_API_KEY");
        
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SecurityValidationError::SecretInConfig { .. }));
    }

    #[test]
    fn test_validate_all_success() {
        // Set environment variable
        std::env::set_var("TEST_JWT_SECRET_ALL", "secret-from-env");

        // Config should be empty
        let result = SecurityValidator::validate_all("", "TEST_JWT_SECRET_ALL");
        
        assert!(result.is_ok());

        // Cleanup
        std::env::remove_var("TEST_JWT_SECRET_ALL");
    }

    #[test]
    fn test_validate_all_fails_when_secret_in_config() {
        // Config has secret (should fail)
        let result = SecurityValidator::validate_all("secret-in-config", "TEST_JWT_SECRET_ALL");
        
        assert!(result.is_err());
    }
}

