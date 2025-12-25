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

//! Helper functions for creating facets from proto configuration
//!
//! ## Purpose
//! Provides simple, extensible functions to convert proto Facet configurations
//! to facet instances. This enables automatic facet attachment during actor creation
//! from ChildSpec configurations.
//!
//! ## Design Principles
//! - **Simple**: Straightforward conversion from proto to facet instances
//! - **Extensible**: Easy to add new facet types via FacetRegistry
//! - **Debuggable**: Clear error messages when facet creation fails
//! - **Production-grade**: Handles errors gracefully, logs appropriately

use plexspaces_facet::{Facet, FacetError, FacetRegistry};
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use serde_json::Value;
use std::collections::HashMap;
use tracing;

/// Convert proto Facet configuration to facet instance
///
/// ## Purpose
/// Creates a facet instance from proto Facet configuration using FacetRegistry.
/// This is used by supervisors to automatically attach facets from ChildSpec.
///
/// ## Arguments
/// * `proto_facet` - Proto Facet configuration
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// `Ok(Box<dyn Facet>)` if facet was created successfully
/// `Err(FacetError)` if facet type not found or creation failed
///
/// ## Example
/// ```rust,ignore
/// let proto_facet = plexspaces_proto::common::v1::Facet {
///     r#type: "timer".to_string(),
///     config: HashMap::new(),
///     priority: 100,
///     state: HashMap::new(),
///     metadata: None,
/// };
/// let facet = create_facet_from_proto(&proto_facet, &facet_registry).await?;
/// ```
pub async fn create_facet_from_proto(
    proto_facet: &ProtoFacet,
    facet_registry: &FacetRegistry,
) -> Result<Box<dyn Facet>, FacetError> {
    let facet_type = &proto_facet.r#type;
    
    // Convert proto config (map<string, string>) to serde_json::Value
    let config_value = proto_config_to_value(&proto_facet.config);
    
    // Create facet instance via registry
    let mut facet = facet_registry
        .create_facet(facet_type, config_value)
        .await?;
    
    // Set priority from proto (facets may have default priority, but proto overrides it)
    // Note: This requires facets to support priority setting, which is handled by FacetContainer
    // The priority is stored in FacetContainer metadata, not in the facet itself
    // So we just return the facet - priority will be set during attachment
    
    tracing::debug!(
        facet_type = %facet_type,
        priority = proto_facet.priority,
        "Created facet from proto configuration"
    );
    
    Ok(facet)
}

/// Convert proto config map to serde_json::Value
///
/// ## Purpose
/// Converts proto's `map<string, string>` to `serde_json::Value` for facet configuration.
/// This is a simple, straightforward conversion that preserves all key-value pairs.
///
/// ## Arguments
/// * `config_map` - Proto config map (string -> string)
///
/// ## Returns
/// `serde_json::Value::Object` with all config key-value pairs
fn proto_config_to_value(config_map: &HashMap<String, String>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in config_map {
        // Try to parse value as JSON, fallback to string if not valid JSON
        match serde_json::from_str::<Value>(value) {
            Ok(json_value) => {
                map.insert(key.clone(), json_value);
            }
            Err(_) => {
                // Not valid JSON, treat as string
                map.insert(key.clone(), Value::String(value.clone()));
            }
        }
    }
    Value::Object(map)
}

/// Create multiple facets from proto configurations
///
/// ## Purpose
/// Creates multiple facet instances from proto Facet configurations.
/// Facets are created in the order provided, but should be sorted by priority
/// before calling this function for proper attachment order.
///
/// ## Arguments
/// * `proto_facets` - Vector of proto Facet configurations
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// `Ok(Vec<Box<dyn Facet>>)` with all successfully created facets
/// Errors are logged but don't stop creation of other facets
///
/// ## Error Handling
/// If a facet fails to create, it's logged as a warning and skipped.
/// This ensures that one bad facet doesn't prevent other facets from being attached.
pub async fn create_facets_from_proto(
    proto_facets: &[ProtoFacet],
    facet_registry: &FacetRegistry,
) -> Vec<Box<dyn Facet>> {
    let mut facets = Vec::new();
    
    for proto_facet in proto_facets {
        match create_facet_from_proto(proto_facet, facet_registry).await {
            Ok(facet) => {
                facets.push(facet);
            }
            Err(e) => {
                tracing::warn!(
                    facet_type = %proto_facet.r#type,
                    error = %e,
                    "Failed to create facet from proto configuration (skipping)"
                );
            }
        }
    }
    
    tracing::debug!(
        total = proto_facets.len(),
        created = facets.len(),
        "Created facets from proto configurations"
    );
    
    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_facet::{FacetFactory, FacetMetadata};
    use async_trait::async_trait;
    use std::sync::Arc;

    // Test facet for unit tests
    struct TestFacet {
        config: Value,
        priority: i32,
    }

    #[async_trait]
    impl Facet for TestFacet {
        fn facet_type(&self) -> &str {
            "test"
        }

        fn get_config(&self) -> Value {
            self.config.clone()
        }

        fn get_priority(&self) -> i32 {
            self.priority
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }

        async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
            Ok(())
        }

        async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
            Ok(())
        }
    }

    struct TestFacetFactory;

    #[async_trait]
    impl FacetFactory for TestFacetFactory {
        async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
            Ok(Box::new(TestFacet {
                config,
                priority: 50,
            }))
        }

        fn metadata(&self) -> FacetMetadata {
            FacetMetadata {
                facet_type: "test".to_string(),
                attached_at: std::time::Instant::now(),
                config: serde_json::Value::Null,
                priority: 50,
            }
        }
    }

    #[tokio::test]
    async fn test_proto_config_to_value() {
        let mut config_map = HashMap::new();
        config_map.insert("key1".to_string(), "value1".to_string());
        config_map.insert("key2".to_string(), "123".to_string());
        config_map.insert("key3".to_string(), r#"{"nested": "value"}"#.to_string());

        let value = proto_config_to_value(&config_map);
        assert!(value.is_object());
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("key1").unwrap().as_str().unwrap(), "value1");
        // key2 is parsed as JSON number (123), not string
        assert_eq!(obj.get("key2").unwrap().as_u64().unwrap(), 123);
        // key3 should be parsed as JSON object
        assert!(obj.get("key3").unwrap().is_object());
    }

    #[tokio::test]
    async fn test_create_facet_from_proto() {
        let mut registry = FacetRegistry::new();
        registry.register("test".to_string(), Arc::new(TestFacetFactory));

        let mut config_map = HashMap::new();
        config_map.insert("test_key".to_string(), "test_value".to_string());

        let proto_facet = ProtoFacet {
            r#type: "test".to_string(),
            config: config_map,
            priority: 100,
            state: HashMap::new(),
            metadata: None,
        };

        let facet = create_facet_from_proto(&proto_facet, &registry).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "test");
    }

    #[tokio::test]
    async fn test_create_facet_from_proto_not_found() {
        let registry = FacetRegistry::new();

        let proto_facet = ProtoFacet {
            r#type: "nonexistent".to_string(),
            config: HashMap::new(),
            priority: 100,
            state: HashMap::new(),
            metadata: None,
        };

        let result = create_facet_from_proto(&proto_facet, &registry).await;
        assert!(result.is_err());
        // Check error type without requiring Debug on Ok type
        match &result {
            Ok(_) => panic!("Expected NotFound error, got success"),
            Err(FacetError::NotFound(_)) => {
                // Expected - test passes
            }
            Err(e) => {
                panic!("Expected NotFound error, got: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_create_facets_from_proto() {
        let mut registry = FacetRegistry::new();
        registry.register("test".to_string(), Arc::new(TestFacetFactory));

        let proto_facets = vec![
            ProtoFacet {
                r#type: "test".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "test".to_string(),
                config: HashMap::new(),
                priority: 50,
                state: HashMap::new(),
                metadata: None,
            },
        ];

        let facets = create_facets_from_proto(&proto_facets, &registry).await;
        assert_eq!(facets.len(), 2);
    }

    #[tokio::test]
    async fn test_create_facets_from_proto_with_errors() {
        let registry = FacetRegistry::new(); // No facets registered

        let proto_facets = vec![
            ProtoFacet {
                r#type: "nonexistent1".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "nonexistent2".to_string(),
                config: HashMap::new(),
                priority: 50,
                state: HashMap::new(),
                metadata: None,
            },
        ];

        let facets = create_facets_from_proto(&proto_facets, &registry).await;
        // Should return empty vector (all facets failed, but didn't panic)
        assert_eq!(facets.len(), 0);
    }
}



