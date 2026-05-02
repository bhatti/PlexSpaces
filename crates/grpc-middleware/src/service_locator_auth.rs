// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Resolve HTTP / gRPC-gateway auth parameters from a [`ServiceLocator`](plexspaces_core::ServiceLocator).

use std::sync::Arc;

use plexspaces_core::ServiceLocator;

/// Returns `(auth_disabled, jwt_secret)` for wiring HTTP routes and gateway middleware.
pub async fn http_jwt_auth_snapshot(
    service_locator: Arc<dyn ServiceLocator + Send + Sync>,
) -> (bool, Option<String>) {
    let auth_disabled = service_locator.is_auth_disabled().await;
    let jwt_secret = service_locator
        .get_security_config()
        .await
        .and_then(|c| c.jwt)
        .and_then(|j| if j.secret.is_empty() { None } else { Some(j.secret) });
    (auth_disabled, jwt_secret)
}
