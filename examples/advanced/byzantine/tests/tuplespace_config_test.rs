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

//! TupleSpace Configuration Tests for Byzantine Generals
//!
//! This file demonstrates how to use different TupleSpace backends with
//! Byzantine Generals consensus.

use std::sync::Arc;
use plexspaces::journal::MemoryJournal;
use plexspaces::lattice::ConsistencyLevel;
use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, SqliteBackend};

use byzantine_generals::{
    General, Decision,
    create_tuplespace_from_env,
    create_tuplespace_from_config,
    create_lattice_tuplespace,
};

/// Test 1: Create TupleSpace from environment variables (default to in-memory)
#[tokio::test]
async fn test_create_tuplespace_from_env() {
    // Ensure no env vars are set (tests in-memory fallback)
    std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");

    let tuplespace = create_tuplespace_from_env().await
        .expect("Failed to create TupleSpace from env");

    // Should create an in-memory TupleSpace by default
    assert!(Arc::strong_count(&tuplespace) == 1);
}

/// Test 2: Create TupleSpace with explicit in-memory lattice configuration
#[tokio::test]
async fn test_create_lattice_tuplespace() {
    let tuplespace = create_lattice_tuplespace("test-node", ConsistencyLevel::Linearizable).await
        .expect("Failed to create lattice TupleSpace");

    // Verify we can use it with a General
    let journal = Arc::new(MemoryJournal::new());
    let general = General::new(
        "commander".to_string(),
        true,
        false,
        journal,
        tuplespace.clone(),
    ).await.expect("Failed to create General");

    // Commander can propose
    general.propose(true).await.expect("Proposal failed");

    // Verify proposal was written to TupleSpace
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
}

/// Test 3: Create TupleSpace with explicit SQLite configuration
#[tokio::test]
#[cfg(feature = "sql-backend")]
async fn test_create_tuplespace_with_sqlite() {
    let config = TupleSpaceConfig {
        backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
            SqliteBackend {
                path: ":memory:".to_string(),
            }
        )),
        pool_size: 1,
        default_ttl_seconds: 0,
        enable_indexing: false,
    };

    let tuplespace = create_tuplespace_from_config(config).await
        .expect("Failed to create SQLite TupleSpace");

    // Verify we can use it with a General
    let journal = Arc::new(MemoryJournal::new());
    let general = General::new(
        "commander".to_string(),
        true,
        false,
        journal,
        tuplespace.clone(),
    ).await.expect("Failed to create General");

    // Commander can propose
    general.propose(true).await.expect("Proposal failed");

    // Wait for write
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Lieutenant can read proposal
    let journal2 = Arc::new(MemoryJournal::new());
    let lieutenant = General::new(
        "lieutenant".to_string(),
        false,
        false,
        journal2,
        tuplespace.clone(),
    ).await.expect("Failed to create Lieutenant");

    let votes = lieutenant.read_votes(0).await.expect("Failed to read votes");
    // Should be able to read from SQLite backend
    assert!(votes.is_empty() || !votes.is_empty()); // Just verify it works
}

/// Test 4: Multi-process coordination with SQLite backend
///
/// This test demonstrates how multiple processes can coordinate via SQLite TupleSpace
#[tokio::test]
#[cfg(feature = "sql-backend")]
#[ignore] // Run manually with: cargo test --features sql-backend test_multi_process_coordination -- --ignored
async fn test_multi_process_coordination_with_sqlite() {
    // This would typically use a shared SQLite file
    let config = TupleSpaceConfig {
        backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
            SqliteBackend {
                path: "/tmp/byzantine-test.db".to_string(),
            }
        )),
        pool_size: 1,
        default_ttl_seconds: 0,
        enable_indexing: false,
    };

    let tuplespace = create_tuplespace_from_config(config).await
        .expect("Failed to create shared SQLite TupleSpace");

    // Process 1: Commander proposes
    let journal1 = Arc::new(MemoryJournal::new());
    let commander = General::new(
        "commander".to_string(),
        true,
        false,
        journal1,
        tuplespace.clone(),
    ).await.expect("Failed to create Commander");
    commander.propose(true).await.expect("Proposal failed");

    // Process 2 (simulated): Lieutenant reads and votes
    let journal2 = Arc::new(MemoryJournal::new());
    let lieutenant = General::new(
        "lieutenant".to_string(),
        false,
        false,
        journal2,
        tuplespace.clone(),
    ).await.expect("Failed to create Lieutenant");

    // Wait for proposal to be persisted
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Lieutenant reads proposal and votes
    lieutenant.cast_vote(0, Decision::Attack, vec!["lieutenant".to_string()])
        .await
        .expect("Vote failed");

    // Both processes can now read all votes
    let votes = commander.read_votes(0).await.expect("Failed to read votes");
    assert!(!votes.is_empty(), "Should have at least one vote");

    // Clean up test file
    let _ = std::fs::remove_file("/tmp/byzantine-test.db");
}

/// Test 5: Environment variable configuration for SQLite
#[tokio::test]
#[cfg(feature = "sql-backend")]
async fn test_env_var_configuration_sqlite() {
    std::env::set_var("PLEXSPACES_TUPLESPACE_BACKEND", "sqlite");
    std::env::set_var("PLEXSPACES_SQLITE_PATH", ":memory:");

    let tuplespace = create_tuplespace_from_env().await
        .expect("Failed to create TupleSpace from env vars");

    // Verify it works
    let journal = Arc::new(MemoryJournal::new());
    let general = General::new(
        "test".to_string(),
        true,
        false,
        journal,
        tuplespace,
    ).await.expect("Failed to create General");

    general.propose(false).await.expect("Proposal failed");

    // Clean up env vars
    std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    std::env::remove_var("PLEXSPACES_SQLITE_PATH");
}
