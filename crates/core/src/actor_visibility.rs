// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Spawn [`ActorVisibility`](plexspaces_proto::actor::v1::ActorVisibility) policy helpers.
//!
//! Enforcement runs on `ActorRef` `tell` / `ask` only (see `enforce_visibility_for_actor_ref_messaging`).
//! The actor registry does not run a separate visibility pass; local delivery uses senders that enforce on send.
//!
//! Uses [`RequestContext::auth_enabled`] (`auth_disabled = !auth_enabled`). When the context's
//! `internal` field is `true`, enforcement is skipped so trusted system paths can operate without
//! duplicating visibility metadata.

use plexspaces_proto::actor::v1::ActorVisibility;

use crate::RequestContext;

/// Applies spawn-time visibility to a messaging operation.
///
/// `owner_namespace` is the actor isolation namespace (spec or canonical id).
/// When authentication is disabled on the context, `PROTECTED` behaves like `PUBLIC`;
/// `PRIVATE` still requires namespace alignment.
pub fn check_actor_visibility_for_messaging(
    auth_disabled: bool,
    visibility_raw: i32,
    owner_tenant_id: &str,
    owner_namespace: &str,
    caller_tenant_id: &str,
    caller_namespace: &str,
) -> Result<(), String> {
    let vis = match ActorVisibility::try_from(visibility_raw) {
        Ok(ActorVisibility::ActorVisibilityUnspecified) | Err(_) => {
            ActorVisibility::ActorVisibilityPublic
        }
        Ok(v) => v,
    };

    match vis {
        ActorVisibility::ActorVisibilityPublic | ActorVisibility::ActorVisibilityUnspecified => {
            Ok(())
        }
        ActorVisibility::ActorVisibilityProtected => {
            if auth_disabled {
                return Ok(());
            }
            if owner_tenant_id.is_empty() {
                return Ok(());
            }
            if caller_tenant_id == owner_tenant_id {
                Ok(())
            } else {
                Err(
                    "PROTECTED actor requires caller tenant_id to match owner (owner tenant set)"
                        .to_string(),
                )
            }
        }
        ActorVisibility::ActorVisibilityPrivate => {
            if caller_namespace != owner_namespace {
                return Err(
                    "PRIVATE actor requires caller namespace to match actor namespace".to_string(),
                );
            }
            if auth_disabled {
                return Ok(());
            }
            if owner_tenant_id.is_empty() {
                return Ok(());
            }
            if caller_tenant_id == owner_tenant_id {
                Ok(())
            } else {
                Err("PRIVATE actor requires caller tenant_id to match owner".to_string())
            }
        }
    }
}

/// Enforces tell/ask visibility for a concrete actor handle (`ActorRef` in the actor crate).
///
/// Uses only spawn-time owner tenant, namespace, and visibility carried on the ref.
pub fn enforce_visibility_for_actor_ref_messaging(
    ctx: &RequestContext,
    spawn_owner_tenant_id: &str,
    spawn_owner_namespace: &str,
    spawn_visibility: ActorVisibility,
) -> Result<(), String> {
    if ctx.internal {
        return Ok(());
    }
    let auth_disabled = !ctx.auth_enabled;
    check_actor_visibility_for_messaging(
        auth_disabled,
        spawn_visibility as i32,
        spawn_owner_tenant_id,
        spawn_owner_namespace,
        ctx.tenant_id(),
        ctx.namespace(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_always_ok() {
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityPublic as i32,
            "t1",
            "ns1",
            "t2",
            "ns2",
        )
        .is_ok());
    }

    #[test]
    fn unspecified_treated_as_public() {
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityUnspecified as i32,
            "t1",
            "ns1",
            "t2",
            "ns2",
        )
        .is_ok());
    }

    #[test]
    fn protected_auth_on_tenant_match() {
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityProtected as i32,
            "t1",
            "ns1",
            "t1",
            "ns2",
        )
        .is_ok());
    }

    #[test]
    fn protected_auth_on_tenant_mismatch() {
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityProtected as i32,
            "t1",
            "ns1",
            "t2",
            "ns1",
        )
        .is_err());
    }

    #[test]
    fn protected_auth_disabled_skips_tenant() {
        assert!(check_actor_visibility_for_messaging(
            true,
            ActorVisibility::ActorVisibilityProtected as i32,
            "t1",
            "ns1",
            "t2",
            "ns2",
        )
        .is_ok());
    }

    #[test]
    fn private_namespace_enforced_even_when_auth_disabled() {
        assert!(check_actor_visibility_for_messaging(
            true,
            ActorVisibility::ActorVisibilityPrivate as i32,
            "t1",
            "ns1",
            "t2",
            "ns1",
        )
        .is_ok());
        assert!(check_actor_visibility_for_messaging(
            true,
            ActorVisibility::ActorVisibilityPrivate as i32,
            "t1",
            "ns1",
            "t2",
            "ns2",
        )
        .is_err());
    }

    #[test]
    fn private_auth_on_requires_tenant_when_owner_tenant_set() {
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityPrivate as i32,
            "t1",
            "ns1",
            "t1",
            "ns1",
        )
        .is_ok());
        assert!(check_actor_visibility_for_messaging(
            false,
            ActorVisibility::ActorVisibilityPrivate as i32,
            "t1",
            "ns1",
            "t2",
            "ns1",
        )
        .is_err());
    }
}
