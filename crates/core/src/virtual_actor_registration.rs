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

//! Centralized virtual actor type registration helper
//!
//! ## Purpose
//! Provides a consistent helper function to register virtual actor types from SDK annotations,
//! app-config.toml, or WASM applications. Ensures consistent behavior across all languages and entry points.
//!
//! ## Design
//! - Extracts facet_config from facets consistently using `extract_facet_config_for_registration()`
//! - Uses keyed format required by `register_virtual_actor_type()`
//! - Logs errors but doesn't fail (actor will still work, just no auto-activation)
//! - Idempotent (safe to call multiple times)

use plexspaces_facet::facet_helpers::extract_facet_config_for_registration;
use plexspaces_facet::Facet;
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_proto::v1::actor::ActorConfig;
use std::collections::HashMap;

/// Framework-owned registration payload for virtual actor definitions.
///
/// SDKs and deployment surfaces should construct this value and delegate to
/// core registration instead of re-implementing registration details locally.
#[derive(Clone, Debug)]
pub struct VirtualActorDefinitionRegistration {
    /// Actor type used for activation lookup.
    pub actor_type: String,
    /// Optional behavior kind for observability and SDK parity metadata.
    pub behavior_kind: Option<String>,
    /// Namespace for tenant isolation.
    pub namespace: String,
    /// Proto-first actor configuration.
    pub actor_config: Option<ActorConfig>,
    /// Proto-shaped facets captured at registration time.
    pub proto_facets: Vec<ProtoFacet>,
    /// Canonical keyed facet configuration used by activation/rebuild.
    pub facet_config: serde_json::Value,
    /// Optional tenant ID for type registration.
    pub tenant_id: Option<String>,
    /// Optional WASM init config template.
    pub init_config_template: Option<Vec<u8>>,
}

/// Convert SDK/runtime facets into proto-shaped facets for shared metadata.
pub fn proto_facets_for_registration(
    facets: Option<&[Box<dyn Facet>]>,
    proto_facets: Option<&[ProtoFacet]>,
) -> Vec<ProtoFacet> {
    if let Some(proto_facets) = proto_facets {
        return proto_facets.to_vec();
    }

    facets
        .map(|facets| {
            facets
                .iter()
                .map(|facet| ProtoFacet {
                    r#type: facet.facet_type().to_string(),
                    config: value_to_proto_config(&facet.get_config()),
                    priority: facet.get_priority(),
                    state: HashMap::new(),
                    metadata: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Materialize a stored init-config template with the concrete actor ID used for activation.
///
/// Activation templates are registered at the actor-type level, so they cannot know the final
/// actor ID ahead of time. When a virtual actor instance is activated or reactivated, the
/// framework must inject the resolved actor ID before handing the config to the behavior
/// constructor. Non-object or non-JSON templates are returned unchanged so non-WASM callers
/// keep their original semantics.
pub fn materialize_init_config_template(
    init_config_template: Option<Vec<u8>>,
    actor_id: &str,
) -> Option<Vec<u8>> {
    let template = init_config_template?;
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&template) else {
        return Some(template);
    };

    let Some(object) = value.as_object_mut() else {
        return Some(template);
    };

    object.insert(
        "actor_id".to_string(),
        serde_json::Value::String(actor_id.to_string()),
    );

    serde_json::to_vec(&value).ok().or(Some(template))
}

fn value_to_proto_config(value: &serde_json::Value) -> HashMap<String, String> {
    let mut config = HashMap::new();
    if let Some(obj) = value.as_object() {
        for (key, item) in obj {
            let encoded = match item {
                serde_json::Value::String(s) => s.clone(),
                _ => item.to_string(),
            };
            config.insert(key.clone(), encoded);
        }
    }
    config
}

/// Register virtual actor type consistently across all entry points
///
/// ## Purpose
/// Centralized helper to register virtual actor types from SDK annotations, app-config.toml,
/// or WASM applications. Ensures consistent behavior across all languages and entry points.
///
/// ## Arguments
/// * `service_locator` - ServiceLocator for accessing VirtualActorManager
/// * `actor_type` - Actor type (e.g., "GenServer", "read-state-tracker")
/// * `namespace` - Namespace for isolation
/// * `facets` - Optional vector of facet trait objects (from SDK)
/// * `proto_facets` - Optional vector of proto facets (from WASM app-config.toml)
/// * `actor_config` - Optional actor configuration
/// * `tenant_id` - Optional tenant ID (defaults to empty for type-level registration)
/// * `init_config_template` - Optional init config template for WASM actors (JSON bytes)
///   Preserves config structure from ApplicationSpec's ChildSpec.args for virtual actor activation
///
/// ## Returns
/// `Ok(())` if registration succeeded, `Err` if failed (logged but non-fatal)
///
/// ## Design
/// - Extracts facet_config from facets consistently using `extract_facet_config_for_registration()`
/// - Uses keyed format required by `register_virtual_actor_type()`
/// - Logs errors but doesn't fail (actor will still work, just no auto-activation)
/// - Idempotent (safe to call multiple times)
pub async fn register_virtual_actor_type_consistent(
    service_locator: &std::sync::Arc<dyn crate::ServiceLocator>,
    actor_type: String,
    namespace: String,
    facets: Option<&[Box<dyn Facet>]>,
    proto_facets: Option<&[ProtoFacet]>,
    actor_config: Option<ActorConfig>,
    tenant_id: Option<String>,
    init_config_template: Option<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    register_virtual_actor_definition(
        service_locator,
        VirtualActorDefinitionRegistration {
            behavior_kind: Some(actor_type.clone()),
            actor_type,
            namespace,
            actor_config,
            proto_facets: proto_facets_for_registration(facets, proto_facets),
            facet_config: extract_facet_config_for_registration(facets, proto_facets),
            tenant_id,
            init_config_template,
        },
    )
    .await
}

/// Register a virtual actor definition using the framework-owned metadata shape.
pub async fn register_virtual_actor_definition(
    service_locator: &std::sync::Arc<dyn crate::ServiceLocator>,
    definition: VirtualActorDefinitionRegistration,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let virtual_actor_manager = service_locator
        .virtual_actor_manager()
        .await
        .ok_or_else(|| "VirtualActorManager not available".to_string())?;

    virtual_actor_manager
        .register_virtual_actor_definition(definition.clone())
        .await
        .map_err(|e| {
            tracing::warn!(
                actor_type = %definition.actor_type,
                namespace = %definition.namespace,
                error = %e,
                "Failed to register virtual actor type (actor will still work, but auto-activation may fail)"
            );
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })?;

    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!(
            actor_type = %definition.actor_type,
            namespace = %definition.namespace,
            "Registered virtual actor type for auto-activation"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::materialize_init_config_template;

    #[test]
    fn materialize_init_config_template_replaces_actor_id() {
        let template =
            br#"{"actor_id":"","behavior_kind":"GenServer","args":{"role":"abstractions"}}"#
                .to_vec();
        let materialized = materialize_init_config_template(
            Some(template),
            "cart-1//abstractions::abstractions-rust@test-node-8091",
        )
        .expect("template should be materialized");

        let value: serde_json::Value = serde_json::from_slice(&materialized)
            .expect("materialized template must be valid json");
        assert_eq!(
            value.get("actor_id").and_then(|v| v.as_str()),
            Some("cart-1//abstractions::abstractions-rust@test-node-8091")
        );
        assert_eq!(
            value.get("behavior_kind").and_then(|v| v.as_str()),
            Some("GenServer")
        );
        assert_eq!(
            value
                .get("args")
                .and_then(|v| v.get("role"))
                .and_then(|v| v.as_str()),
            Some("abstractions")
        );
    }

    #[test]
    fn materialize_init_config_template_preserves_non_json() {
        let template = b"not-json".to_vec();
        let materialized =
            materialize_init_config_template(Some(template.clone()), "actor-1").expect("template");
        assert_eq!(materialized, template);
    }
}
