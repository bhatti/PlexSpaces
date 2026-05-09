// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Register [`RuntimeConfig`](plexspaces_proto::node::v1::RuntimeConfig) and security settings
//! from a loaded [`ReleaseSpec`](plexspaces_proto::node::v1::ReleaseSpec) into [`ServiceLocator`](plexspaces_actor::ServiceLocator).

use std::sync::Arc;

use plexspaces_actor::InitializableServiceLocator;

/// Pushes `runtime` and optional `security` from `release_spec` into the service locator.
///
/// Node startup calls this after release spec is available so downstream services read config
/// only from the locator, not by reaching back into node-owned state.
pub async fn register_runtime_and_security_from_release(
    service_locator: Arc<dyn InitializableServiceLocator + Send + Sync>,
    release_spec: &plexspaces_proto::node::v1::ReleaseSpec,
) {
    if let Some(ref runtime) = release_spec.runtime {
        service_locator
            .register_runtime_config(runtime.clone())
            .await;
    }
    if let Some(ref security) = release_spec.runtime.as_ref().and_then(|r| r.security.as_ref()) {
        service_locator
            .register_security_config((*security).clone())
            .await;
    }
}
