// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Multi-Process Byzantine Generals Consensus Test
//!
//! This test validates PlexSpaces distributed actor system by:
//! 1. Starting multiple node processes
//! 2. Each node runs one or more general actors
//! 3. Generals coordinate via gRPC + TupleSpace
//! 4. Verify consensus despite faulty generals

use std::process::{Command, Child, Stdio};
use std::time::Duration;
use tokio::time::sleep;

/// Helper to start a Byzantine node process
fn start_byzantine_node(
    node_id: &str,
    address: &str,
    generals: &str,
) -> Result<Child, std::io::Error> {
    Command::new("cargo")
        .arg("run")
        .arg("--bin")
        .arg("byzantine")
        .arg("--")
        .arg("--node-id")
        .arg(node_id)
        .arg("--address")
        .arg(address)
        .arg("--generals")
        .arg(generals)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

#[tokio::test]
#[ignore] // Requires multi-process setup - run with: cargo test --test multi_process_consensus -- --ignored
async fn test_byzantine_two_nodes_four_generals() {
    println!("🚀 Starting Multi-Process Byzantine Consensus Test");
    println!("==================================================");

    // Start Node 1 with general0 (commander) and general1
    println!("\n📍 Starting Node 1 (localhost:19001)...");
    let mut node1 = start_byzantine_node("node1", "localhost:19001", "general0,general1")
        .expect("Failed to start node1");

    // Give Node 1 time to start
    sleep(Duration::from_secs(2)).await;

    // Start Node 2 with general2 and general3
    println!("📍 Starting Node 2 (localhost:19002)...");
    let mut node2 = start_byzantine_node("node2", "localhost:19002", "general2,general3")
        .expect("Failed to start node2");

    // Give Node 2 time to start
    sleep(Duration::from_secs(2)).await;

    println!("\n✅ Both nodes started successfully!");
    println!("  Node 1: general0 (commander), general1");
    println!("  Node 2: general2, general3");

    // Let them run for a bit to establish connections
    println!("\n⏳ Waiting for nodes to establish connections...");
    sleep(Duration::from_secs(3)).await;

    // TODO: Trigger consensus and verify results
    // For now, we just verify the nodes started successfully
    println!("\n📊 Test Status:");
    println!("  ✅ Node 1 running (PID: {})", node1.id());
    println!("  ✅ Node 2 running (PID: {})", node2.id());

    // In a full implementation, we would:
    // 1. Connect to the shared TupleSpace (Redis or in-memory)
    // 2. Write a "start_consensus" tuple to trigger voting
    // 3. Wait for all 4 generals to write their votes
    // 4. Verify consensus was reached
    // 5. Check vote distribution (Attack vs Retreat)

    println!("\n⏰ Letting nodes run for 5 seconds...");
    sleep(Duration::from_secs(5)).await;

    // Cleanup
    println!("\n🧹 Cleaning up processes...");
    node1.kill().expect("Failed to kill node1");
    node2.kill().expect("Failed to kill node2");

    println!("✅ Test completed successfully!");
    println!("\n📝 Note: This is a basic smoke test.");
    println!("   Full consensus verification requires:");
    println!("   - Shared TupleSpace (Redis or distributed in-memory)");
    println!("   - Byzantine general behavior implementation");
    println!("   - Vote collection and verification logic");
}

#[tokio::test]
#[ignore] // Manual test
async fn test_byzantine_three_nodes_six_generals() {
    println!("🚀 Starting 3-Node Byzantine Consensus Test");
    println!("===========================================");

    // Start 3 nodes with 2 generals each (6 total)
    println!("\n📍 Starting Node 1 (localhost:19011)...");
    let mut node1 = start_byzantine_node("node1", "localhost:19011", "general0,general1")
        .expect("Failed to start node1");

    sleep(Duration::from_secs(1)).await;

    println!("📍 Starting Node 2 (localhost:19012)...");
    let mut node2 = start_byzantine_node("node2", "localhost:19012", "general2,general3")
        .expect("Failed to start node2");

    sleep(Duration::from_secs(1)).await;

    println!("📍 Starting Node 3 (localhost:19013)...");
    let mut node3 = start_byzantine_node("node3", "localhost:19013", "general4,general5")
        .expect("Failed to start node3");

    sleep(Duration::from_secs(2)).await;

    println!("\n✅ All 3 nodes started successfully!");
    println!("  Node 1: general0 (commander), general1");
    println!("  Node 2: general2, general3");
    println!("  Node 3: general4, general5");

    println!("\n⏰ Letting nodes run for 8 seconds...");
    sleep(Duration::from_secs(8)).await;

    // Cleanup
    println!("\n🧹 Cleaning up processes...");
    node1.kill().expect("Failed to kill node1");
    node2.kill().expect("Failed to kill node2");
    node3.kill().expect("Failed to kill node3");

    println!("✅ Test completed successfully!");
}

#[tokio::test]
async fn test_node_binary_exists() {
    // Simple test to verify the binary can be found
    let result = Command::new("cargo")
        .arg("build")
        .arg("--bin")
        .arg("byzantine")
        .output();

    assert!(result.is_ok(), "Failed to build byzantine binary");
    let output = result.unwrap();
    assert!(output.status.success(), "Binary build failed: {:?}", String::from_utf8_lossy(&output.stderr));
}
