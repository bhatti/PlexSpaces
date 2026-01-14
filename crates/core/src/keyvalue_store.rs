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

//! KeyValueStore trait for key-value store implementations

use async_trait::async_trait;
use crate::RequestContext;

use std::time::Duration;

/// Trait for key-value store implementations
#[async_trait]
pub trait KeyValueStore: Send + Sync {
    /// Get value for key
    async fn get(&self, ctx: &RequestContext, key: &str) -> Result<Option<Vec<u8>>, String>;
    /// Put value for key (overwrites existing)
    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>) -> Result<(), String>;
    /// Put value for key with TTL (overwrites existing)
    async fn put_with_ttl(&self, ctx: &RequestContext, key: &str, value: Vec<u8>, ttl: Duration) -> Result<(), String>;
    /// Delete key
    async fn delete(&self, ctx: &RequestContext, key: &str) -> Result<(), String>;
    /// Check if key exists
    async fn exists(&self, ctx: &RequestContext, key: &str) -> Result<bool, String>;
    /// List all keys matching prefix
    async fn list_keys(&self, ctx: &RequestContext, prefix: &str) -> Result<Vec<String>, String>;
    /// List all keys matching prefix (alias for list_keys)
    async fn list(&self, ctx: &RequestContext, prefix: &str) -> Result<Vec<String>, String> {
        self.list_keys(ctx, prefix).await
    }
    /// Compare-and-swap: only put if current value matches expected
    async fn cas(&self, ctx: &RequestContext, key: &str, expected: Option<Vec<u8>>, new_value: Vec<u8>) -> Result<bool, String>;
    /// Atomic increment (for counters/metrics)
    async fn increment(&self, ctx: &RequestContext, key: &str, delta: i64) -> Result<i64, String>;
}

