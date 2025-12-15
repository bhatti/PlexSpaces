// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Genomic Pipeline Application
//!
//! Implements the `Application` trait for node-based deployment.
//! Uses NodeBuilder/ActorBuilder patterns for simplified setup.

use async_trait::async_trait;
use plexspaces_core::application::{
    Application, ApplicationError, ApplicationNode, HealthStatus,
};
use crate::actors::{QcActorBehavior, AlignmentActorBehavior, VariantCallingActorBehavior};
use std::sync::Arc;
use tracing::info;
use plexspaces_mailbox::mailbox_config_default;

/// The Genomic Pipeline Application
///
/// This application can be deployed to a PlexSpaces node for workflow orchestration.
/// Actors are spawned using NodeBuilder/ActorBuilder patterns.
#[derive(Debug)]
pub struct GenomicsPipelineApplication {
    name: String,
    version: String,
}

impl GenomicsPipelineApplication {
    /// Creates a new instance of the Genomic Pipeline Application
    pub fn new() -> Self {
        Self {
            name: "genomic-pipeline".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

#[async_trait]
impl Application for GenomicsPipelineApplication {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        info!("GenomicsPipelineApplication '{}' (v{}) starting...", self.name(), self.version());

        // Note: In a production implementation, actors would be spawned here using NodeBuilder/ActorBuilder
        // For this simplified example, the standalone runner (main.rs) handles workflow execution directly
        // This Application implementation is kept for node-based deployment scenarios

        info!("GenomicsPipelineApplication '{}' started successfully.", self.name());
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        info!("GenomicsPipelineApplication '{}' stopping...", self.name());
        info!("GenomicsPipelineApplication '{}' stopped.", self.name());
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Note: In production, this would check actor health via ObjectRegistry
        HealthStatus::HealthStatusHealthy
    }
}

impl Default for GenomicsPipelineApplication {
    fn default() -> Self {
        Self::new()
    }
}
