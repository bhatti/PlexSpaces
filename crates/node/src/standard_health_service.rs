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

//! # Standard gRPC Health Service Implementation
//!
//! ## Purpose
//! Implements the standard `grpc.health.v1.Health` service that integrates with
//! PlexSpacesHealthReporter to provide service-specific health status.
//!
//! ## Architecture Context
//! This service implements the standard gRPC Health Checking Protocol:
//! - https://grpc.io/docs/guides/health-checking/
//! - Supports service-specific health checks (e.g., "plexspaces.actor.v1.ActorService")
//! - Integrates with PlexSpacesHealthReporter for status tracking
//! - Rejects requests during shutdown

use crate::health_service::PlexSpacesHealthReporter;
use plexspaces_proto::system::v1::ServingStatus;
use std::sync::Arc;
use tonic::{Request, Response, Status};
// Import health service types from tonic-health
// Note: tonic-health v0.10 uses pb module for proto types
// Check dependency_registration.rs for correct import pattern
use tonic_health::pb::health_server::Health;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::HealthCheckResponse;
use tonic_health::pb::health_check_response::ServingStatus as HealthServingStatus;

/// Standard gRPC Health Service implementation
pub struct StandardHealthServiceImpl {
    /// Health reporter for status tracking
    health_reporter: Arc<PlexSpacesHealthReporter>,
}

impl StandardHealthServiceImpl {
    /// Create new standard health service
    pub fn new(health_reporter: Arc<PlexSpacesHealthReporter>) -> Self {
        Self { health_reporter }
    }
    
    /// Convert PlexSpaces ServingStatus to tonic-health ServingStatus
    fn convert_status(status: ServingStatus) -> HealthServingStatus {
        match status {
            ServingStatus::ServingStatusServing => HealthServingStatus::Serving,
            ServingStatus::ServingStatusNotServing => HealthServingStatus::NotServing,
            ServingStatus::ServingStatusUnknown => HealthServingStatus::Unknown,
            _ => HealthServingStatus::Unknown,
        }
    }
}

#[tonic::async_trait]
impl Health for StandardHealthServiceImpl {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let req = request.into_inner();
        let service_name = if req.service.is_empty() {
            String::new()
        } else {
            req.service
        };
        
        // Check if shutdown is in progress - health checks should still work during shutdown
        // but return NOT_SERVING status
        
        // Get service-specific health status
        let status = if service_name.is_empty() {
            // Overall health - check via public methods
            let (is_ready, _) = self.health_reporter.check_readiness().await;
            let is_alive = self.health_reporter.is_alive().await;
            
            if !is_alive || !is_ready {
                ServingStatus::ServingStatusNotServing
            } else {
                ServingStatus::ServingStatusServing
            }
        } else {
            // Service-specific health
            self.health_reporter.get_service_status(&service_name).await
        };
        
        let health_status = Self::convert_status(status);
        
        Ok(Response::new(HealthCheckResponse {
            status: health_status as i32,
        }))
    }

    type WatchStream = tokio_stream::wrappers::ReceiverStream<
        Result<HealthCheckResponse, Status>,
    >;

    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let req = request.into_inner();
        let service_name = if req.service.is_empty() {
            String::new()
        } else {
            req.service
        };
        
        // Create a channel for streaming health status updates
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        
        // Spawn a task to periodically check and send health status
        let health_reporter = self.health_reporter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // Get current health status
                let status = if service_name.is_empty() {
                    // Overall health - check via public methods
                    let (is_ready, _) = health_reporter.check_readiness().await;
                    let is_alive = health_reporter.is_alive().await;
                    
                    if !is_alive || !is_ready {
                        ServingStatus::ServingStatusNotServing
                    } else {
                        ServingStatus::ServingStatusServing
                    }
                } else {
                    health_reporter.get_service_status(&service_name).await
                };
                
                let health_status = Self::convert_status(status);
                
                // Send status update
                if tx
                    .send(Ok(HealthCheckResponse {
                        status: health_status as i32,
                    }))
                    .await
                    .is_err()
                {
                    // Receiver dropped, stop sending
                    break;
                }
            }
        });
        
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}
