// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Shared helpers for integration tests that call [`Actor::start`], which registers the actor
// via [`plexspaces_core::ServiceLocator::actor_registry`]. Plain [`Actor::new`] installs a stub
// locator without a registry and causes `register_in_registry` to fail.

use plexspaces_actor::Actor;
use plexspaces_core::{ActorContext, ActorId, ServiceLocator};
use plexspaces_mailbox::Mailbox;
use std::sync::Arc;

/// Builds an [`Actor`] whose context uses [`plexspaces_node::create_default_service_locator`]
/// so [`Actor::start`] can register with [`ActorRegistry`].
pub async fn actor_with_default_service_locator(
    id: ActorId,
    behavior: Box<dyn plexspaces_core::Actor>,
    mailbox: Mailbox,
    tenant_id: String,
    namespace: String,
) -> Actor {
    let locator_impl = plexspaces_node::create_default_service_locator(None, None).await;
    let node_id = ServiceLocator::get_node_config(locator_impl.as_ref())
        .await
        .map(|n| n.id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "test-node".to_string());
    let service_locator: Arc<dyn ServiceLocator> = locator_impl;
    let context = Arc::new(ActorContext::new(
        node_id,
        tenant_id.clone(),
        namespace.clone(),
        service_locator,
        None,
    ));
    Actor::new(id, behavior, mailbox, tenant_id, namespace, None).set_context(context)
}
