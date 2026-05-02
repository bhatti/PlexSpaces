// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Test Harness for Multi-Process Integration Testing
//!
//! Manages lifecycle of multiple ActorService nodes running in separate processes.

use plexspaces_proto::ActorServiceClient;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;
use tonic::transport::Channel;

/// Locate node_runner binary (next to test binary in target/debug or target/debug/deps).
fn find_node_runner_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Current exe is e.g. target/debug/deps/integration_tests-<hash> or target/debug/integration_tests-<hash>
    let current_exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    let target_debug = current_exe
        .parent()
        .and_then(|p| {
            if p.file_name().and_then(|n| n.to_str()) == Some("deps") {
                p.parent()
            } else {
                Some(p)
            }
        })
        .ok_or("cannot get target/debug from current exe")?;
    let exe_name = format!("node_runner{}", std::env::consts::EXE_SUFFIX);
    let candidate = target_debug.join(&exe_name);
    if candidate.exists() {
        return Ok(candidate);
    }
    // Fallback: workspace root relative (e.g. cwd when running from repo root)
    for rel in ["target/debug", "../../target/debug"] {
        let p = std::path::Path::new(rel).join(&exe_name);
        if p.exists() {
            return Ok(p.canonicalize().unwrap_or_else(|_| p));
        }
    }
    Err("node_runner binary not found. Run 'cargo build --bin node_runner -p plexspaces-services' first (or 'make build' with binaries).".into())
}

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

        // Find node_runner binary: same target/debug as this test binary (workspace or package target)
        let binary_path = find_node_runner_binary()?;

        println!("Using node_runner at: {}", binary_path.display());

        // Spawn node_runner binary
        let process = Command::new(&binary_path)
            .arg(node_id)
            .arg(port.to_string())
            .spawn()
            .map_err(|e| format!("Failed to spawn node_runner: {}", e))?;

        // Poll until the gRPC server accepts connections (up to 5 s, 100 ms intervals).
        let addr = format!("http://127.0.0.1:{}", port);
        let client = {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match ActorServiceClient::connect(addr.clone()).await {
                    Ok(c) => break c,
                    Err(_) if std::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        return Err(format!(
                            "Failed to connect to node {} at {} within 5s: {}",
                            node_id, addr, e
                        )
                        .into());
                    }
                }
            }
        };

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
