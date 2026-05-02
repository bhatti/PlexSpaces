// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! HTTP REST bridge routes for the node crate.
//!
//! Each sub-module owns one domain: actors, nodes, deploy.
//! `all_http_routes()` composes them into a single `axum::Router`.

use std::sync::Arc;

use axum::Router;
use plexspaces_core::{NodeConnectivity, ServiceLocator};
use plexspaces_services::actor_service::ActorServiceImpl;

pub mod actor_routes;
pub mod deploy_routes;
pub mod node_routes;

pub use actor_routes::actor_router;
pub use deploy_routes::deploy_router;
pub use node_routes::node_router;

/// Compose all HTTP bridge routes into a single router.
pub fn all_http_routes(
    actor_service: Arc<ActorServiceImpl>,
    service_locator: Arc<dyn ServiceLocator>,
    node_connectivity: Arc<dyn NodeConnectivity>,
    auth_disabled: bool,
    jwt_secret: Option<String>,
) -> Router {
    actor_router(actor_service, auth_disabled, jwt_secret.clone())
        .merge(node_router(service_locator.clone()))
        .merge(deploy_router(
            service_locator,
            node_connectivity,
            auth_disabled,
            jwt_secret,
        ))
}
