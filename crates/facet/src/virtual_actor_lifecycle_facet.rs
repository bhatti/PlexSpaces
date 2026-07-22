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

//! Virtual Actor Lifecycle Facet Trait
//!
//! ## Purpose
//! Defines the interface for virtual actor lifecycle management without creating
//! circular dependencies. This trait is implemented by `VirtualActorFacet` in
//! the `journaling` crate, allowing `core` to work with virtual actor facets
//! without depending on `journaling`.
//!
//! ## Architecture Context
//! - Defined in `core` crate (no dependencies on `journaling`)
//! - Implemented by `VirtualActorFacet` in `journaling` crate
//! - Used by `VirtualActorManager` to store and manage virtual actor lifecycle
//!
//! ## Design Decision
//! Uses trait-based design instead of `Any` types for type safety and clarity.
//! This eliminates the need for unsafe downcasting and provides compile-time
//! guarantees.

use async_trait::async_trait;
use plexspaces_common::ActivationStrategy;
use std::time::{Duration, SystemTime};

/// Lifecycle state for virtual actors
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualActorLifecycleState {
    /// Last activation time
    pub last_activated: Option<SystemTime>,
    /// Last access time
    pub last_accessed: Option<SystemTime>,
    /// Activation count
    pub activation_count: u32,
    /// Is currently activating (prevents duplicate activations)
    pub is_activating: bool,
    /// Idle timeout before deactivation
    pub idle_timeout: Duration,
}

/// Trait for virtual actor lifecycle management
///
/// ## Purpose
/// Provides lifecycle management methods for virtual actors without requiring
/// `core` to depend on `journaling` crate. This trait is implemented by
/// `VirtualActorFacet` and used by `VirtualActorManager`.
///
/// ## Implementation
/// `VirtualActorFacet` in `plexspaces-journaling` implements this trait.
#[async_trait]
pub trait VirtualActorLifecycleFacet: Send + Sync + std::fmt::Debug {
    /// Get activation strategy (lazy, eager, prewarm)
    async fn get_activation_strategy(&self) -> ActivationStrategy;

    /// Get current lifecycle state
    async fn get_lifecycle_state(&self) -> VirtualActorLifecycleState;

    /// Check if actor should be activated
    async fn should_activate(&self) -> bool;

    /// Check if actor should be deactivated (idle timeout exceeded)
    async fn should_deactivate(&self) -> bool;

    /// Start activation process (returns false if already activating)
    async fn start_activation(&self) -> bool;

    /// Mark actor as activated
    async fn mark_activated(&self);

    /// Mark actor as deactivated
    async fn mark_deactivated(&self);

    /// Update last access time
    async fn update_access_time(&self);
}
