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

//! Facet extraction helpers for proto configurations
//!
//! ## Purpose
//! Provides helper functions to extract facet configurations from proto Facet messages.
//! Used by WASM applications, Rust applications, and actor factories to consistently
//! extract facet configs and check for specific facet types.
//!
//! ## Design Principles
//! - **Proto-First**: Works with proto Facet messages directly
//! - **Consistent**: Same extraction logic for all facets (virtual_actor, durability, timer, etc.)
//! - **Production-Grade**: Handles errors gracefully, supports all facet types

use plexspaces_proto::common::v1::Facet as ProtoFacet;
use serde_json::Value;
use std::collections::HashMap;
use crate::{Facet, FacetError, FacetRegistry, FacetContainer};

/// Extract facet config from proto facets by type
///
/// ## Purpose
/// Searches through proto facets to find a facet of the specified type and returns its config.
/// Used to extract facet configurations (e.g., virtual_actor config, durability config) from ChildSpec.
///
/// ## Arguments
/// * `facets` - Vector of proto Facet configurations
/// * `facet_type` - Facet type to search for (e.g., "virtual_actor", "durability", "timer")
///
/// ## Returns
/// `Some(Value)` if facet type found, `None` otherwise
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_facet::facet_helpers::extract_facet_config;
/// 
/// let facets = vec![
///     ProtoFacet {
///         r#type: "virtual_actor".to_string(),
///         config: HashMap::new(),
///         ..
///     },
/// ];
/// let config = extract_facet_config(&facets, "virtual_actor");
/// ```
pub fn extract_facet_config(facets: &[ProtoFacet], facet_type: &str) -> Option<Value> {
    for facet in facets {
        if facet.r#type == facet_type {
            return Some(proto_config_to_value(&facet.config));
        }
    }
    None
}

/// Check if a facet type is present in proto facets
///
/// ## Purpose
/// Checks if a specific facet type exists in the proto facets list.
/// Used to determine if an actor has a particular facet (e.g., virtual_actor, durability).
///
/// ## Arguments
/// * `facets` - Vector of proto Facet configurations
/// * `facet_type` - Facet type to check for
///
/// ## Returns
/// `true` if facet type is present, `false` otherwise
pub fn has_facet_type(facets: &[ProtoFacet], facet_type: &str) -> bool {
    facets.iter().any(|f| f.r#type == facet_type)
}

/// Extract all facet configs by type
///
/// ## Purpose
/// Extracts configs for all facets matching the specified type.
/// Returns a vector since some facet types may appear multiple times (though rare).
///
/// ## Arguments
/// * `facets` - Vector of proto Facet configurations
/// * `facet_type` - Facet type to extract
///
/// ## Returns
/// Vector of facet configs (Value) for the specified facet type
pub fn extract_all_facet_configs(facets: &[ProtoFacet], facet_type: &str) -> Vec<Value> {
    facets
        .iter()
        .filter(|f| f.r#type == facet_type)
        .map(|f| proto_config_to_value(&f.config))
        .collect()
}

/// Create facet from JSON config (for VirtualActorMetadata and other JSON-based configs)
///
/// ## Purpose
/// Creates a facet instance from JSON config using FacetRegistry.
/// Used when facet config comes from JSON (e.g., VirtualActorMetadata.facet_config)
/// rather than proto Facet message.
///
/// ## Arguments
/// * `facet_type` - Facet type (e.g., "virtual_actor", "durability", "timer", "reminder")
/// * `config` - JSON config value
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// `Ok(Box<dyn Facet>)` if facet was created successfully
/// `Err(FacetError)` if facet type not found or creation failed
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_facet::facet_helpers::create_facet_from_json;
/// 
/// let config = serde_json::json!({
///     "idle_timeout": "5m",
///     "activation_strategy": "lazy"
/// });
/// let facet = create_facet_from_json("virtual_actor", config, &facet_registry).await?;
/// ```
pub async fn create_facet_from_json(
    facet_type: &str,
    config: Value,
    facet_registry: &FacetRegistry,
) -> Result<Box<dyn Facet>, FacetError> {
    let facet = facet_registry
        .create_facet(facet_type, config)
        .await?;

    Ok(facet)
}

/// Create facets from facet config JSON (for virtual actor type activation)
///
/// ## Purpose
/// Creates facets from facet_config JSON when activating a virtual actor type.
/// Extracts facet configs from JSON and creates facet instances.
/// Supports all facet types: virtual_actor, durability, timer, reminder, etc.
///
/// ## Arguments
/// * `facet_config` - JSON config containing facet configurations
///   - If object: each key is a facet type, value is its config
///   - If object with single key: legacy format (just virtual_actor config)
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// Vector of created facets (empty if no facets configured)
///
/// ## Design
/// - Checks for all facet types, not just virtual_actor
/// - Uses facet_config JSON object to extract facet configs
/// - Each facet type is a key in the JSON object
/// - Supports multiple facets (virtual_actor, durability, timer, etc.)
/// - Works for both WASM and Rust applications
pub async fn create_facets_from_config(
    facet_config: &Value,
    facet_registry: &FacetRegistry,
) -> Vec<Box<dyn Facet>> {
    let mut facets = Vec::new();
    
    // facet_config is a JSON object
    if let Some(config_obj) = facet_config.as_object() {
        // Check if this looks like a keyed format (has known facet type keys)
        // Known facet types: virtual_actor, durability, timer, reminder, etc.
        let known_facet_types = ["virtual_actor", "durability", "timer", "reminder", "event_sourcing", 
                                  "locks", "key_value", "http_client", "event_emitter", "logging", 
                                  "caching", "metrics", "registry", "process_group"];
        let has_facet_type_keys = config_obj.keys().any(|k| known_facet_types.contains(&k.as_str()));
        
        if has_facet_type_keys {
            let mut created_types: Vec<String> = Vec::new();
            for (facet_type, config_value) in config_obj {
                match create_facet_from_json(facet_type, config_value.clone(), facet_registry).await {
                    Ok(facet) => {
                        created_types.push(facet_type.clone());
                        facets.push(facet);
                    }
                    Err(e) => {
                        // JournalStorage not found is a critical error - log as error, not warning
                        let is_journal_error = e.to_string().contains("JournalStorage not found");
                        if is_journal_error {
                            tracing::error!(
                                facet_type = %facet_type,
                                error = %e,
                                "Failed to create facet from config - JournalStorage not registered in ServiceLocator"
                            );
                        } else {
                            tracing::warn!(
                                facet_type = %facet_type,
                                error = %e,
                                "Failed to create facet from config (skipping)"
                            );
                        }
                    }
                }
            }
            if tracing::enabled!(tracing::Level::DEBUG) && !created_types.is_empty() {
                tracing::debug!(
                    facets = %created_types.join(", "),
                    created = facets.len(),
                    "Created facets from JSON config"
                );
            }
        } else if !config_obj.is_empty() {
            // Legacy format: flat object with virtual_actor config keys (idle_timeout, activation_strategy, etc.)
            // Try to create virtual_actor facet
            // Skip empty objects (no config provided)
            if let Ok(facet) = create_facet_from_json("virtual_actor", facet_config.clone(), facet_registry).await {
                facets.push(facet);
            }
        }
        // Empty object: no facets to create
    }
    
    facets
}

/// Check if a facet type is attached to an actor
///
/// ## Purpose
/// Checks if a specific facet type is attached to an actor by examining its FacetContainer.
/// Used to verify facet attachment after actor creation (e.g., virtual_actor, durability, timer).
///
/// ## Arguments
/// * `facet_container` - Actor's facet container (from `actor.facets()`)
/// * `facet_type` - Facet type to check for (e.g., "virtual_actor", "durability", "timer")
///
/// ## Returns
/// `true` if facet type is attached, `false` otherwise
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_facet::facet_helpers::has_facet_attached;
/// 
/// let facets_container = actor.facets();
/// let has_virtual = has_facet_attached(&facets_container, "virtual_actor").await;
/// ```
pub async fn has_facet_attached(
    facet_container: &std::sync::Arc<tokio::sync::RwLock<FacetContainer>>,
    facet_type: &str,
) -> bool {
    let facets_guard = facet_container.read().await;
    facets_guard.list_facets().contains(&facet_type.to_string())
}

/// Extract facet_config from facets for virtual actor type registration
///
/// ## Purpose
/// Extracts facet configurations from facets (either Vec<Box<dyn Facet>> or proto facets)
/// and builds a keyed JSON object suitable for `register_virtual_actor_type()`.
/// This ensures consistent facet_config format across SDK, WASM applications, and app-config.toml.
///
/// ## Arguments
/// * `facets` - Optional vector of facet trait objects (from SDK spawn_with_facets)
/// * `proto_facets` - Optional vector of proto facets (from WASM app-config.toml)
///
/// ## Returns
/// JSON object with facet configs keyed by facet type (e.g., `{"virtual_actor": {...}, "durability": {...}}`)
///
/// ## Design
/// - Extracts config from all facets (virtual_actor, durability, timer, etc.)
/// - Uses keyed format required by `register_virtual_actor_type()`
/// - Falls back to defaults for virtual_actor if not present
/// - Consistent across SDK annotations and app-config.toml
pub fn extract_facet_config_for_registration(
    facets: Option<&[Box<dyn Facet>]>,
    proto_facets: Option<&[ProtoFacet]>,
) -> serde_json::Value {
    use plexspaces_common::virtual_actor_config::{DEFAULT_IDLE_TIMEOUT_SECONDS, DEFAULT_ACTIVATION_STRATEGY, format_duration};
    use plexspaces_common::to_config_str;
    use std::time::Duration;
    
    let mut facet_configs = serde_json::Map::new();
    
    // Extract from proto facets (WASM app-config.toml)
    if let Some(proto_facets) = proto_facets {
        let mut extracted_types = Vec::new();
        for facet in proto_facets {
            let facet_type = &facet.r#type;
            let config = proto_config_to_value(&facet.config);
            facet_configs.insert(facet_type.clone(), config);
            extracted_types.push(facet_type.clone());
        }
    }
    
    // Extract from facet trait objects (SDK annotations)
    if let Some(facets) = facets {
        for facet in facets {
            let facet_type = facet.facet_type();
            // Get config from facet (if it exposes it)
            // For VirtualActorFacet, we can extract config
            // Get config from facet (get_config() returns Value directly)
            let config = facet.get_config();
            facet_configs.insert(facet_type.to_string(), config);
        }
    }
    
    // If no facets found, provide default virtual_actor config
    if facet_configs.is_empty() {
        facet_configs.insert("virtual_actor".to_string(), serde_json::json!({
            "idle_timeout": format_duration(Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECONDS)),
            "activation_strategy": to_config_str(&DEFAULT_ACTIVATION_STRATEGY)
        }));
    }
    
    serde_json::Value::Object(facet_configs)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_facet_config() {
        let mut config = HashMap::new();
        config.insert("idle_timeout".to_string(), "5m".to_string());
        
        let facets = vec![
            ProtoFacet {
                r#type: "virtual_actor".to_string(),
                config: config.clone(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "durability".to_string(),
                config: HashMap::new(),
                priority: 90,
                state: HashMap::new(),
                metadata: None,
            },
        ];
        
        let config = extract_facet_config(&facets, "virtual_actor");
        assert!(config.is_some());
        let config_value = config.unwrap();
        let config_obj = config_value.as_object().unwrap();
        assert_eq!(config_obj.get("idle_timeout").unwrap().as_str().unwrap(), "5m");
    }

    #[test]
    fn test_has_facet_type() {
        let facets = vec![
            ProtoFacet {
                r#type: "virtual_actor".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
        ];
        
        assert!(has_facet_type(&facets, "virtual_actor"));
        assert!(!has_facet_type(&facets, "durability"));
    }

    #[test]
    fn test_extract_facet_config_not_found() {
        let facets = vec![
            ProtoFacet {
                r#type: "durability".to_string(),
                config: HashMap::new(),
                priority: 90,
                state: HashMap::new(),
                metadata: None,
            },
        ];
        
        let config = extract_facet_config(&facets, "virtual_actor");
        assert!(config.is_none());
    }

    #[test]
    fn test_extract_all_facet_configs() {
        let mut config1 = HashMap::new();
        config1.insert("idle_timeout".to_string(), "5m".to_string());
        
        let mut config2 = HashMap::new();
        config2.insert("idle_timeout".to_string(), "10m".to_string());
        
        let facets = vec![
            ProtoFacet {
                r#type: "virtual_actor".to_string(),
                config: config1,
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "virtual_actor".to_string(),
                config: config2,
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "durability".to_string(),
                config: HashMap::new(),
                priority: 90,
                state: HashMap::new(),
                metadata: None,
            },
        ];
        
        let configs = extract_all_facet_configs(&facets, "virtual_actor");
        assert_eq!(configs.len(), 2);
        
        let configs = extract_all_facet_configs(&facets, "durability");
        assert_eq!(configs.len(), 1);
        
        let configs = extract_all_facet_configs(&facets, "nonexistent");
        assert_eq!(configs.len(), 0);
    }

    #[test]
    fn test_extract_facet_config_empty_facets() {
        let facets = vec![];
        
        let config = extract_facet_config(&facets, "virtual_actor");
        assert!(config.is_none());
        
        assert!(!has_facet_type(&facets, "virtual_actor"));
    }

    #[test]
    fn test_proto_config_to_value_json_parsing() {
        use super::proto_config_to_value;
        
        let mut config = HashMap::new();
        config.insert("string_val".to_string(), "hello".to_string());
        config.insert("number_val".to_string(), "123".to_string());
        config.insert("json_val".to_string(), r#"{"nested": "value"}"#.to_string());
        
        let value = proto_config_to_value(&config);
        assert!(value.is_object());
        let obj = value.as_object().unwrap();
        
        assert_eq!(obj.get("string_val").unwrap().as_str().unwrap(), "hello");
        assert_eq!(obj.get("number_val").unwrap().as_u64().unwrap(), 123);
        assert!(obj.get("json_val").unwrap().is_object());
    }
}
