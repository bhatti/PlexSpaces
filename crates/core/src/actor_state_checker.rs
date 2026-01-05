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

//! Actor state fetcher helper
//!
//! This module provides a helper function to fetch actor state from plexspaces_core
//! without creating a circular dependency. Uses dynamic dispatch via ActorStateFetcher trait.
//!
//! ## Why This Trait Exists
//!
//! `plexspaces_core` cannot import `plexspaces_actor::Actor` directly due to circular dependencies:
//! - `plexspaces_actor` depends on `plexspaces_core` for core types
//! - `plexspaces_core` needs to check actor state (e.g., in `VirtualActorManager::is_active()`)
//!
//! The `ActorStateFetcher` trait provides a clean abstraction that allows `plexspaces_core` to
//! fetch actor state without importing the concrete `Actor` type. The trait is implemented by
//! `Actor` in the `plexspaces_actor` crate, and the helper function uses dynamic dispatch to
//! call the trait method.
//!
//! ## Usage Pattern
//!
//! 1. `Actor` implements `ActorStateFetcher` in `plexspaces_actor` crate
//! 2. `VirtualActorManager::is_active()` calls `ActorRegistry::is_actor_state_active()`
//! 3. `ActorRegistry::is_actor_state_active()` calls `get_actor_state()` and checks if it's `Active`
//! 4. `get_actor_state()` uses the trait to fetch state without importing `Actor`

use std::sync::Arc;
use async_trait::async_trait;

/// Trait for fetching actor state (implemented by Actor)
///
/// ## Purpose
/// Allows `plexspaces_core` to fetch actor state without importing `Actor` directly.
/// This breaks the circular dependency between `plexspaces_core` and `plexspaces_actor`.
///
/// ## Implementation
/// `Actor` in `plexspaces_actor` crate implements this trait to expose its state.
/// The trait method returns the actor's current state as a proto `ActorState` enum.
///
/// ## Usage
/// This trait is used by `ActorRegistry::is_actor_state_active()` to check if an actor
/// is truly active (state is `Active`). This is consistent for all actor types:
/// - Regular actors: state is `Active` when message loop is running
/// - Virtual actors (lazy): state is `Creating` until activated, then `Active`
/// - Virtual actors (eager): state is `Active` immediately after registration
/// - Workflows: state is `Active` when workflow is running
#[async_trait]
pub trait ActorStateFetcher: std::any::Any + Send + Sync {
    /// Get the actor's current state
    ///
    /// ## Returns
    /// The actor's current state as a proto `ActorState` enum value
    async fn get_state(&self) -> i32;
}

use std::sync::OnceLock;

/// Callback type for fetching actor state
/// This callback is registered by code that can import `Actor` (in `plexspaces_actor` crate)
type StateFetcherCallback = fn(&Arc<dyn std::any::Any + Send + Sync>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<i32>> + Send>>;

/// Static callback for fetching actor state
/// This is set by code that can import `Actor` (in `plexspaces_actor` crate)
static STATE_FETCHER_CALLBACK: OnceLock<StateFetcherCallback> = OnceLock::new();

/// Register callback for fetching actor state
/// This should be called by code that can import `Actor` (in `plexspaces_actor` crate)
/// The callback uses `Arc::downcast` to get the concrete `Actor` type, then calls the trait method.
pub fn register_state_fetcher_callback(callback: StateFetcherCallback) {
    let _ = STATE_FETCHER_CALLBACK.set(callback);
}

/// Get actor state
///
/// ## Purpose
/// Helper function to get an actor instance's current state.
/// This is called from `ActorRegistry` to avoid circular dependency.
///
/// ## Implementation
/// Uses a callback pattern with the `ActorStateFetcher` trait. The callback is registered
/// by code that can import `Actor` (in `plexspaces_actor` crate). The callback uses
/// `Arc::downcast` to get the concrete `Actor` type, then calls the `ActorStateFetcher::get_state()`
/// trait method.
///
/// ## Usage
/// This function is called by:
/// - `ActorRegistry::get_actor_state()` - gets actor instance state
/// - `ActorRegistry::is_actor_state_active()` - checks if actor state is Active
///
/// ## Returns
/// `Option<i32>` - The actor's state as a proto `ActorState` enum value, or `None` if not an Actor or callback not registered
pub async fn get_actor_state(instance: &Arc<dyn std::any::Any + Send + Sync>) -> Option<i32> {
    // Use callback if registered (callback uses ActorStateFetcher trait)
    // The callback will handle downcasting to Actor and calling get_state()
    if let Some(callback) = STATE_FETCHER_CALLBACK.get() {
        callback(instance).await
    } else {
        // Callback not registered - return None as safe default
        // This should not happen in production as the callback is registered during initialization
        eprintln!("🔴 [ACTOR_STATE_CHECKER] get_actor_state: callback not registered!");
        None
    }
}
