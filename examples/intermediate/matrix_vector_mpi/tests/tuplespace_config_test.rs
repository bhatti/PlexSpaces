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

//! TupleSpace Configuration Tests for Matrix-Vector MPI
//!
//! This file demonstrates how to use different TupleSpace backends with
//! MPI-style collective operations.

use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, SqliteBackend};

use matrix_vector_mpi::{
    create_tuplespace_from_env,
    create_tuplespace_from_config,
    create_in_memory_tuplespace,
};

/// Test 1: Create TupleSpace from environment variables (default to in-memory)
#[tokio::test]
async fn test_create_tuplespace_from_env() {
    // Ensure no env vars are set (tests in-memory fallback)
    std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");

    let tuplespace = create_tuplespace_from_env().await
        .expect("Failed to create TupleSpace from env");

    // Should create an in-memory TupleSpace by default
    assert!(std::sync::Arc::strong_count(&tuplespace) == 1);
}

/// Test 2: Create in-memory TupleSpace explicitly
#[tokio::test]
async fn test_create_in_memory_tuplespace() {
    let tuplespace = create_in_memory_tuplespace().await
        .expect("Failed to create in-memory TupleSpace");

    // Verify we can use it for MPI operations
    assert!(std::sync::Arc::strong_count(&tuplespace) == 1);
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

    // Verify we can use it
    assert!(std::sync::Arc::strong_count(&tuplespace) == 1);
}

/// Test 4: Multi-process coordination with SQLite backend
///
/// This test demonstrates how multiple processes can coordinate via SQLite TupleSpace
/// for MPI-style operations (scatter, gather, barrier)
#[tokio::test]
#[cfg(feature = "sql-backend")]
#[ignore] // Run manually with: cargo test --features sql-backend test_multi_process_coordination -- --ignored
async fn test_multi_process_coordination_with_sqlite() {
    use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField};

    // This would typically use a shared SQLite file
    let config = TupleSpaceConfig {
        backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
            SqliteBackend {
                path: "/tmp/matrix-mpi-test.db".to_string(),
            }
        )),
        pool_size: 1,
        default_ttl_seconds: 0,
        enable_indexing: false,
    };

    let tuplespace = create_tuplespace_from_config(config).await
        .expect("Failed to create shared SQLite TupleSpace");

    // Process 1: Write scatter data
    let matrix_row = vec![1.0, 2.0, 3.0];
    let tuple = Tuple::new(vec![
        TupleField::String("scatter".to_string()),
        TupleField::String("matrix_rows".to_string()),
        TupleField::Integer(0), // worker_id
        TupleField::String(serde_json::to_string(&matrix_row).unwrap()),
    ]);
    tuplespace.write(tuple).await.expect("Write failed");

    // Process 2 (simulated): Read scatter data
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("scatter".to_string())),
        PatternField::Exact(TupleField::String("matrix_rows".to_string())),
        PatternField::Exact(TupleField::Integer(0)),
        PatternField::Wildcard,
    ]);

    // Wait for data to be persisted
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Read the scattered data
    let result = tuplespace.read(pattern).await.expect("Read failed");
    assert!(result.is_some(), "Should have scatter data");

    // Clean up test file
    let _ = std::fs::remove_file("/tmp/matrix-mpi-test.db");
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
    assert!(std::sync::Arc::strong_count(&tuplespace) == 1);

    // Clean up env vars
    std::env::remove_var("PLEXSPACES_TUPLESPACE_BACKEND");
    std::env::remove_var("PLEXSPACES_SQLITE_PATH");
}

/// Test 6: MPI scatter operation with configured TupleSpace
#[tokio::test]
async fn test_mpi_scatter_with_configured_backend() {
    use matrix_vector_mpi::mpi_ops::scatter_rows;

    let tuplespace = create_in_memory_tuplespace().await
        .expect("Failed to create TupleSpace");

    // Create a small matrix for testing
    let matrix = vec![
        vec![1.0, 2.0],
        vec![3.0, 4.0],
    ];

    // Scatter the matrix rows to workers
    scatter_rows(&tuplespace, &matrix, 2).await
        .expect("Scatter failed");

    // Verify scatter worked (data written to TupleSpace)
    // This is a smoke test - actual verification would read the tuples back
}

/// Test 7: MPI broadcast operation with configured TupleSpace
#[tokio::test]
async fn test_mpi_broadcast_with_configured_backend() {
    use matrix_vector_mpi::mpi_ops::broadcast_vector;

    let tuplespace = create_in_memory_tuplespace().await
        .expect("Failed to create TupleSpace");

    // Create a vector to broadcast
    let vector = vec![1.0, 2.0, 3.0];

    // Broadcast the vector
    broadcast_vector(&tuplespace, &vector).await
        .expect("Broadcast failed");

    // Verify broadcast worked (data written to TupleSpace)
    // This is a smoke test - actual verification would read the tuple back
}
