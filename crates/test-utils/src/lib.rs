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

//! # PlexSpaces Test Utilities
//!
//! Shared test helpers used across all PlexSpaces crates. This crate eliminates
//! the 38+ duplicate `create_test_message` definitions that were scattered across
//! the workspace.
//!
//! # Usage
//!
//! Add to your crate's `[dev-dependencies]`:
//! ```toml
//! plexspaces-test-utils = { workspace = true }
//! ```
//!
//! Then import in test modules:
//! ```rust,no_run
//! use plexspaces_test_utils::messages::create_test_message;
//! use plexspaces_test_utils::env::setup_aws_local_env;
//! ```

pub mod actors;
pub mod env;
pub mod messages;
