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

//! Actor spawn/activation logic for ActorServiceImpl.

use std::sync::Arc;

use plexspaces_actor::{
    ActorId, ActorRef as ActorRefImpl, RequestContextExt, ServiceLocator as ServiceLocatorTrait,
};
use tonic::Status;

use super::ActorServiceImpl;

impl ActorServiceImpl {
    /// Spawn a new actor locally from an [`plexspaces_actor::ActorSpawnSpec`].
    ///
    /// Always materializes on this node via [`ActorFactory`]. Callers targeting another node must
    /// use that node's service.
    pub async fn spawn_actor_local_from_spec(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        input_spec: &plexspaces_actor::ActorSpawnSpec,
    ) -> Result<ActorRefImpl, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_actor::ActorFactory;
        use plexspaces_actor::ActorSpawnSpec;
        use plexspaces_proto::common::v1::ActorIdentity;

        let identity = input_spec.identity.as_ref().ok_or_else(
            || -> Box<dyn std::error::Error + Send + Sync> {
                "spawn_actor_local_from_spec: spec.identity is required".into()
            },
        )?;
        if identity.actor_type.is_empty() {
            return Err("spawn_actor_local_from_spec: spec.identity.actor_type is required".into());
        }

        let effective_namespace = if input_spec.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            input_spec.namespace.clone()
        };

        let requested_actor_type = identity.actor_type.clone();
        let resolved_actor_type =
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                manager
                    .resolve_actor_type_for_name(&effective_namespace, &requested_actor_type)
                    .await
            } else {
                requested_actor_type.clone()
            };

        let role = if !input_spec.role.is_empty() {
            input_spec.role.clone()
        } else if resolved_actor_type != requested_actor_type {
            requested_actor_type.clone()
        } else {
            String::new()
        };

        let mut init_args = input_spec.args.clone();
        if init_args.is_empty() && !role.is_empty() {
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                if let Some(definition) = manager
                    .get_virtual_actor_definition(&effective_namespace, &role)
                    .await
                {
                    init_args = definition.spec.args;
                }
            }
        }

        let name_for_id = identity.name.as_str();
        let local_actor_id = if !name_for_id.is_empty() {
            if let Ok(parsed) = ActorId::from_canonical(name_for_id) {
                if parsed.node_id() != self.local_node_id {
                    return Err(format!(
                        "Cannot spawn actor on remote node '{}' via local ActorService. ActorService always creates actors locally. To spawn on '{}', call that node's ActorService directly.",
                        parsed.node_id(), parsed.node_id()
                    )
                    .into());
                }
                self.build_canonical_actor_id(
                    parsed.name(),
                    &resolved_actor_type,
                    if parsed.namespace().is_empty() {
                        effective_namespace.as_str()
                    } else {
                        parsed.namespace()
                    },
                    &self.local_node_id,
                )
                .map_err(
                    |e: Status| -> Box<dyn std::error::Error + Send + Sync> {
                        e.to_string().into()
                    },
                )?
            } else {
                self.build_canonical_actor_id(
                    name_for_id,
                    &resolved_actor_type,
                    &effective_namespace,
                    &self.local_node_id,
                )
                .map_err(
                    |e: Status| -> Box<dyn std::error::Error + Send + Sync> {
                        e.to_string().into()
                    },
                )?
            }
        } else {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            self.build_canonical_actor_id(
                &format!("actor-{}", timestamp),
                &resolved_actor_type,
                &effective_namespace,
                &self.local_node_id,
            )
            .map_err(|e: Status| -> Box<dyn std::error::Error + Send + Sync> {
                e.to_string().into()
            })?
        };

        let actor_factory: Arc<dyn ActorFactory> = self.service_locator.get_actor_factory().await
            .ok_or_else(|| -> Box<dyn std::error::Error + Send + Sync> {
                format!(
                    "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called and ActorFactory is registered. Actor ID would be: {}",
                    local_actor_id
                )
                .into()
            })?;

        let facets_proto = input_spec.facets.clone();
        let mut facet_boxes: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();
        if !facets_proto.is_empty() {
            if let Some(facet_registry_wrapper) = self.service_locator.get_facet_registry().await {
                let facet_registry = facet_registry_wrapper.inner();
                for proto_facet in &facets_proto {
                    match plexspaces_actor::create_facet_from_proto(proto_facet, facet_registry)
                        .await
                    {
                        Ok(facet_box) => facet_boxes.push(facet_box),
                        Err(e) => {
                            tracing::warn!(
                                actor_id = %local_actor_id,
                                facet_type = %proto_facet.r#type,
                                error = %e,
                                "spawn_actor_local_from_spec: skip facet"
                            );
                        }
                    }
                }
            }
        }

        let effective_tenant = if input_spec.tenant_id.is_empty() {
            ctx.tenant_id().to_string()
        } else {
            input_spec.tenant_id.clone()
        };

        let spawn_spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: local_actor_id.name().to_string(),
                actor_type: resolved_actor_type.clone(),
            }),
            role,
            namespace: effective_namespace.clone(),
            tenant_id: effective_tenant,
            visibility: input_spec.visibility,
            behavior_kind: input_spec.behavior_kind.clone(),
            args: init_args,
            facets: facets_proto,
            config: input_spec.config.clone(),
            labels: input_spec.labels.clone(),
            register_in_object_registry: input_spec.register_in_object_registry,
            enforce_unique_placement: input_spec.enforce_unique_placement,
            placement_strategy: input_spec.placement_strategy,
        };

        actor_factory
            .spawn_actor(ctx, &spawn_spec, facet_boxes)
            .await?;

        let registry = self.get_actor_registry().await;
        let routing_ctx = plexspaces_actor::RequestContext::new_without_auth(
            ctx.tenant_id().to_string(),
            effective_namespace,
        );
        Ok(Self::create_actor_ref_for_local_actor(
            &routing_ctx,
            &registry,
            &local_actor_id,
            &self.local_node_id,
            self.service_locator.clone(),
        )
        .await)
    }
}
