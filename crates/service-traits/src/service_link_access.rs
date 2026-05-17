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

//! ServiceLinkAccess — thin trait for dashboard access to live service links.
//!
//! # Purpose
//! Allows `DashboardServiceImpl` to query all service links (static + dynamically
//! added via gRPC) without depending on the `plexspaces-services` crate directly,
//! which would create a circular dependency.

use async_trait::async_trait;
use plexspaces_common::RequestContext;
use plexspaces_proto::node::v1::ServiceLinkConfig;

/// Read-only access to the live service link catalog.
///
/// # Purpose
/// Implemented by `ServiceLinkServiceImpl` and stored in `ServiceLocator` so the
/// dashboard can list all service links for a node without circular crate dependencies.
#[async_trait]
pub trait ServiceLinkAccess: Send + Sync {
    /// List all service links visible to the caller's tenant context.
    async fn list_links(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<ServiceLinkConfig>, Box<dyn std::error::Error + Send + Sync>>;
}
