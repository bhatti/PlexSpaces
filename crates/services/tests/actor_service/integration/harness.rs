// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Test Harness for Multi-Process Integration Testing
//!
//! Manages lifecycle of multiple ActorService nodes running in separate processes.

use plexspaces_proto::ActorServiceClient;
use std::process::{Child, Command};
use std::time::Duration;
use tonic::transport::Channel;

/// Test harness that manages multiple node processes
pub struct TestHarness {
    nodes: Vec<NodeProcess>,
    base_port: u16,
}

/// A single node process with its gRPC client
pub struct NodeProcess {
    pub node_id: String,
    pub port: u16,
    pub process: Child,
    pub client: ActorServiceClient<Channel>,
}

impl TestHarness {
    /// Create a new test harness
    pub fn new() -> Self {
        TestHarness {
            nodes: vec![],
            base_port: 19001, // Use high ports to avoid conflicts
        }
    }

    /// Spawn a new node process
    ///
    /// # Arguments
    /// * `node_id` - Unique identifier for this node
    ///
    /// # Returns
    /// Reference to the spawned node process
    pub async fn spawn_node(
        &mut self,
        node_id: &str,
    ) -> Result<&NodeProcess, Box<dyn std::error::Error>> {
        let port = self.base_port + self.nodes.len() as u16;

        println!("Spawning node {} on port {}...", node_id, port);

        // Find node_runner binary (tests run from workspace root)
        let binary_path = if std::path::Path::new("target/debug/node_runner").exists() {
            "target/debug/node_runner"
        } else if std::path::Path::new("../../target/debug/node_runner").exists() {
            "../../target/debug/node_runner"
        } else {
            return Err(
                "node_runner binary not found. Run 'cargo build --bin node_runner' first".into(),
            );
        };

        println!("Using node_runner at: {}", binary_path);

        // Spawn node_runner binary
        let process = Command::new(binary_path)
            .arg(node_id)
            .arg(port.to_string())
            .spawn()
            .map_err(|e| format!("Failed to spawn node_runner: {}", e))?;

        // Wait for gRPC server to be ready
        println!("Waiting for node {} to start...", node_id);
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // Create gRPC client
        let addr = format!("http://127.0.0.1:{}", port);
        let client = ActorServiceClient::connect(addr.clone())
            .await
            .map_err(|e| format!("Failed to connect to node {} at {}: {}", node_id, addr, e))?;

        println!("Node {} ready at {}", node_id, addr);

        let node = NodeProcess {
            node_id: node_id.to_string(),
            port,
            process,
            client,
        };

        self.nodes.push(node);
        Ok(self.nodes.last().unwrap())
    }

    /// Get a mutable reference to a node by ID
    pub fn get_node(&mut self, node_id: &str) -> Option<&mut NodeProcess> {
        self.nodes.iter_mut().find(|n| n.node_id == node_id)
    }

    /// Get all nodes
    pub fn nodes(&mut self) -> &mut [NodeProcess] {
        &mut self.nodes
    }

    /// Shutdown all nodes gracefully
    pub async fn shutdown(&mut self) {
        println!("Shutting down {} nodes...", self.nodes.len());
        for node in &mut self.nodes {
            println!(
                "Killing node {} (pid: {:?})...",
                node.node_id,
                node.process.id()
            );
            let _ = node.process.kill();
            let _ = node.process.wait(); // Clean up zombie process
        }
        self.nodes.clear();
        println!("All nodes shut down");
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        // Ensure all processes are killed even if test panics
        for node in &mut self.nodes {
            let _ = node.process.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Run manually: cargo test --test integration -- --ignored
    async fn test_harness_spawn_and_shutdown() {
        let mut harness = TestHarness::new();

        // Spawn two nodes
        let node1 = harness.spawn_node("test_node1").await;
        assert!(node1.is_ok());

        let node2 = harness.spawn_node("test_node2").await;
        assert!(node2.is_ok());

        // Verify nodes are running
        assert_eq!(harness.nodes().len(), 2);
        assert_eq!(harness.nodes()[0].node_id, "test_node1");
        assert_eq!(harness.nodes()[1].node_id, "test_node2");

        // Shutdown
        harness.shutdown().await;
        assert_eq!(harness.nodes().len(), 0);
    }
}
