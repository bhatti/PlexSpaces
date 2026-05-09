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

//! Re-exports `MessageSender` from `plexspaces-service-traits`.
//!
//! The canonical definition now lives in `plexspaces-service-traits` so that
//! it can be used by `plexspaces-journaling` and other crates without
//! depending on `plexspaces-core`. This module is a thin re-export shim so
//! existing `use plexspaces_actor::MessageSender` imports keep working.

pub use plexspaces_service_traits::MessageSender;
