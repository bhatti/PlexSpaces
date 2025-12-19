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

//! Behavior Factory for Dynamic Actor Spawning
//!
//! ## Purpose
//! Enables native Rust applications to dynamically spawn actors from `start_module` strings
//! in ApplicationSpec supervisor trees. This bridges the gap between proto-defined supervisor
//! trees and runtime actor instantiation.
//!
//! ## Problem
//! Native Rust applications cannot dynamically instantiate behaviors from `start_module` strings
//! at runtime because Rust doesn't support dynamic code loading. The `start_module` field in
//! `ChildSpec` is a string like `"my_app::Worker"`, but there's no way to convert this string
//! to a `Box<dyn ActorBehavior>` at runtime without a registry.
//!
//! ## Solution
//! The BehaviorFactory trait and BehaviorRegistry provide a registry pattern where behavior
//! constructors are registered at compile-time or startup, mapping module names to constructors.
//!
//! ## Usage
//! ```rust
//! use plexspaces_core::behavior_factory::{BehaviorFactory, BehaviorRegistry};
//!
//! // Register behaviors at startup
//! let mut registry = BehaviorRegistry::new();
//! registry.register("my_app::Worker", || Box::new(Worker::new()));
//! registry.register("my_app::Coordinator", || Box::new(Coordinator::new()));
//!
//! // Use in application
//! let behavior = registry.create("my_app::Worker", &[])?;
//! ```
//!
//! ## Design
//! - **BehaviorFactory**: Trait for creating behaviors from module names
//! - **BehaviorRegistry**: Default implementation using a HashMap
//! - **Registration**: Behaviors registered at compile-time or startup
//! - **Error Handling**: Returns `BehaviorFactoryError` for unknown modules or creation failures

use crate::Actor;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Error types for behavior factory operations
#[derive(Error, Debug)]
pub enum BehaviorFactoryError {
    /// Unknown module name
    #[error("Unknown behavior module: {0}")]
    UnknownModule(String),

    /// Failed to create behavior
    #[error("Failed to create behavior from module '{0}': {1}")]
    CreationFailed(String, String),

    /// Invalid arguments
    #[error("Invalid arguments for module '{0}': {1}")]
    InvalidArguments(String, String),
}

/// Trait for creating actor behaviors from module names
///
/// ## Purpose
/// Allows applications to dynamically create behaviors from `start_module` strings
/// defined in ApplicationSpec supervisor trees.
///
/// ## Implementation
/// Implementations should maintain a registry mapping module names to constructors.
/// The registry can be populated at compile-time (via macros) or at runtime (via registration).
///
/// ## Example
/// ```rust
/// struct MyBehaviorFactory {
///     registry: HashMap<String, Box<dyn Fn(&[u8]) -> Result<Box<dyn Actor>, BehaviorFactoryError>>>,
/// }
///
/// #[async_trait::async_trait]
/// impl BehaviorFactory for MyBehaviorFactory {
///     async fn create(&self, module: &str, args: &[u8]) -> Result<Box<dyn Actor>, BehaviorFactoryError> {
///         let constructor = self.registry.get(module)
///             .ok_or_else(|| BehaviorFactoryError::UnknownModule(module.to_string()))?;
///         constructor(args)
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait BehaviorFactory: Send + Sync {
    /// Create a behavior from a module name
    ///
    /// ## Arguments
    /// * `module` - Module name (e.g., "my_app::Worker")
    /// * `args` - Optional arguments (protobuf-encoded, from ChildSpec.args)
    ///
    /// ## Returns
    /// * `Ok(Box<dyn ActorBehavior>)` - Created behavior
    /// * `Err(BehaviorFactoryError)` - Creation failed
    async fn create(
        &self,
        module: &str,
        args: &[u8],
    ) -> Result<Box<dyn Actor>, BehaviorFactoryError>;

    /// Check if a module is registered
    ///
    /// ## Arguments
    /// * `module` - Module name to check
    ///
    /// ## Returns
    /// `true` if module is registered, `false` otherwise
    async fn is_registered(&self, module: &str) -> bool;
}

/// Type alias for behavior constructor functions
pub type BehaviorConstructor = dyn Fn(&[u8]) -> Result<Box<dyn Actor>, BehaviorFactoryError> + Send + Sync;

/// Default implementation of BehaviorFactory using a HashMap registry
///
/// ## Purpose
/// Provides a simple, thread-safe registry for behavior constructors.
///
/// ## Usage
/// ```rust
/// let mut registry = BehaviorRegistry::new();
/// registry.register("my_app::Worker", |_| Ok(Box::new(Worker::new())));
///
/// let behavior = registry.create("my_app::Worker", &[])?;
/// ```
pub struct BehaviorRegistry {
    constructors: Arc<tokio::sync::RwLock<HashMap<String, Box<BehaviorConstructor>>>>,
}

impl BehaviorRegistry {
    /// Create a new behavior registry
    pub fn new() -> Self {
        Self {
            constructors: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Register a behavior constructor
    ///
    /// ## Arguments
    /// * `module` - Module name (e.g., "my_app::Worker")
    /// * `constructor` - Function that creates the behavior
    ///
    /// ## Example
    /// ```rust
    /// registry.register("my_app::Worker", |args| {
    ///     // Parse args if needed
    ///     Ok(Box::new(Worker::new()))
    /// });
    /// ```
    pub async fn register<F>(&mut self, module: impl Into<String>, constructor: F)
    where
        F: Fn(&[u8]) -> Result<Box<dyn Actor>, BehaviorFactoryError> + Send + Sync + 'static,
    {
        let mut constructors = self.constructors.write().await;
        constructors.insert(module.into(), Box::new(constructor));
    }

    /// Register a behavior constructor that doesn't use arguments
    ///
    /// ## Arguments
    /// * `module` - Module name
    /// * `constructor` - Function that creates the behavior (ignores args)
    ///
    /// ## Example
    /// ```rust
    /// registry.register_simple("my_app::Worker", || Worker::new()).await;
    /// ```
    pub async fn register_simple<F, B>(&mut self, module: impl Into<String>, constructor: F)
    where
        F: Fn() -> B + Send + Sync + 'static,
        B: Actor + 'static,
    {
        self.register(module, move |_| Ok(Box::new(constructor()))).await;
    }

    /// Get all registered module names
    pub async fn registered_modules(&self) -> Vec<String> {
        let constructors = self.constructors.read().await;
        constructors.keys().cloned().collect()
    }
}

impl Default for BehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Service trait so BehaviorRegistry can be registered in ServiceLocator
impl crate::Service for BehaviorRegistry {}

#[async_trait::async_trait]
impl BehaviorFactory for BehaviorRegistry {
    async fn create(
        &self,
        module: &str,
        args: &[u8],
    ) -> Result<Box<dyn Actor>, BehaviorFactoryError> {
        let constructors = self.constructors.read().await;
        let constructor = constructors
            .get(module)
            .ok_or_else(|| BehaviorFactoryError::UnknownModule(module.to_string()))?;

        constructor(args).map_err(|e| {
            BehaviorFactoryError::CreationFailed(module.to_string(), e.to_string())
        })
    }

    async fn is_registered(&self, module: &str) -> bool {
        let constructors = self.constructors.read().await;
        constructors.contains_key(module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Actor;
    use crate::Message;

    struct TestBehavior {
        id: String,
    }

    #[async_trait::async_trait]
    impl Actor for TestBehavior {
        async fn handle_message(&mut self, _ctx: &crate::ActorContext, _msg: Message) -> Result<(), crate::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> crate::BehaviorType {
            crate::BehaviorType::GenServer
        }
    }

    #[tokio::test]
    async fn test_register_and_create() {
        let mut registry = BehaviorRegistry::new();
        registry.register_simple("test::Worker", || TestBehavior {
            id: "worker-1".to_string(),
        }).await;

        let behavior = registry.create("test::Worker", &[]).await.unwrap();
        assert!(registry.is_registered("test::Worker").await);
        assert!(!registry.is_registered("test::Unknown").await);
    }

    #[tokio::test]
    async fn test_unknown_module() {
        let registry = BehaviorRegistry::new();
        let result = registry.create("test::Unknown", &[]).await;
        assert!(matches!(result, Err(BehaviorFactoryError::UnknownModule(_))));
    }

    #[tokio::test]
    async fn test_registered_modules() {
        let mut registry = BehaviorRegistry::new();
        registry.register_simple("test::Worker1", || TestBehavior {
            id: "1".to_string(),
        }).await;
        registry.register_simple("test::Worker2", || TestBehavior {
            id: "2".to_string(),
        }).await;

        let modules = registry.registered_modules().await;
        assert_eq!(modules.len(), 2);
        assert!(modules.contains(&"test::Worker1".to_string()));
        assert!(modules.contains(&"test::Worker2".to_string()));
    }
}
