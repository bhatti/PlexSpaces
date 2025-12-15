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

//! HTTP router for blob service endpoints
//!
//! Provides a way to run blob HTTP endpoints alongside the gRPC server.
//! Uses Axum router for blob HTTP endpoints and integrates with tonic server.

use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Start blob HTTP server on a separate task if blob service is available
/// Returns the join handle for the HTTP server task
pub async fn start_blob_http_server(
    blob_service: Option<Arc<plexspaces_blob::BlobService>>,
    http_addr: std::net::SocketAddr,
) -> Result<Option<JoinHandle<()>>, Box<dyn std::error::Error + Send + Sync>> {
    let Some(service) = blob_service else {
        return Ok(None);
    };

    // Create Axum router for blob endpoints
    use plexspaces_blob::server::http_axum::create_blob_router;
    let router = create_blob_router(service);

    // Start Axum server on separate task
    let listener = TcpListener::bind(http_addr).await?;
    let handle = tokio::spawn(async move {
        use axum::serve;
        if let Err(e) = serve(listener, router).await {
            eprintln!("Blob HTTP server error: {}", e);
        }
    });

    Ok(Some(handle))
}
