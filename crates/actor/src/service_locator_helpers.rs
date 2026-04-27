// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Helper functions for ServiceLocator integration with ActorFactory
//!
//! ## Note
//! This module previously contained helper functions for accessing ActorFactory from ServiceLocator.
//! These functions have been moved to `plexspaces-services` crate to avoid circular dependencies.
//!
//! ## Migration
//! Use the `ServiceLocatorActorFactory` extension trait from `plexspaces-services` instead:
//!
//! ```rust,ignore
//! // Old (deprecated):
//! use plexspaces_actor::get_actor_factory;
//! let factory = get_actor_factory(service_locator.as_ref()).await?;
//!
//! // New (recommended):
//! use plexspaces_services::ServiceLocatorActorFactory;
//! let factory = service_locator.get_actor_factory().await?;
//! ```
//!
//! Or use the helper function from `plexspaces-services`:
//!
//! ```rust,ignore
//! use plexspaces_services::get_actor_factory;
//! let factory = get_actor_factory(service_locator.as_ref()).await?;
//! ```

// This module is kept for backwards compatibility documentation only.
// All functionality has been moved to plexspaces-services crate.
