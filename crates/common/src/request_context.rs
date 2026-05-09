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

//! Request Context type alias and error definitions.
//!
//! ## Design
//! `RequestContext` is now the prost-generated type from `plexspaces_proto::common::v1`.
//! All ergonomic methods (accessors, builders, smart constructors) live in
//! [`crate::RequestContextExt`].
//!
//! ## Usage
//! ```rust
//! use plexspaces_common::{RequestContext, RequestContextExt};
//!
//! let ctx = RequestContext::new_without_auth("tenant".into(), "ns".into());
//! assert_eq!(ctx.tenant_id(), "tenant");
//! ```

/// The canonical `RequestContext` type — the prost-generated proto struct.
///
/// All fields are public (prost convention). For ergonomic method access import
/// [`crate::RequestContextExt`].
pub use plexspaces_proto::common::v1::RequestContext;

pub use crate::request_context_ext::RequestContextExt;

/// Hint appended to auth errors so users know how to fix or disable auth for testing.
/// Use when returning 401/Unauthenticated so clients get actionable guidance.
pub const AUTH_REQUIRED_HINT: &str =
    " Authentication required: provide a valid JWT in Authorization header (HTTP) or use mTLS (gRPC). For local testing, set PLEXSPACES_DISABLE_AUTH=1.";

/// Errors that can occur when constructing or validating a `RequestContext`.
#[derive(Debug, thiserror::Error)]
pub enum RequestContextError {
    /// Missing required tenant_id (when auth is enabled).
    #[error("Missing required tenant_id in RequestContext.{AUTH_REQUIRED_HINT}")]
    MissingTenantId,
}

impl RequestContextError {
    /// Return the proto error code for this error.
    pub fn code(&self) -> plexspaces_proto::common::v1::RequestContextErrorCode {
        use plexspaces_proto::common::v1::RequestContextErrorCode;
        match self {
            RequestContextError::MissingTenantId => {
                RequestContextErrorCode::RequestContextErrorMissingTenantId
            }
        }
    }
}
