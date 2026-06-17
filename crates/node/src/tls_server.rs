// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! mTLS accept loop for the single-port gRPC+HTTP server.
//!
//! ## Purpose
//! Wraps an axum `Router` with a `tokio-rustls` `TlsAcceptor` so inbound connections
//! go through a mutual-TLS handshake before being handed to axum.  The result is
//! functionally equivalent to `axum::serve(listener, app)` but each connection is
//! authenticated by a client certificate signed by the configured CA.
//!
//! ## Design
//! - Uses `hyper_util` (already a transitive dep via axum 0.7 / hyper 1.0) to drive
//!   individual HTTP/2-or-1.1 connections after the TLS layer upgrades them.
//! - Errors on individual connections are logged at DEBUG and do not abort the loop.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::connect_info::IntoMakeServiceWithConnectInfo;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;

/// Run an mTLS-wrapped axum server until the listener is dropped or an I/O error occurs.
///
/// Each accepted TCP connection is upgraded to TLS (client cert required), then
/// driven as an HTTP/2-or-1.1 connection by `hyper_util`.
pub async fn serve_tls(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    mut make_service: IntoMakeServiceWithConnectInfo<axum::Router, SocketAddr>,
) -> io::Result<()> {
    loop {
        let (tcp_stream, remote_addr) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                // Non-fatal I/O errors (e.g., "connection reset by peer") — keep looping.
                tracing::debug!(error = %e, "mTLS: TCP accept error");
                continue;
            }
        };

        let acceptor = acceptor.clone();
        let tower_svc = match make_service.call(remote_addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(remote_addr = %remote_addr, error = ?e, "mTLS: make_service error");
                continue;
            }
        };

        tokio::spawn(async move {
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::debug!(remote_addr = %remote_addr, error = %e, "mTLS: TLS handshake failed");
                    return;
                }
            };

            let io = TokioIo::new(tls_stream);
            let hyper_svc = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                let mut svc = tower_svc.clone();
                async move { svc.call(req).await }
            });

            if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, hyper_svc)
                .await
            {
                tracing::debug!(remote_addr = %remote_addr, error = %e, "mTLS: connection error");
            }
        });
    }
}
