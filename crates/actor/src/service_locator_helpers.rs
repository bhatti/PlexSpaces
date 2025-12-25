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

//! Helper functions for ServiceLocator integration with ActorFactory
//!
//! This module provides helper functions to work with ActorFactory in ServiceLocator,
//! avoiding TypeId mismatch issues when using trait objects.

use std::sync::Arc;
use crate::ActorFactory;

/// Get ActorFactory from ServiceLocator as a trait object
///
/// ## Purpose
/// Retrieves ActorFactory from ServiceLocator, converting from the type-erased
/// storage (Arc<dyn Any>) to the trait object (Arc<dyn ActorFactory>).
///
/// ## TypeId Safety
/// This function uses trait objects to avoid TypeId mismatch issues. Trait objects
/// have stable TypeIds regardless of import paths, so this works even when the
/// service is registered from a different crate.
///
/// ## Arguments
/// * `service_locator` - The ServiceLocator to retrieve from
///
/// ## Returns
/// `Some(Arc<dyn ActorFactory>)` if registered, `None` otherwise
pub async fn get_actor_factory(
    service_locator: &plexspaces_core::ServiceLocator,
) -> Option<Arc<dyn ActorFactory>> {
    let factory_any = service_locator.get_actor_factory().await?;
    
    // Convert Arc<dyn Any> back to Arc<dyn ActorFactory>
    // This is safe because:
    // 1. The factory was registered as Arc<dyn ActorFactory> and cast to Arc<dyn Any>
    // 2. We know it's actually Arc<dyn ActorFactory> (enforced by registration)
    // 3. The conversion is reversible since we're just changing the trait object type
    //
    // We can't use downcast because Arc<dyn Any> and Arc<dyn ActorFactory> are different
    // trait object types. However, since we know the original type, we can safely convert back.
    //
    // Safety: This is safe because:
    // 1. We know factory_any is actually Arc<dyn ActorFactory> (enforced by registration)
    // 2. The memory layout is compatible (both are Arc pointers to trait objects)
    // 3. We maintain the Arc reference count correctly
    unsafe {
        let ptr = Arc::into_raw(factory_any);
        // Transmute the pointer from *const (dyn Any + Send + Sync) to *const (dyn ActorFactory + Send + Sync)
        // This is safe because we know the original type was Arc<dyn ActorFactory>
        let actor_factory_ptr = std::mem::transmute::<
            *const (dyn std::any::Any + Send + Sync),
            *const (dyn ActorFactory + Send + Sync),
        >(ptr);
        Some(Arc::from_raw(actor_factory_ptr))
    }
}
