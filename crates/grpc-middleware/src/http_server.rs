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
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tonic::body::BoxBody;
use tonic::server::NamedService;
use tonic::service::Routes;
use tower::Service;
use tower_http::cors::{Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;

/// mTLS configuration for the gRPC/HTTP server.
#[derive(Clone)]
pub struct MtlsServerConfig {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub ca_pem: Vec<u8>,
}

impl MtlsServerConfig {
    /// Load mTLS config from PEM files.
    pub fn from_files(
        cert_path: &str,
        key_path: &str,
        ca_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            cert_pem: std::fs::read(cert_path)
                .map_err(|e| format!("Cannot read cert {}: {}", cert_path, e))?,
            key_pem: std::fs::read(key_path)
                .map_err(|e| format!("Cannot read key {}: {}", key_path, e))?,
            ca_pem: std::fs::read(ca_path)
                .map_err(|e| format!("Cannot read CA {}: {}", ca_path, e))?,
        })
    }

    /// Build a rustls `ServerConfig` from the loaded PEM data.
    pub fn build_rustls_config(
        &self,
    ) -> Result<Arc<rustls::ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};
        use rustls_pemfile::{certs, private_key};
        use std::io::Cursor;

        let server_certs: Vec<CertificateDer<'static>> =
            certs(&mut Cursor::new(&self.cert_pem))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("Invalid server cert PEM: {}", e))?;

        let key: PrivateKeyDer<'static> = private_key(&mut Cursor::new(&self.key_pem))
            .map_err(|e| format!("Invalid server key PEM: {}", e))?
            .ok_or("No private key found in key PEM")?;

        // Build root cert store for client cert verification (mutual TLS)
        let mut root_store = rustls::RootCertStore::empty();
        for ca_cert in certs(&mut Cursor::new(&self.ca_pem)).flatten() {
            root_store.add(ca_cert).map_err(|e| format!("Invalid CA cert: {}", e))?;
        }
        let client_verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(root_store))
            .build()
            .map_err(|e| format!("Client verifier: {}", e))?;

        let config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(server_certs, key)
            .map_err(|e| format!("TLS config error: {}", e))?;

        Ok(Arc::new(config))
    }
}

/// Public return type for the TLS server (listener + router + optional TLS acceptor).
pub struct BuiltServer {
    pub listener: TcpListener,
    pub app: Router,
    pub tls_config: Option<Arc<rustls::ServerConfig>>,
}

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
        let grpc_router: Router = match self.routes {
            Some(routes) => routes.into_axum_router(),
            None => Router::new(),
        };

        // CORS applied to HTTP routes only (not gRPC which uses HTTP/2 and is
        // exempt from CORS). Permissive for dashboard dev; production should
        // override via reverse proxy (nginx/envoy) with strict allow-origin.
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        // Timeout on HTTP REST routes (not gRPC streaming). 5 minutes allows WASM file uploads.
        let http_with_timeout = self.http_routes.layer(TimeoutLayer::new(Duration::from_secs(300)));

        let app = grpc_router
            .merge(http_with_timeout)
            .layer(cors);

        let listener = TcpListener::bind(self.addr)
            .await
            .map_err(|e| ServerBuildError::Bind {
                addr: self.addr,
                source: e,
            })?;

        Ok((listener, app))
    }
}
