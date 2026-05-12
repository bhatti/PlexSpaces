// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Helpers for `application.v1.ChildSpec` identity (`ActorIdentity`).

use crate::ApplicationError;
use plexspaces_actor::ActorId;
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::supervision::v1::ChildSpec;

/// Returns `actor_identity` when present and valid (same rules as [`ActorId`] / proto `ActorIdentity`).
pub fn require_child_identity(spec: &ChildSpec) -> Result<&ActorIdentity, ApplicationError> {
    let Some(ref id) = spec.actor_identity else {
        return Err(ApplicationError::ConfigError(
            "ChildSpec missing actor_identity (name + actor_type)".to_string(),
        ));
    };
    ActorId::validate_proto_actor_identity(id).map_err(|e| {
        ApplicationError::ConfigError(format!("ChildSpec.actor_identity invalid: {e}"))
    })?;
    Ok(id)
}
