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

//! Tests for error types

use plexspaces_blob::BlobError;

#[test]
fn test_error_from_str() {
    let err: BlobError = "test error".into();
    match err {
        BlobError::InternalError(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Expected InternalError"),
    }
}

#[test]
fn test_error_from_string() {
    let err: BlobError = "test error".to_string().into();
    match err {
        BlobError::InternalError(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Expected InternalError"),
    }
}

#[test]
fn test_error_display() {
    let err = BlobError::ConfigError("invalid config".to_string());
    assert!(format!("{}", err).contains("invalid config"));

    let err = BlobError::NotFound("blob-1".to_string());
    assert!(format!("{}", err).contains("blob-1"));

    let err = BlobError::InvalidInput("empty data".to_string());
    assert!(format!("{}", err).contains("empty data"));
}
