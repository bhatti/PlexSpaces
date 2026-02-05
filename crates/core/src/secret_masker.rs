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

//! Secret Masking Utilities
//!
//! ## Purpose
//! Provides utilities for masking sensitive data (passwords, API keys, tokens, etc.)
//! before exposing configurations via APIs. This prevents credential leakage in logs,
//! API responses, and error messages.
//!
//! ## Design Principles
//! - **Zero-Copy Where Possible**: Uses string replacement only when needed
//! - **Configurable**: Supports custom masking patterns and masks
//! - **Production-Ready**: Thread-safe, no panics, comprehensive error handling
//! - **Extensible**: Easy to add new secret field patterns
//!
//! ## Usage
//! ```rust,ignore
//! use plexspaces_core::secret_masker::{SecretMasker, mask_release_spec};
//!
//! // Create a masker with default patterns
//! let masker = SecretMasker::default();
//!
//! // Mask a string
//! let masked = masker.mask("my-secret-password");
//!
//! // Mask a ReleaseSpec
//! let masked_spec = mask_release_spec(release_spec);
//! ```

use std::collections::HashSet;

/// Default mask string used to replace sensitive values
pub const DEFAULT_MASK: &str = "***REDACTED***";

/// Default patterns for identifying secret field names
const DEFAULT_SECRET_PATTERNS: &[&str] = &[
    "password",
    "secret",
    "token",
    "key",
    "credential",
    "auth",
    "private",
    "api_key",
    "apikey",
    "access_key",
    "accesskey",
    "secret_key",
    "secretkey",
    "bearer",
    "jwt",
    "certificate",
    "cert",
    "private_key",
    "privatekey",
];

/// Secret field masker for configuration objects
///
/// ## Thread Safety
/// SecretMasker is `Send + Sync` and can be safely shared across threads.
///
/// ## Performance
/// - Field pattern matching is O(n * m) where n = patterns, m = field name length
/// - For typical configs with <100 fields, performance is negligible
/// - Consider caching masked configs for high-frequency access
#[derive(Debug, Clone)]
pub struct SecretMasker {
    /// Patterns that identify secret fields (case-insensitive substring match)
    secret_patterns: HashSet<String>,
    /// The mask string to use for replacement
    mask: String,
}

impl Default for SecretMasker {
    fn default() -> Self {
        Self::new(
            DEFAULT_SECRET_PATTERNS.iter().map(|s| s.to_string()).collect(),
            DEFAULT_MASK.to_string(),
        )
    }
}

impl SecretMasker {
    /// Create a new SecretMasker with custom patterns and mask
    ///
    /// ## Arguments
    /// * `secret_patterns` - Patterns to match against field names (case-insensitive)
    /// * `mask` - The string to replace secret values with
    pub fn new(secret_patterns: Vec<String>, mask: String) -> Self {
        Self {
            secret_patterns: secret_patterns.into_iter().map(|s| s.to_lowercase()).collect(),
            mask,
        }
    }

    /// Check if a field name matches any secret pattern
    ///
    /// ## Arguments
    /// * `field_name` - The field name to check (case-insensitive)
    ///
    /// ## Returns
    /// `true` if the field name contains any secret pattern
    pub fn is_secret_field(&self, field_name: &str) -> bool {
        let lower = field_name.to_lowercase();
        self.secret_patterns.iter().any(|pattern| lower.contains(pattern))
    }

    /// Mask a value if it's considered secret
    ///
    /// ## Arguments
    /// * `value` - The value to potentially mask
    ///
    /// ## Returns
    /// The mask string if value is non-empty, empty string if value is empty
    pub fn mask(&self, value: &str) -> String {
        if value.is_empty() {
            String::new()
        } else {
            self.mask.clone()
        }
    }

    /// Mask a field value if the field name matches a secret pattern
    ///
    /// ## Arguments
    /// * `field_name` - The field name to check
    /// * `value` - The value to potentially mask
    ///
    /// ## Returns
    /// Masked value if field is secret, original value otherwise
    pub fn mask_field(&self, field_name: &str, value: &str) -> String {
        if self.is_secret_field(field_name) {
            self.mask(value)
        } else {
            value.to_string()
        }
    }

    /// Get the mask string
    pub fn mask_string(&self) -> &str {
        &self.mask
    }
}

/// Mask secrets in a ReleaseSpec
///
/// ## Purpose
/// Returns a copy of the ReleaseSpec with sensitive fields masked.
/// This should be used before returning configs via APIs.
///
/// ## Masked Fields
/// - `runtime.blob.secret_access_key`
/// - `runtime.blob.access_key_id`
/// - `runtime.blob.azure_account_key`
/// - `runtime.blob.gcp_service_account_json`
/// - `runtime.security.api_keys[*].key_hash` (already hashed, but mask anyway)
/// - `runtime.security.jwt.secret`
/// - `runtime.security.service_identity.private_key`
/// - `runtime.db.connection_string` (password portion)
///
/// ## Arguments
/// * `spec` - The ReleaseSpec to mask
///
/// ## Returns
/// A new ReleaseSpec with secrets masked
pub fn mask_release_spec(spec: plexspaces_proto::node::v1::ReleaseSpec) -> plexspaces_proto::node::v1::ReleaseSpec {
    let mut masked = spec;
    
    // Mask runtime config secrets
    if let Some(ref mut runtime) = masked.runtime {
        // Mask blob storage secrets
        if let Some(ref mut blob) = runtime.blob {
            if !blob.secret_access_key.is_empty() {
                blob.secret_access_key = DEFAULT_MASK.to_string();
            }
            if !blob.access_key_id.is_empty() {
                blob.access_key_id = DEFAULT_MASK.to_string();
            }
            if !blob.azure_account_key.is_empty() {
                blob.azure_account_key = DEFAULT_MASK.to_string();
            }
            if !blob.gcp_service_account_json.is_empty() {
                blob.gcp_service_account_json = DEFAULT_MASK.to_string();
            }
        }
        
        // Mask security config secrets
        if let Some(ref mut security) = runtime.security {
            // Mask API key hashes (while they're hashes, we still mask them for extra security)
            for api_key in &mut security.api_keys {
                if !api_key.key_hash.is_empty() {
                    api_key.key_hash = DEFAULT_MASK.to_string();
                }
            }
            
            // Mask JWT secrets
            if let Some(ref mut jwt) = security.jwt {
                if !jwt.secret.is_empty() {
                    jwt.secret = DEFAULT_MASK.to_string();
                }
            }
            
            // Mask service identity secrets (private_key is bytes)
            if let Some(ref mut identity) = security.service_identity {
                if !identity.private_key.is_empty() {
                    identity.private_key = DEFAULT_MASK.as_bytes().to_vec();
                }
            }
        }
        
        // Mask database passwords in db config
        if let Some(ref mut db_config) = runtime.db {
            mask_database_url(&mut db_config.connection_string);
        }
    }
    
    masked
}

/// Mask password in a database URL
///
/// ## Purpose
/// Replaces password in database connection URLs with mask.
///
/// ## Supported Formats
/// - `postgres://user:password@host:port/db`
/// - `postgresql://user:password@host:port/db`
/// - `mysql://user:password@host:port/db`
/// - `sqlite://path` (no masking needed)
fn mask_database_url(url: &mut String) {
    if url.is_empty() || url.starts_with("sqlite:") {
        return;
    }
    
    // Parse URL to find and mask password
    // Format: scheme://user:password@host:port/path
    if let Some(at_pos) = url.find('@') {
        if let Some(scheme_end) = url.find("://") {
            let credentials_start = scheme_end + 3;
            let credentials = &url[credentials_start..at_pos];
            
            if let Some(colon_pos) = credentials.find(':') {
                let user = &credentials[..colon_pos];
                let scheme = &url[..scheme_end];
                let rest = &url[at_pos..];
                
                *url = format!("{}://{}:{}{}",scheme, user, DEFAULT_MASK, rest);
            }
        }
    }
}

/// Mask secrets in a map of string values
///
/// ## Purpose
/// Masks values in a HashMap where keys match secret patterns.
///
/// ## Arguments
/// * `map` - Mutable reference to the map
/// * `masker` - The SecretMasker to use
pub fn mask_map_secrets(
    map: &mut std::collections::HashMap<String, String>,
    masker: &SecretMasker,
) {
    for (key, value) in map.iter_mut() {
        if masker.is_secret_field(key) && !value.is_empty() {
            *value = masker.mask_string().to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_masker() {
        let masker = SecretMasker::default();
        
        // Should match secret patterns
        assert!(masker.is_secret_field("password"));
        assert!(masker.is_secret_field("secret_access_key"));
        assert!(masker.is_secret_field("api_key"));
        assert!(masker.is_secret_field("JWT_TOKEN"));
        assert!(masker.is_secret_field("my_private_key"));
        
        // Should not match non-secret patterns
        assert!(!masker.is_secret_field("username"));
        assert!(!masker.is_secret_field("email"));
        assert!(!masker.is_secret_field("host"));
        assert!(!masker.is_secret_field("port"));
    }

    #[test]
    fn test_mask_value() {
        let masker = SecretMasker::default();
        
        // Non-empty value should be masked
        assert_eq!(masker.mask("my-secret"), DEFAULT_MASK);
        
        // Empty value should remain empty
        assert_eq!(masker.mask(""), "");
    }

    #[test]
    fn test_mask_field() {
        let masker = SecretMasker::default();
        
        // Secret field should be masked
        assert_eq!(masker.mask_field("password", "secret123"), DEFAULT_MASK);
        assert_eq!(masker.mask_field("api_key", "key123"), DEFAULT_MASK);
        
        // Non-secret field should not be masked
        assert_eq!(masker.mask_field("username", "john"), "john");
        assert_eq!(masker.mask_field("host", "localhost"), "localhost");
    }

    #[test]
    fn test_mask_database_url() {
        // PostgreSQL URL
        let mut pg_url = "postgres://user:secret123@localhost:5432/db".to_string();
        mask_database_url(&mut pg_url);
        assert_eq!(pg_url, format!("postgres://user:{}@localhost:5432/db", DEFAULT_MASK));
        
        // MySQL URL
        let mut mysql_url = "mysql://admin:password@db.example.com:3306/mydb".to_string();
        mask_database_url(&mut mysql_url);
        assert_eq!(mysql_url, format!("mysql://admin:{}@db.example.com:3306/mydb", DEFAULT_MASK));
        
        // SQLite URL (should not change)
        let mut sqlite_url = "sqlite:///path/to/db.sqlite".to_string();
        let original = sqlite_url.clone();
        mask_database_url(&mut sqlite_url);
        assert_eq!(sqlite_url, original);
        
        // URL without password
        let mut no_pass_url = "postgres://user@localhost:5432/db".to_string();
        let original = no_pass_url.clone();
        mask_database_url(&mut no_pass_url);
        assert_eq!(no_pass_url, original);
    }

    #[test]
    fn test_mask_map_secrets() {
        let masker = SecretMasker::default();
        let mut map = std::collections::HashMap::new();
        
        map.insert("username".to_string(), "john".to_string());
        map.insert("password".to_string(), "secret123".to_string());
        map.insert("api_key".to_string(), "key-abc-123".to_string());
        map.insert("host".to_string(), "localhost".to_string());
        
        mask_map_secrets(&mut map, &masker);
        
        // Non-secret fields should not change
        assert_eq!(map.get("username").unwrap(), "john");
        assert_eq!(map.get("host").unwrap(), "localhost");
        
        // Secret fields should be masked
        assert_eq!(map.get("password").unwrap(), DEFAULT_MASK);
        assert_eq!(map.get("api_key").unwrap(), DEFAULT_MASK);
    }

    #[test]
    fn test_custom_masker() {
        let masker = SecretMasker::new(
            vec!["custom_secret".to_string()],
            "[HIDDEN]".to_string(),
        );
        
        assert!(masker.is_secret_field("custom_secret"));
        assert!(!masker.is_secret_field("password")); // Not in custom patterns
        
        assert_eq!(masker.mask("value"), "[HIDDEN]");
    }

    #[test]
    fn test_case_insensitive_matching() {
        let masker = SecretMasker::default();
        
        // Should match regardless of case
        assert!(masker.is_secret_field("PASSWORD"));
        assert!(masker.is_secret_field("Password"));
        assert!(masker.is_secret_field("API_KEY"));
        assert!(masker.is_secret_field("Secret_Access_Key"));
    }

    #[test]
    fn test_partial_matching() {
        let masker = SecretMasker::default();
        
        // Should match substrings
        assert!(masker.is_secret_field("db_password_encrypted"));
        assert!(masker.is_secret_field("aws_secret_access_key"));
        assert!(masker.is_secret_field("jwt_bearer_token"));
    }
}
