// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Single-port gRPC + HTTP server assembly.
//!
//! # Purpose
//! `GrpcHttpServerBuilder` merges tonic gRPC services and thin HTTP REST bridge
//! routes onto a single TCP port. Content-type routing (`application/grpc*` vs
//! everything else) is handled automatically by tonic's axum integration.
//!
//! # Architecture
//! ```text
//! Single TCP port
//!   ├─ application/grpc*     → tonic gRPC services (binary proto, HTTP/2)
//!   ├─ application/grpc-web* → services wrapped with tonic_web::enable()
//!   └─ everything else       → axum HTTP REST bridge routes
//! ```
//!
//! # Usage
//! ```rust,ignore
//! use plexspaces_grpc_middleware::GrpcHttpServerBuilder;
//!
//! // Wrap individual services with tonic_web::enable() for gRPC-Web support.
//! let (listener, app) = GrpcHttpServerBuilder::new(addr)
//!     .grpc_service(tonic_web::enable(ActorServiceServer::new(actor_svc)))
//!     .grpc_service(tonic_web::enable(NodeServiceServer::new(node_svc)))
//!     .http_routes(http_routes)
//!     .build()
//!     .await?;
//!
//! axum::serve(listener, app).await?;
//! ```

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tonic::body::BoxBody;
use tonic::server::NamedService;
use tonic::service::Routes;
use tower::Service;

/// Errors that can occur during server assembly or binding.
#[derive(thiserror::Error, Debug)]
pub enum ServerBuildError {
    /// Failed to bind the TCP listener to the configured address.
    #[error("failed to bind {addr}: {source}")]
    Bind {
        /// The address that could not be bound.
        addr: SocketAddr,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Builder for a single-port server that combines tonic gRPC services with
/// axum HTTP REST bridge routes.
///
/// Services may optionally be wrapped with `tonic_web::enable()` before
/// passing to `grpc_service()` to support gRPC-Web browser clients.
pub struct GrpcHttpServerBuilder {
    addr: SocketAddr,
    routes: Option<Routes>,
    http_routes: Router,
}

impl GrpcHttpServerBuilder {
    /// Create a new builder for the given bind address.
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            routes: None,
            http_routes: Router::new(),
        }
    }

    /// Add a tonic gRPC service.
    ///
    /// Wrap with `tonic_web::enable(svc)` before calling this to support gRPC-Web.
    pub fn grpc_service<S>(mut self, svc: S) -> Self
    where
        S: Service<
                tonic::codegen::http::Request<BoxBody>,
                Response = tonic::codegen::http::Response<BoxBody>,
                Error = Infallible,
            > + NamedService
            + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
    {
        self.routes = Some(match self.routes.take() {
            None => Routes::new(svc),
            Some(r) => r.add_service(svc),
        });
        self
    }

    /// Merge additional axum HTTP routes (thin REST bridge routes).
    pub fn http_routes(mut self, router: Router) -> Self {
        self.http_routes = self.http_routes.merge(router);
        self
    }

    /// Build: convert gRPC routes into axum, merge HTTP bridge routes, bind listener.
    ///
    /// Returns `(TcpListener, Router<()>)`. Caller drives with
    /// `axum::serve(listener, app).await`.
    pub async fn build(self) -> Result<(TcpListener, Router), ServerBuildError> {
        // tonic 0.12: Routes::into_axum_router() returns axum::Router.
        // gRPC and HTTP are multiplexed by content-type automatically.
        let grpc_router: Router = match self.routes {
            Some(routes) => routes.into_axum_router(),
            None => Router::new(),
        };

        let app = grpc_router.merge(self.http_routes);

        let listener = TcpListener::bind(self.addr)
            .await
            .map_err(|e| ServerBuildError::Bind {
                addr: self.addr,
                source: e,
            })?;

        Ok((listener, app))
    }
}
