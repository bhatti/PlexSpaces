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

/// Default TCP accept-queue depth.
///
/// macOS caps `listen(2)` backlog at `kern.ipc.somaxconn` (128 by default).
/// Override at runtime with `PLEXSPACES_TCP_BACKLOG=<n>` to match the OS limit
/// (`sudo sysctl -w kern.ipc.somaxconn=4096` on macOS,
///  already 4096+ on Linux 5.4+).
///
/// The backlog only covers the *accept queue* — the rate-limiting bottleneck is
/// the work done per connection once accepted (SWIM, WsRegistry, etc.).
/// A backlog of 1024 is more than enough for any realistic burst; larger values
/// waste kernel memory and mask underlying throughput problems.
const DEFAULT_TCP_BACKLOG: u32 = 1024;

/// mTLS configuration for the gRPC/HTTP server.
#[derive(Clone)]
pub struct MtlsServerConfig {
    /// Server certificate in PEM format.
    pub cert_pem: Vec<u8>,
    /// Server private key in PEM format.
    pub key_pem: Vec<u8>,
    /// CA certificate in PEM format (for client verification).
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

        let server_certs: Vec<CertificateDer<'static>> = certs(&mut Cursor::new(&self.cert_pem))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Invalid server cert PEM: {}", e))?;

        let key: PrivateKeyDer<'static> = private_key(&mut Cursor::new(&self.key_pem))
            .map_err(|e| format!("Invalid server key PEM: {}", e))?
            .ok_or("No private key found in key PEM")?;

        // Build root cert store for client cert verification (mutual TLS)
        let mut root_store = rustls::RootCertStore::empty();
        for ca_cert in certs(&mut Cursor::new(&self.ca_pem)).flatten() {
            root_store
                .add(ca_cert)
                .map_err(|e| format!("Invalid CA cert: {}", e))?;
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

/// Create a `TcpListener` with `SO_REUSEADDR` and a custom accept-queue depth.
///
/// Using `socket2` gives us control over the `listen(2)` backlog, which Tokio's
/// `TcpListener::bind` exposes only as the OS default (`SOMAXCONN`).  On macOS
/// that default is 128; under a 50-VU burst all slots fill instantly and new
/// SYNs are dropped, causing exponential-backoff retries (1 s, 3 s …) visible
/// as 4 s+ upgrade latency in k6.
///
/// The kernel silently clamps `backlog` to `somaxconn`, so passing 1024 on an
/// untuned macOS box behaves identically to the current code — the improvement
/// is only realised once the operator raises `kern.ipc.somaxconn` or on Linux
/// where the kernel default is already 4096.
fn bind_tcp_listener(addr: SocketAddr, backlog: u32) -> std::io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};

    let domain = if addr.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    // SO_REUSEPORT allows multiple sockets on the same port for load balancing
    // across Tokio worker threads on Linux (no-op on macOS where the kernel
    // still serialises accept, but harmless).
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(backlog as i32)?;

    // Convert to a Tokio TcpListener without rebinding.
    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

/// Public return type for the TLS server (listener + router + optional TLS acceptor).
pub struct BuiltServer {
    /// Bound TCP listener ready to accept connections.
    pub listener: TcpListener,
    /// Axum router combining gRPC and HTTP routes.
    pub app: Router,
    /// Optional TLS configuration for HTTPS/gRPC-TLS.
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
    ///
    /// The TCP listener is created via `socket2` so we can set:
    /// - `SO_REUSEADDR`: allows fast server restart without TIME_WAIT delays
    /// - Custom backlog: defaults to `DEFAULT_TCP_BACKLOG` (1024), overridable
    ///   via `PLEXSPACES_TCP_BACKLOG` env var (capped at the OS `somaxconn`
    ///   ceiling automatically by the kernel)
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
        let http_with_timeout = self
            .http_routes
            .layer(TimeoutLayer::new(Duration::from_secs(300)));

        let app = grpc_router.merge(http_with_timeout).layer(cors);

        let backlog = std::env::var("PLEXSPACES_TCP_BACKLOG")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DEFAULT_TCP_BACKLOG);

        let listener = bind_tcp_listener(self.addr, backlog).map_err(|e| ServerBuildError::Bind {
            addr: self.addr,
            source: e,
        })?;

        Ok((listener, app))
    }
}
