// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Resolve HTTP / gRPC-gateway auth parameters from a [`ServiceLocator`](plexspaces_actor::ServiceLocator).

use std::sync::Arc;

use plexspaces_actor::ServiceLocator;

/// Returns `(auth_disabled, jwt_key_pair)` for wiring HTTP routes and gateway middleware.
///
/// Resolves the JWT key pair from the JwtConfig proto (which already has env var overrides applied
/// by the config converter). Resolution order:
/// 1. `private_key_pem` (from config/env `PLEXSPACES_JWT_PRIVATE_KEY`)
/// 2. `private_key_file` (from config/env `PLEXSPACES_JWT_PRIVATE_KEY_FILE`)
/// 3. `secret` (HS256 fallback from config/env `PLEXSPACES_JWT_SECRET`)
/// 4. Auto-generate ephemeral ES256 key (when `auto_generate_key` is true)
pub async fn http_jwt_auth_snapshot(
    service_locator: Arc<dyn ServiceLocator + Send + Sync>,
) -> (bool, Option<Arc<crate::jwt_keys::JwtKeyPair>>) {
    let auth_disabled = service_locator.is_auth_disabled().await;
    let jwt_config = service_locator
        .get_security_config()
        .await
        .and_then(|c| c.jwt);

    let jwt_key_pair = match jwt_config {
        Some(cfg) => crate::jwt_keys::JwtKeyPair::from_config(
            &cfg.private_key_pem,
            &cfg.private_key_file,
            &cfg.secret,
            cfg.auto_generate_key,
        )
        .map(Arc::new)
        .ok(),
        None => crate::jwt_keys::JwtKeyPair::from_env(None).map(Arc::new).ok(),
    };

    (auth_disabled, jwt_key_pair)
}
