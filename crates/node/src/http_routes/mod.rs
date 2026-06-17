// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge routes for the node crate.
//!
//! Each sub-module owns one domain: actors, nodes, deploy, auth.
//! `all_http_routes()` composes them into a single `axum::Router`.

use std::sync::Arc;

use axum::Router;
use plexspaces_actor::{NodeConnectivity, ServiceLocator};
use plexspaces_services::actor_service::ActorServiceImpl;

pub mod actor_routes;
pub mod auth_routes;
pub mod deploy_routes;
pub mod node_routes;

pub use actor_routes::actor_router;
pub use auth_routes::{auth_router, AuthRouteState};
pub use deploy_routes::deploy_router;
pub use node_routes::node_router;

/// Compose all HTTP bridge routes into a single router.
pub fn all_http_routes(
    actor_service: Arc<ActorServiceImpl>,
    service_locator: Arc<dyn ServiceLocator>,
    node_connectivity: Arc<dyn NodeConnectivity>,
    auth_disabled: bool,
    jwt_key_pair: Option<Arc<plexspaces_grpc_middleware::JwtKeyPair>>,
    auth_state: Option<AuthRouteState>,
) -> Router {
    let tenant_repo = auth_state.as_ref().map(|s| s.tenant_repo.clone());
    let base = actor_router(actor_service, auth_disabled, jwt_key_pair.clone())
        .merge(node_router(service_locator.clone(), auth_disabled, jwt_key_pair.clone()))
        .merge(deploy_router(
            service_locator,
            node_connectivity,
            auth_disabled,
            jwt_key_pair,
            tenant_repo,
        ));

    if let Some(state) = auth_state {
        base.merge(auth_router(state))
    } else {
        base
    }
}
