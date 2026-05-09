// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Simplified TupleSpaceProvider trait for actor/WASM use.
//!
//! This is the actor-facing interface for TupleSpace access.
//! The full TupleSpaceProvider (with capabilities, tenant, namespace, barriers, etc.)
//! lives in `plexspaces-tuplespace`. This simplified version is used by actors and
//! WASM instances where only basic read/write/take/count operations are needed.

use async_trait::async_trait;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};

/// Simplified TupleSpace access for actors and WASM instances.
#[async_trait]
pub trait TupleSpaceProvider: Send + Sync {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError>;
    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError>;
    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError>;
    async fn count(&self, pattern: &Pattern) -> Result<usize, TupleSpaceError>;
}
