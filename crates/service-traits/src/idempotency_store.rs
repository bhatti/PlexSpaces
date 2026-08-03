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

//! Idempotency store trait for exactly-once message deduplication.
//!
//! # Purpose
//! Provides a shared, node-wide store that deduplicates messages by their
//! client-assigned idempotency key. A check-and-record operation is atomic:
//! concurrent retries of the same key race for the first-write slot; all
//! subsequent arrivals receive the cached response immediately.
//!
//! # Architecture Context
//! Lives in `plexspaces-service-traits` so that `plexspaces-mailbox` can
//! depend on it without pulling in heavier crates. The node creates one store
//! at startup (via `ServiceLocator`) and injects it into every real `Mailbox`.
//! Temporary-sender mailboxes (used for ask() internals) receive `None`.
//!
//! # Design
//! - **Atomic check-and-record**: Single call guarantees correctness under
//!   concurrent retries — no TOCTOU race possible with two separate methods.
//! - **Scoped by (tenant, namespace)**: Each bucket has its own LRU capacity.
//! - **Two backends**: in-memory (default) and persistent (SQLite / Postgres).

use async_trait::async_trait;
use bytes::Bytes;

/// Result type for idempotency store operations.
pub type IdempotencyResult<T> = Result<T, IdempotencyError>;

/// Errors returned by `IdempotencyStore` operations.
#[derive(Debug, thiserror::Error)]
pub enum IdempotencyError {
    /// Underlying storage backend error.
    #[error("Idempotency storage error: {0}")]
    Storage(String),

    /// Serialization / deserialization error.
    #[error("Idempotency serialization error: {0}")]
    Serialization(String),

    /// Configuration error (bad capacity, bad DSN, etc.).
    #[error("Idempotency configuration error: {0}")]
    Configuration(String),
}

/// Outcome returned by [`IdempotencyStore::check_and_record`].
#[derive(Debug)]
pub enum IdempotencyOutcome {
    /// This key was seen for the first time.
    /// The caller should process the message and then call
    /// [`IdempotencyStore::complete_record`] with the response.
    FirstSeen,

    /// This key was already processed and the response is cached.
    /// Return the cached response to the caller without re-processing.
    Duplicate(Option<Bytes>),

    /// A concurrent request is already in-flight for this key.
    /// The caller should wait or return a "processing" response.
    InFlight,
}

/// Shared idempotency deduplication store.
///
/// # Concurrency Contract
/// `check_and_record` is the atomic entry point. Under concurrent retries with
/// the same key, exactly one caller gets `FirstSeen`; all others get either
/// `Duplicate` (if already complete) or `InFlight` (while the first is still
/// processing).
///
/// After processing, the caller that got `FirstSeen` must call
/// `complete_record` to store the response and unblock any `InFlight` waiters.
///
/// # Scope
/// All operations are scoped to `(tenant_id, namespace)` — one noisy tenant
/// cannot evict another tenant's entries.
#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Atomically check whether `key` was seen before and record it as in-flight
    /// if it is new.
    ///
    /// - Returns `FirstSeen` → process the message, then call `complete_record`.
    /// - Returns `Duplicate(response)` → return the cached response immediately.
    /// - Returns `InFlight` → another request is processing this key right now.
    async fn check_and_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
    ) -> IdempotencyResult<IdempotencyOutcome>;

    /// Finalize a previously recorded in-flight key with its response.
    ///
    /// Must be called after a `FirstSeen` response from `check_and_record`.
    /// Transitions the entry from in-flight to complete and stores `response`
    /// for future duplicate requests.
    async fn complete_record(
        &self,
        tenant_id: &str,
        namespace: &str,
        key: &str,
        response: Option<Bytes>,
    ) -> IdempotencyResult<()>;

    /// Remove expired entries across all buckets.
    ///
    /// Called periodically by a background maintenance task. Returns the number
    /// of entries evicted.
    async fn cleanup_expired(&self) -> IdempotencyResult<usize>;
}
