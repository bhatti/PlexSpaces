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

//! Re-exports `ActorId` and related types from `plexspaces-service-traits`.
//!
//! The canonical definition now lives in `plexspaces-service-traits` so that
//! `plexspaces-journaling` can use `ActorId` without depending on
//! `plexspaces-core`. This module is a thin re-export shim so existing
//! `use plexspaces_actor::ActorId` imports keep working without changes.

pub use plexspaces_service_traits::{
    wasm_root_supervisor_actor_type_from_application_name,
    wasm_worker_actor_type_from_application_name,
    ActorId,
    ActorIdError,
};
