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

//! `ChannelRepository` trait — tenant-isolated durable channel lifecycle.
//!
//! ## Purpose
//! Abstracts over how durable `Channel` instances are obtained. The actor
//! factory calls `get_or_create()` with a `RequestContext`; the repository
//! returns an `Arc<dyn Channel>` without the factory needing to know anything
//! about SQL, pools, or connection strings.
//!
//! ## Implementations
//! Both `SqliteChannelRepository` and `PostgresChannelRepository` live in
//! `sql.rs`, following the convention in `plexspaces-keyvalue` (`sql.rs`) and
//! `plexspaces-object-registry` (`repository/sql.rs`).
//!
//! ## Tenant Isolation
//! The cache key is `(tenant_id, namespace, name)` — mirroring the kv_store
//! PRIMARY KEY `(tenant_id, namespace, key)`. SQL queries inside each backend
//! further scope every read/write to `tenant_id` and `namespace`.

use crate::ChannelResult;
use async_trait::async_trait;
use plexspaces_common::RequestContext;
use std::sync::Arc;

/// Obtain (or create) a durable channel scoped to a `RequestContext`.
///
/// Implementations cache channels by `(tenant_id, namespace, name)` so that
/// repeated calls for the same actor return the same `Arc<dyn Channel>`.
#[async_trait]
pub trait ChannelRepository: Send + Sync {
    /// Return an existing channel or create one backed by the shared DB pool.
    ///
    /// * `ctx`  — supplies `tenant_id` and `namespace` for tenant isolation.
    /// * `name` — logical channel name (typically the actor's string ID).
    async fn get_or_create(
        &self,
        ctx: &RequestContext,
        name: &str,
    ) -> ChannelResult<Arc<dyn crate::Channel>>;
}
