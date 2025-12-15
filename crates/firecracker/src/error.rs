// SPDX-License-Identifier: LGPL-2.1-or-later
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

//! Firecracker-specific error types

/// Firecracker error type
#[derive(Debug, thiserror::Error)]
pub enum FirecrackerError {
    /// VM creation failed
    #[error("VM creation failed: {0}")]
    VmCreationFailed(String),

    /// VM boot failed
    #[error("VM boot failed: {0}")]
    VmBootFailed(String),

    /// VM operation failed
    #[error("VM operation failed: {0}")]
    VmOperationFailed(String),

    /// Network setup failed
    #[error("Network setup failed: {0}")]
    NetworkSetupFailed(String),

    /// Firecracker API error
    #[error("Firecracker API error: {0}")]
    ApiError(String),

    /// VM not found
    #[error("VM not found: {0}")]
    VmNotFound(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// HTTP client error
    #[error("HTTP client error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

/// Firecracker Result type
pub type FirecrackerResult<T> = std::result::Result<T, FirecrackerError>;
