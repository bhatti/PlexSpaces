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

//! Capability Provider trait for pluggable service extensions
//!
//! ## Purpose
//! Defines the interface for capability providers that extend PlexSpaces
//! with new service types. Inspired by wasmCloud's capability provider model.
//!
//! ## Design
//! - Providers register with ServiceLocator using a unique provider_id
//! - Each provider can optionally fulfill a WIT interface for WASM actors
//! - Providers have lifecycle hooks (start/stop) for ordered initialization
//! - Providers handle invocations from actors via handle_invocation()
//!
//! ## Example
//! ```rust,ignore
//! struct HttpClientProvider { ... }
//!
//! impl CapabilityProvider for HttpClientProvider {
//!     fn provider_id(&self) -> &str { "plexspaces:http-client" }
//!     fn description(&self) -> &str { "HTTP client for outbound requests" }
//!     async fn start(&self, config: &ProviderConfig) -> Result<(), ProviderError> { ... }
//!     async fn stop(&self) -> Result<(), ProviderError> { ... }
//!     async fn health_check(&self) -> Result<ProviderHealth, ProviderError> { ... }
//!     async fn handle_invocation(&self, ctx: &RequestContext, op: &str, payload: &[u8])
//!         -> Result<Vec<u8>, ProviderError> { ... }
//! }
//! ```

use async_trait::async_trait;
use std::collections::HashMap;

use crate::RequestContext;

/// A capability provider that can be dynamically registered with the ServiceLocator.
///
/// Capability providers are plugins that extend PlexSpaces with new service types.
/// They follow the wasmCloud pattern: host-side implementations that fulfill
/// WIT interface contracts for WASM actors.
///
/// ## Lifecycle
/// 1. Provider is created with configuration
/// 2. Provider is registered with ServiceLocator via register_capability_provider()
/// 3. Provider.start() is called during node startup
/// 4. Provider handles requests from actors via its WIT interface
/// 5. Provider.stop() is called during node shutdown
#[async_trait]
pub trait CapabilityProvider: Send + Sync {
    /// Unique provider identifier (e.g., "plexspaces:http-client", "plexspaces:redis")
    fn provider_id(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// WIT interface this provider fulfills (if any)
    /// e.g., "plexspaces:actor/http-client@0.1.0"
    fn wit_interface(&self) -> Option<&str> {
        None
    }

    /// Start the provider (called after registration)
    async fn start(&self, config: &ProviderConfig) -> Result<(), ProviderError>;

    /// Stop the provider (called during shutdown)
    async fn stop(&self) -> Result<(), ProviderError>;

    /// Health check
    async fn health_check(&self) -> Result<ProviderHealth, ProviderError>;

    /// Handle a capability invocation from a WASM actor.
    ///
    /// This is the core dispatch method - routes WIT function calls to provider logic.
    ///
    /// ## Arguments
    /// * `ctx` - Request context with tenant/namespace isolation
    /// * `operation` - The operation name (e.g., "request", "get", "post")
    /// * `payload` - Serialized request payload
    ///
    /// ## Returns
    /// Serialized response payload
    async fn handle_invocation(
        &self,
        ctx: &RequestContext,
        operation: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, ProviderError>;
}

/// Configuration for a capability provider
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// Provider-specific configuration (key-value pairs)
    pub config: HashMap<String, String>,
    /// Link configuration (which actors are linked to this provider)
    pub links: Vec<ProviderLink>,
}

/// A link between an actor and a capability provider
#[derive(Debug, Clone)]
pub struct ProviderLink {
    /// Actor ID that is linked to this provider
    pub actor_id: String,
    /// Link name (for multiple links to same provider type)
    pub link_name: String,
    /// Link-specific configuration
    pub config: HashMap<String, String>,
}

/// Provider health status
#[derive(Debug, Clone)]
pub struct ProviderHealth {
    /// Whether the provider is healthy
    pub healthy: bool,
    /// Human-readable health message
    pub message: String,
}

impl ProviderHealth {
    /// Create a healthy status
    pub fn healthy() -> Self {
        Self {
            healthy: true,
            message: "healthy".to_string(),
        }
    }

    /// Create an unhealthy status with message
    pub fn unhealthy(message: impl Into<String>) -> Self {
        Self {
            healthy: false,
            message: message.into(),
        }
    }
}

/// Errors from capability providers
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Provider has not been started
    #[error("Provider not started: {0}")]
    NotStarted(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Error during invocation handling
    #[error("Invocation error: {0}")]
    InvocationError(String),

    /// Unknown operation
    #[error("Unknown operation: {0}")]
    UnknownOperation(String),

    /// Generic provider error
    #[error("Provider error: {0}")]
    Other(String),
}
