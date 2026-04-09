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

//! Facet helpers and factories for creating facets from proto configurations
//!
//! ## Purpose
//! Provides helper functions to extract facet configurations from proto Facet messages
//! and factories for creating facet instances. Used by WASM applications, Rust applications,
//! and actor factories to consistently extract facet configs and create facets.
//!
//! ## Design Principles
//! - **Proto-First**: Works with proto Facet messages directly
//! - **Consistent**: Same extraction logic for all facets (virtual_actor, durability, timer, etc.)
//! - **Production-Grade**: Handles errors gracefully, supports all facet types
//! - **Runtime Config**: Factories use ServiceLocator to get runtime configuration

use crate::{ProcessGroupService, RequestContext};
use async_trait::async_trait;
use plexspaces_facet::{
    Facet, FacetContainer, FacetError, FacetFactory, FacetMetadata, FacetRegistry,
};
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_proto::locks::prv::{
    AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions,
};
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing;

// Re-export facet helpers from facet crate (for consistency)
pub use plexspaces_facet::facet_helpers::{
    create_facet_from_json, create_facets_from_config, extract_all_facet_configs,
    extract_facet_config, has_facet_attached, has_facet_type,
};

/// Return the config object for a specific facet type or an empty object when absent.
pub fn facet_config_value(config: &Value, facet_type: &str) -> Value {
    config
        .get(facet_type)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

/// Build the canonical default virtual actor facet config.
pub fn default_virtual_actor_facet_config(config: Option<&Value>) -> Value {
    use plexspaces_common::to_config_str;
    use plexspaces_common::virtual_actor_config::{
        format_duration, DEFAULT_ACTIVATION_STRATEGY, DEFAULT_IDLE_TIMEOUT_SECONDS,
    };
    use std::time::Duration;

    config
        .and_then(|value| value.get("virtual_actor"))
        .cloned()
        .unwrap_or_else(|| {
            serde_json::json!({
                "idle_timeout": format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS)),
                "activation_strategy": to_config_str(&DEFAULT_ACTIVATION_STRATEGY)
            })
        })
}

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
pub async fn create_facet_from_proto(
    proto_facet: &ProtoFacet,
    facet_registry: &FacetRegistry,
) -> Result<Box<dyn Facet>, FacetError> {
    let facet_type = &proto_facet.r#type;

    // Convert proto config (map<string, string>) to serde_json::Value
    let config_value = proto_config_to_value(&proto_facet.config);

    let facet = facet_registry
        .create_facet(facet_type, config_value)
        .await?;

    Ok(facet)
}

/// Convert proto config map to serde_json::Value
///
/// ## Purpose
/// Converts proto's `map<string, string>` to `serde_json::Value` for facet configuration.
/// This is a simple, straightforward conversion that preserves all key-value pairs.
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
/// `Vec<Box<dyn Facet>>` with all successfully created facets
/// Errors are logged but don't stop creation of other facets
pub async fn create_facets_from_proto(
    proto_facets: &[ProtoFacet],
    facet_registry: &FacetRegistry,
) -> Vec<Box<dyn Facet>> {
    let mut facets = Vec::new();
    let mut created_types: Vec<String> = Vec::new();

    for proto_facet in proto_facets {
        match create_facet_from_proto(proto_facet, facet_registry).await {
            Ok(facet) => {
                created_types.push(format!("{}({})", proto_facet.r#type, proto_facet.priority));
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

    if tracing::enabled!(tracing::Level::TRACE) && !created_types.is_empty() {
        tracing::trace!(
            facets = %created_types.join(", "),
            total = proto_facets.len(),
            created = facets.len(),
            "Created facets from proto configurations"
        );
    }

    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_facet::{FacetFactory, FacetMetadata};
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
            Err(_) => panic!("Expected NotFound error, got different error"),
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
                r#type: "nonexistent".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "test".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
        ];

        let facets = create_facets_from_proto(&proto_facets, &registry).await;
        // Should create 2 facets (test, test) and skip nonexistent
        assert_eq!(facets.len(), 2);
    }
}
