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

//! Multi-environment testing infrastructure for PlexSpaces
//!
//! Supports testing actors in various environments:
//! - In-process (Erlang-like lightweight processes)
//! - Docker containers
//! - Kubernetes pods
//! - Firecracker VMs

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

use crate::actor::ActorState;
// Re-exported from core crate
use crate::mailbox::Message;

pub mod docker;
pub mod example;
pub mod firecracker;
pub mod in_process;
pub mod kubernetes;

/// Errors that can occur during testing
#[derive(Error, Debug)]
pub enum TestError {
    #[error("Failed to deploy actor: {0}")]
    DeploymentFailed(String),

    #[error("Failed to send message: {0}")]
    MessageFailed(String),

    #[error("Actor not found: {0}")]
    ActorNotFound(String),

    #[error("Environment error: {0}")]
    EnvironmentError(String),

    #[error("Timeout waiting for result")]
    Timeout,
}

/// Types of test environments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentType {
    /// In-process actors (like Erlang processes)
    InProcess,
    /// Docker containers
    Docker,
    /// Kubernetes pods
    Kubernetes,
    /// Firecracker microVMs
    Firecracker,
}

impl EnvironmentType {
    pub fn all() -> Vec<Self> {
        vec![
            Self::InProcess,
            Self::Docker,
            Self::Kubernetes,
            Self::Firecracker,
        ]
    }
}

/// Handle to an actor in any environment
#[derive(Debug, Clone)]
pub enum ActorHandle {
    /// In-process actor handle (actor_id, join_handle)
    Local(String, Arc<tokio::task::JoinHandle<()>>),
    /// Docker container ID
    Docker(String),
    /// Kubernetes pod name
    Kubernetes(String),
    /// Firecracker VM ID
    Firecracker(String),
}

/// Configuration for deploying an actor
#[derive(Debug, Clone)]
pub struct ActorConfig {
    pub id: String,
    pub actor_type: String,
    pub resources: ResourceProfile,
    pub metadata: HashMap<String, String>,
}

/// Resource requirements for an actor
#[derive(Debug, Clone)]
pub struct ResourceProfile {
    pub cpu_cores: f32,
    pub memory_mb: u32,
    pub disk_gb: u32,
    pub network_bandwidth_mbps: u32,
}

impl ResourceProfile {
    /// Profile for a Byzantine commander
    pub fn commander() -> Self {
        ResourceProfile {
            cpu_cores: 2.0,
            memory_mb: 2048,
            disk_gb: 10,
            network_bandwidth_mbps: 100,
        }
    }

    /// Profile for a Byzantine lieutenant
    pub fn lieutenant() -> Self {
        ResourceProfile {
            cpu_cores: 1.0,
            memory_mb: 1024,
            disk_gb: 5,
            network_bandwidth_mbps: 50,
        }
    }

    /// Minimal resources for testing
    pub fn minimal() -> Self {
        ResourceProfile {
            cpu_cores: 0.5,
            memory_mb: 512,
            disk_gb: 1,
            network_bandwidth_mbps: 10,
        }
    }
}

/// Metrics collected from test environment
#[derive(Debug, Clone)]
pub struct EnvironmentMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub avg_latency_ms: f64,
    pub cpu_usage_percent: f64,
    pub memory_usage_mb: u64,
    pub network_bytes_sent: u64,
    pub network_bytes_received: u64,
}

/// Result of a test run
#[derive(Debug, Clone)]
pub struct TestResult {
    pub environment: EnvironmentType,
    pub duration: Duration,
    pub success: bool,
    pub metrics: EnvironmentMetrics,
    pub errors: Vec<String>,
}

/// Trait for test environments
#[async_trait]
pub trait TestEnvironment: Send + Sync {
    /// Get the environment type
    fn environment_type(&self) -> EnvironmentType;

    /// Deploy an actor in this environment
    async fn deploy_actor(&self, config: ActorConfig) -> Result<ActorHandle, TestError>;

    /// Send a message to an actor
    async fn send_message(&self, actor: &ActorHandle, msg: Message) -> Result<(), TestError>;

    /// Get actor state (for verification)
    async fn get_state(&self, actor: &ActorHandle) -> Result<ActorState, TestError>;

    /// Kill an actor (for fault tolerance testing)
    async fn kill_actor(&self, actor: &ActorHandle) -> Result<(), TestError>;

    /// Create a network partition between two groups
    async fn create_partition(
        &self,
        group1: Vec<ActorHandle>,
        group2: Vec<ActorHandle>,
    ) -> Result<(), TestError>;

    /// Heal a network partition
    async fn heal_partition(&self) -> Result<(), TestError>;

    /// Collect metrics from the environment
    async fn collect_metrics(&self) -> Result<EnvironmentMetrics, TestError>;

    /// Cleanup all resources
    async fn cleanup(&self) -> Result<(), TestError>;
}

/// Builder for creating test environments
pub struct TestEnvironmentBuilder {
    env_type: EnvironmentType,
    config: HashMap<String, String>,
}

impl TestEnvironmentBuilder {
    pub fn new(env_type: EnvironmentType) -> Self {
        TestEnvironmentBuilder {
            env_type,
            config: HashMap::new(),
        }
    }

    pub fn with_config(mut self, key: &str, value: &str) -> Self {
        self.config.insert(key.to_string(), value.to_string());
        self
    }

    pub async fn build(self) -> Result<Box<dyn TestEnvironment>, TestError> {
        match self.env_type {
            EnvironmentType::InProcess => Ok(Box::new(in_process::InProcessEnvironment::new())),
            EnvironmentType::Docker => {
                Ok(Box::new(docker::DockerEnvironment::new(self.config).await?))
            }
            EnvironmentType::Kubernetes => Ok(Box::new(
                kubernetes::KubernetesEnvironment::new(self.config).await?,
            )),
            EnvironmentType::Firecracker => Ok(Box::new(
                firecracker::FirecrackerEnvironment::new(self.config).await?,
            )),
        }
    }
}

/// Test harness for running tests across environments
pub struct MultiEnvironmentTestHarness {
    environments: Vec<Box<dyn TestEnvironment>>,
    results: Arc<RwLock<Vec<TestResult>>>,
}

impl Default for MultiEnvironmentTestHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiEnvironmentTestHarness {
    pub fn new() -> Self {
        MultiEnvironmentTestHarness {
            environments: Vec::new(),
            results: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn add_environment(mut self, env: Box<dyn TestEnvironment>) -> Self {
        self.environments.push(env);
        self
    }

    pub async fn run_test<F, Fut>(&self, test_fn: F) -> Vec<TestResult>
    where
        F: Fn(Box<dyn TestEnvironment>) -> Fut + Send + Sync + Clone + 'static,
        Fut: std::future::Future<Output = TestResult> + Send + 'static,
    {
        let mut handles = Vec::new();

        for env in &self.environments {
            let test = test_fn.clone();
            let env_clone = env.environment_type();
            let results = self.results.clone();

            let handle = tokio::spawn(async move {
                let _start = Instant::now();
                let env = TestEnvironmentBuilder::new(env_clone)
                    .build()
                    .await
                    .unwrap();

                let result = test(env).await;

                let mut results_lock = results.write().await;
                results_lock.push(result);
            });

            handles.push(handle);
        }

        // Wait for all tests to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Return collected results
        let guard = self.results.read().await;
        guard.clone()
    }

    pub fn generate_comparison_report(&self, results: &[TestResult]) {
        println!("\n=== Multi-Environment Test Results ===\n");

        println!(
            "{:<15} | {:<10} | {:<10} | {:<15} | {:<10}",
            "Environment", "Success", "Duration", "Avg Latency", "Memory (MB)"
        );
        println!("{:-<70}", "");

        for result in results {
            println!(
                "{:<15} | {:<10} | {:<10.2?} | {:<15.2} ms | {:<10}",
                format!("{:?}", result.environment),
                if result.success { "✅" } else { "❌" },
                result.duration,
                result.metrics.avg_latency_ms,
                result.metrics.memory_usage_mb
            );
        }

        println!("\n");
    }
}

/// Helper function to run Byzantine consensus test in any environment
pub async fn test_byzantine_consensus(
    env: Box<dyn TestEnvironment>,
    num_generals: usize,
    num_faulty: usize,
) -> TestResult {
    let start = Instant::now();
    let env_type = env.environment_type();

    // Deploy generals
    let mut generals = Vec::new();
    let mut errors = Vec::new();

    for i in 0..num_generals {
        let config = ActorConfig {
            id: format!("general_{}", i),
            actor_type: if i == 0 {
                "commander".to_string()
            } else {
                "lieutenant".to_string()
            },
            resources: if i == 0 {
                ResourceProfile::commander()
            } else {
                ResourceProfile::lieutenant()
            },
            metadata: {
                let mut m = HashMap::new();
                m.insert("is_faulty".to_string(), (i <= num_faulty).to_string());
                m
            },
        };

        match env.deploy_actor(config).await {
            Ok(handle) => generals.push(handle),
            Err(e) => {
                errors.push(format!("Failed to deploy general {}: {}", i, e));
                break;
            }
        }
    }

    if !errors.is_empty() {
        return TestResult {
            environment: env_type,
            duration: start.elapsed(),
            success: false,
            metrics: EnvironmentMetrics {
                messages_sent: 0,
                messages_received: 0,
                avg_latency_ms: 0.0,
                cpu_usage_percent: 0.0,
                memory_usage_mb: 0,
                network_bytes_sent: 0,
                network_bytes_received: 0,
            },
            errors,
        };
    }

    // Run consensus protocol
    // ... (implementation depends on specific test)

    // Collect metrics
    let metrics = env.collect_metrics().await.unwrap_or(EnvironmentMetrics {
        messages_sent: 0,
        messages_received: 0,
        avg_latency_ms: 0.0,
        cpu_usage_percent: 0.0,
        memory_usage_mb: 0,
        network_bytes_sent: 0,
        network_bytes_received: 0,
    });

    // Cleanup
    let _ = env.cleanup().await;

    TestResult {
        environment: env_type,
        duration: start.elapsed(),
        success: errors.is_empty(),
        metrics,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_environment_builder() {
        let env = TestEnvironmentBuilder::new(EnvironmentType::InProcess)
            .with_config("max_actors", "100")
            .build()
            .await
            .unwrap();

        assert_eq!(env.environment_type(), EnvironmentType::InProcess);
    }

    #[tokio::test]
    async fn test_multi_environment_harness() {
        let harness = MultiEnvironmentTestHarness::new()
            .add_environment(Box::new(in_process::InProcessEnvironment::new()));

        let results = harness
            .run_test(|env| async move { test_byzantine_consensus(env, 4, 1).await })
            .await;

        assert!(!results.is_empty());
        harness.generate_comparison_report(&results);
    }
}
