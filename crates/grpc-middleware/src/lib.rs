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

//! # PlexSpaces gRPC Middleware
//!
//! ## Purpose
//! Production-ready middleware (interceptors) for gRPC services with observability,
//! security, and reliability features.
//!
//! ## Architecture Context
//! This crate implements **Phase 3: gRPC Middleware** of PlexSpaces. It provides:
//! - **Observability**: Metrics (Prometheus), tracing (OpenTelemetry), logging
//! - **Security**: mTLS, authentication, authorization (RBAC)
//! - **Reliability**: Rate limiting, circuit breaking, retries, compression
//!
//! ## Integration with PlexSpaces
//! - **PlexSpacesNode**: Loads middleware chain from `release.toml` configuration
//! - **gRPC Services**: ActorService, TupleSpaceService, SupervisionService all use middleware
//! - **Configuration**: Proto-first design with type-safe config
//!
//! ## Design Principles
//! 1. **Composable**: Chain multiple interceptors in any order
//! 2. **Configurable**: Enable/disable via config (metrics always on)
//! 3. **Proto-First**: All configuration defined in proto
//! 4. **Zero-Cost When Disabled**: Feature flags prevent overhead
//! 5. **Observable**: All interceptors emit metrics
//!
//! ## Examples
//!
//! ### Basic Usage with Metrics (Always Enabled)
//! ```rust
//! use plexspaces_grpc_middleware::{InterceptorChain, MetricsInterceptor};
//! use plexspaces_proto::grpc::v1::{MiddlewareConfig, MiddlewareSpec};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create interceptor chain with metrics (always enabled)
//! let chain = InterceptorChain::from_config(&MiddlewareConfig::default())?;
//!
//! // Apply to gRPC service
//! // Server::builder()
//! //     .layer(chain)
//! //     .add_service(ActorService::new(...))
//! //     .serve(addr).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Full Middleware Stack
//! ```rust
//! use plexspaces_grpc_middleware::chain::InterceptorChain;
//! use plexspaces_proto::grpc::v1::{MiddlewareConfig, MiddlewareSpec, MiddlewareType, AuthMethod, AuthMiddlewareConfig};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Load from release.toml configuration
//! let config = MiddlewareConfig {
//!     middleware: vec![
//!         MiddlewareSpec {
//!             middleware_type: MiddlewareType::MiddlewareTypeMetrics as i32,
//!             enabled: true,  // Always enabled
//!             priority: 10,   // First to see requests
//!             config: None,
//!         },
//!         MiddlewareSpec {
//!             middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
//!             enabled: true,  // Enabled via config
//!             priority: 30,
//!             config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(
//!                 AuthMiddlewareConfig {
//!                     method: AuthMethod::AuthMethodJwt as i32,
//!                     jwt_key: "my-secret".to_string(),
//!                     rbac: None,
//!                     allow_unauthenticated: true,
//!                     mtls_ca_certificate: String::new(),
//!                     mtls_trusted_services: vec![],
//!                 }
//!             )),
//!         },
//!     ],
//! };
//!
//! let chain = InterceptorChain::from_config(&config)?;
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod auth;
pub mod cert_gen;
pub mod chain;
pub mod compression;
pub mod metrics;
pub mod observability;
pub mod rate_limit;
pub mod retry;
pub mod security_validation;
pub mod tracing_interceptor;

pub use auth::AuthInterceptor;
pub use cert_gen::{CertificateGenerator, CertificatePaths, CertGenError};
pub use chain::{Interceptor, InterceptorChain, InterceptorError};
pub use compression::CompressionInterceptor;
pub use metrics::MetricsInterceptor;
pub use observability::{ObservabilityManager, ObservabilityError, LogLevel, metrics_handler};
pub use rate_limit::RateLimitInterceptor;
pub use retry::RetryInterceptor;
pub use security_validation::{SecurityValidator, SecurityValidationError};
pub use tracing_interceptor::TracingInterceptor;

/// Result type for interceptor operations
pub type Result<T> = std::result::Result<T, InterceptorError>;
