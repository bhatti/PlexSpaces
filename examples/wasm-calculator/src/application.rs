// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Application trait implementation for WASM calculator

use plexspaces_core::application::{Application, ApplicationNode, ApplicationError, HealthStatus};
use async_trait::async_trait;

/// WASM Calculator Application
pub struct WasmCalculatorApp {
    name: String,
    version: String,
}

impl WasmCalculatorApp {
    /// Create new application
    pub fn new() -> Self {
        Self {
            name: "wasm-calculator".to_string(),
            version: "0.1.0".to_string(),
        }
    }
}

impl Default for WasmCalculatorApp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Application for WasmCalculatorApp {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn start(&mut self, _node: std::sync::Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        tracing::info!("Starting WASM Calculator application v{}", self.version);
        // TODO: Spawn calculator actors, load WASM modules
        // TODO: Create supervision tree for calculator workers
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        tracing::info!("Stopping WASM Calculator application");
        // TODO: Graceful shutdown: stop accepting work, drain queues
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // TODO: Implement actual health checks
        HealthStatus::HealthStatusHealthy
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
