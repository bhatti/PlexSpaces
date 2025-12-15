// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Event sourcing and state persistence
//!
//! Provides journaling and snapshot capabilities for durable actor execution.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Export the journal module
mod r#mod;

// Export execution context module
pub mod execution_context;

// Export codec module (compression and encryption)
pub mod codec;

// Export configuration module
pub mod config;

// Re-export all public items
pub use r#mod::*;
