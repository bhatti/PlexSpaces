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

//! PlexSpaces Dashboard
//!
//! ## Purpose
//! Provides dashboard service and UI for monitoring PlexSpaces nodes, applications, actors, and workflows.
//!
//! ## Architecture Context
//! - Dashboard service aggregates metrics from all nodes
//! - HTTP handlers serve HTML pages and static assets
//! - Supports filtering, pagination, and real-time updates

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod dashboard_handlers;

pub use dashboard_handlers::create_dashboard_router;
pub use plexspaces_services::dashboard_service::{DashboardServiceImpl, HealthReporterAccess};
