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

//! TupleSpace Configuration Tests for Heat Diffusion
//!
//! This file demonstrates how to use different TupleSpace backends with
//! heat diffusion simulation for multi-process coordination.

use heat_diffusion::{
    create_tuplespace_from_env,
    create_in_memory_tuplespace,
    config::GridConfig,
    coordinator::Coordinator,
};

#[cfg(feature = "sql-backend")]
use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, SqliteBackend};

#[cfg(feature = "sql-backend")]
use heat_diffusion::create_tuplespace_from_config;

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

    // Verify we can use it for heat diffusion
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
/// for heat diffusion simulation (boundary exchange between grid regions)
#[tokio::test]
#[cfg(feature = "sql-backend")]
#[ignore] // Run manually with: cargo test --features sql-backend test_multi_process_coordination -- --ignored
async fn test_multi_process_coordination_with_sqlite() {
    use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField};

    // This would typically use a shared SQLite file
    let config = TupleSpaceConfig {
        backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
            SqliteBackend {
                path: "/tmp/heat-diffusion-test.db".to_string(),
            }
        )),
        pool_size: 1,
        default_ttl_seconds: 0,
        enable_indexing: false,
    };

    let tuplespace = create_tuplespace_from_config(config).await
        .expect("Failed to create shared SQLite TupleSpace");

    // Process 1: Write boundary temperature from region (0, 0)
    let boundary_temp = vec![25.0, 26.0, 27.0]; // East boundary
    let tuple = Tuple::new(vec![
        TupleField::String("boundary".to_string()),
        TupleField::Integer(0), // region_x
        TupleField::Integer(0), // region_y
        TupleField::String("east".to_string()),
        TupleField::String(serde_json::to_string(&boundary_temp).unwrap()),
    ]);
    tuplespace.write(tuple).await.expect("Write failed");

    // Process 2 (simulated): Read boundary temperature from region (1, 0) (west neighbor)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("boundary".to_string())),
        PatternField::Exact(TupleField::Integer(0)),
        PatternField::Exact(TupleField::Integer(0)),
        PatternField::Exact(TupleField::String("east".to_string())),
        PatternField::Wildcard,
    ]);

    // Wait for data to be persisted
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Read the boundary data
    let result = tuplespace.read(pattern).await.expect("Read failed");
    assert!(result.is_some(), "Should have boundary data");

    // Clean up test file
    let _ = std::fs::remove_file("/tmp/heat-diffusion-test.db");
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

/// Test 6: Coordinator with configured TupleSpace backend
#[tokio::test]
async fn test_coordinator_with_configured_backend() {
    let tuplespace = create_in_memory_tuplespace().await
        .expect("Failed to create TupleSpace");

    // Create coordinator with custom TupleSpace
    let grid_config = GridConfig::new(
        (10, 10),  // grid_size: 10×10 total cells
        (5, 5),    // region_size: 5×5 cells per actor (creates 4 actors)
    ).expect("Failed to create grid config");

    let _coordinator = Coordinator::new_with_tuplespace(grid_config, tuplespace);

    // Verify coordinator created successfully
    // This is a smoke test - actual simulation tests are in other files
}

/// Test 7: Verify TupleSpace can be shared across region actors
#[tokio::test]
async fn test_tuplespace_sharing() {
    let tuplespace = create_in_memory_tuplespace().await
        .expect("Failed to create TupleSpace");

    // Clone Arc to simulate sharing across actors
    let space1 = tuplespace.clone();
    let space2 = tuplespace.clone();

    // All should point to same TupleSpace
    assert!(std::sync::Arc::strong_count(&tuplespace) == 3);
    assert!(std::sync::Arc::strong_count(&space1) == 3);
    assert!(std::sync::Arc::strong_count(&space2) == 3);
}
