// SPDX-License-Identifier: AGPL-3.0-or-later
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
//! Provides helper functions to register virtual actor types from SDK annotations,
//! app-config.toml, or WASM applications. Ensures consistent behavior across all
//! languages and entry points using `ActorSpawnSpec` as the single source of truth.
//!
//! ## Design
//! - `register_virtual_actor_definition` takes `ActorSpawnSpec` directly — no intermediate struct.
//! - `register_virtual_actor_type_consistent` is a convenience wrapper for callers that build
//!   registration data from SDK facets + proto facets rather than a fully-formed spec.
//! - Idempotent (safe to call multiple times).

use plexspaces_facet::facet_helpers::extract_facet_config_for_registration;
use plexspaces_facet::Facet;
use plexspaces_proto::actor::v1::ActorSpawnSpec;
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_proto::v1::actor::ActorConfig;
use std::collections::HashMap;

/// Convert SDK/runtime facets into proto-shaped facets for shared metadata.
///
/// Prefers proto_facets when provided; otherwise converts SDK trait objects.
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

/// Register a virtual actor type consistently across all entry points.
///
/// ## Purpose
/// Convenience wrapper for callers that build registration data from SDK facets +
/// proto facets. Constructs an `ActorSpawnSpec` and delegates to
/// `register_virtual_actor_definition`.
///
/// ## Arguments
/// * `service_locator` - ServiceLocator for accessing VirtualActorManager
/// * `actor_type` - Behavior class (VirtualActorManager key, BehaviorRegistry key)
/// * `instance_name` - Declaration-time name (reverse index key); empty for type-only registration
/// * `namespace` - Namespace for isolation
/// * `facets` - Optional SDK trait-object facets (from Rust SDK annotations)
/// * `proto_facets` - Optional proto facets (from WASM app-config.toml)
/// * `actor_config` - Optional actor runtime configuration
/// * `tenant_id` - Optional tenant ID (defaults to empty for type-level registration)
/// * `init_config_template` - Optional JSON bytes carrying initial args
///   (`{"args": {"k": "v"}}` or flat `{"k": "v"}`). Decoded into `spec.args` so
///   `wasm_init_payload` can derive the init payload at activation time without
///   any further JSON round-trips.
pub async fn register_virtual_actor_type_consistent(
    service_locator: &std::sync::Arc<dyn crate::ServiceLocator>,
    actor_type: String,
    instance_name: String,
    namespace: String,
    facets: Option<&[Box<dyn Facet>]>,
    proto_facets: Option<&[ProtoFacet]>,
    actor_config: Option<ActorConfig>,
    tenant_id: Option<String>,
    init_config_template: Option<Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let effective_proto_facets = proto_facets_for_registration(facets, proto_facets);
    let args = extract_args_from_template(init_config_template.as_deref());
    // behavior_kind is not known at this call site — callers that know it should call
    // register_virtual_actor_definition directly with a fully-formed ActorSpawnSpec.
    let spec = ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: instance_name,
            actor_type,
        }),
        role: String::new(),
        namespace,
        tenant_id: tenant_id.unwrap_or_default(),
        behavior_kind: String::new(),
        args,
        facets: effective_proto_facets,
        labels: HashMap::new(),
        config: actor_config,
        visibility: 0,
        ..Default::default()
    };
    register_virtual_actor_definition(service_locator, spec).await
}

/// Extract `HashMap<String,String>` args from a JSON init-config template.
///
/// Supports two formats:
///   `{"args": {"k": "v"}}` — canonical nested form
///   `{"k": "v"}` — flat top-level scalars (legacy test helpers)
/// Meta fields (actor_id, actor_type, role, behavior_kind, args) are excluded.
fn extract_args_from_template(template: Option<&[u8]>) -> HashMap<String, String> {
    const META: &[&str] = &["actor_id", "actor_type", "role", "behavior_kind", "args"];
    template
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .and_then(|v| {
            let obj = v.as_object()?;
            if let Some(serde_json::Value::Object(map)) = obj.get("args") {
                return Some(
                    map.iter()
                        .map(|(k, v)| {
                            let s = match v {
                                serde_json::Value::String(s) => s.clone(),
                                _ => v.to_string(),
                            };
                            (k.clone(), s)
                        })
                        .collect(),
                );
            }
            let flat: HashMap<String, String> = obj
                .iter()
                .filter(|(k, _)| !META.contains(&k.as_str()))
                .filter_map(|(k, v)| {
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k.clone(), s))
                })
                .collect();
            if flat.is_empty() {
                None
            } else {
                Some(flat)
            }
        })
        .unwrap_or_default()
}

/// Register a virtual actor definition using `ActorSpawnSpec` as the single source of truth.
///
/// ## Purpose
/// All registration paths (WASM `ChildSpec`, Rust SDK `spawn_with_facets`, tests) converge here.
/// Stores the spec in `VirtualActorManager` so `wasm_init_payload` can derive init bytes at
/// activation time without any intermediate JSON encoding.
pub async fn register_virtual_actor_definition(
    service_locator: &std::sync::Arc<dyn crate::ServiceLocator>,
    spec: ActorSpawnSpec,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let actor_type = spec
        .identity
        .as_ref()
        .map(|id| id.actor_type.as_str())
        .unwrap_or("")
        .to_string();
    let namespace = spec.namespace.clone();

    let virtual_actor_manager = service_locator
        .virtual_actor_manager()
        .await
        .ok_or_else(|| "VirtualActorManager not available".to_string())?;

    virtual_actor_manager
        .register_virtual_actor_definition(spec)
        .await
        .map_err(|e| {
            tracing::warn!(
                actor_type = %actor_type,
                namespace = %namespace,
                error = %e,
                "Failed to register virtual actor type (actor will still work, but auto-activation may fail)"
            );
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })?;

    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!(
            actor_type = %actor_type,
            namespace = %namespace,
            "Registered virtual actor type for auto-activation"
        );
    }

    Ok(())
}
