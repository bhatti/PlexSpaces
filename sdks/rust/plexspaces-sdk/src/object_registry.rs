// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Object Registry SDK helpers — re-exports from plexspaces_actor.
//
// Provides ergonomic wrappers for registering actors, discovering them
// by type, and looking them up by identity (Orleans grain directory pattern).

#[cfg(feature = "native")]
pub use plexspaces_actor::object_registry_helpers::{
    build_actor_alias, discover_actors_by_type, lookup_actor_by_identity, register_actor,
    unregister_actor,
};
