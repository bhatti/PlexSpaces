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

//! Integration tests for WASM actor checkpoint-based durability
//!
//! ## Purpose
//! Tests the Cloudflare Durable Objects pattern for WASM actors:
//! - Checkpoint save on actor shutdown (terminate)
//! - Checkpoint load on actor startup (init)
//! - State persistence across actor restarts
//!
//! ## Architecture Context
//! WASM actors use checkpoint-based durability (not DurabilityFacet/replay):
//! - Actor implements get-state() and set-state() WIT functions
//! - Framework auto-saves state on terminate via save_checkpoint()
//! - Framework auto-restores state on init via load_checkpoint()

#[cfg(test)]
mod tests {
    use plexspaces_core::ActorId;
    use plexspaces_core::journal_storage::{Checkpoint, JournalStorage};
    use plexspaces_journaling::sql::SqliteJournalStorage;

    fn canonical_actor_id(name: &str) -> String {
        ActorId::new(name, "gen_server", "default", "test-node")
            .unwrap()
            .to_string()
    }

    /// Test: Checkpoint struct creation and serialization
    #[test]
    fn test_checkpoint_struct_creation() {
        let checkpoint = Checkpoint {
            actor_id: canonical_actor_id("bank-account-alice"),
            sequence: 1,
            timestamp: None,
            state_data: br#"{"balance":1000,"transactions":[]}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };

        assert_eq!(checkpoint.actor_id, canonical_actor_id("bank-account-alice"));
        assert_eq!(checkpoint.sequence, 1);
        assert!(!checkpoint.state_data.is_empty());
    }

    /// Test: SQLite journal storage checkpoint save/load roundtrip
    #[tokio::test]
    async fn test_sqlite_checkpoint_save_load_roundtrip() {
        // Create in-memory SQLite journal storage
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create SQLite journal storage");

        let actor_id = canonical_actor_id("test-actor-checkpoint");
        let state_json = r#"{"balance":500,"transactions":[{"type":"deposit","amount":500}]}"#;

        // Create and save checkpoint
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: None,
            state_data: state_json.as_bytes().to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };

        storage
            .save_checkpoint(&checkpoint)
            .await
            .expect("Failed to save checkpoint");

        // Load checkpoint
        let loaded = storage
            .get_latest_checkpoint(&actor_id)
            .await
            .expect("Failed to load checkpoint");

        assert_eq!(loaded.actor_id, actor_id);
        assert_eq!(loaded.state_data, state_json.as_bytes());

        // Verify state content
        let state: serde_json::Value =
            serde_json::from_slice(&loaded.state_data).expect("Invalid JSON in checkpoint");
        assert_eq!(state["balance"], 500);
    }

    /// Test: Checkpoint update overwrites previous state
    #[tokio::test]
    async fn test_checkpoint_update_overwrites() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        let actor_id = canonical_actor_id("test-actor-update");

        // Save initial checkpoint with balance 100
        let checkpoint1 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: None,
            state_data: br#"{"balance":100}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&checkpoint1).await.unwrap();

        // Save updated checkpoint with balance 500
        let checkpoint2 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 2,
            timestamp: None,
            state_data: br#"{"balance":500}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&checkpoint2).await.unwrap();

        // Load latest should return balance 500
        let loaded = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        let state: serde_json::Value = serde_json::from_slice(&loaded.state_data).unwrap();
        assert_eq!(state["balance"], 500);
    }

    /// Test: Checkpoint not found returns proper error
    #[tokio::test]
    async fn test_checkpoint_not_found() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        let result = storage
            .get_latest_checkpoint(&canonical_actor_id("nonexistent-actor"))
            .await;

        assert!(result.is_err());
        match result {
            Err(plexspaces_core::journal_storage::JournalError::CheckpointNotFound(_)) => {
                // Expected error
            }
            Err(e) => panic!("Expected CheckpointNotFound, got: {:?}", e),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    /// Test: Multiple actors have independent checkpoints
    #[tokio::test]
    async fn test_multiple_actors_independent_checkpoints() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        // Save checkpoint for Alice with balance 1000
        let alice_checkpoint = Checkpoint {
            actor_id: canonical_actor_id("account-alice"),
            sequence: 1,
            timestamp: None,
            state_data: br#"{"balance":1000}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&alice_checkpoint).await.unwrap();

        // Save checkpoint for Bob with balance 500
        let bob_checkpoint = Checkpoint {
            actor_id: canonical_actor_id("account-bob"),
            sequence: 1,
            timestamp: None,
            state_data: br#"{"balance":500}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&bob_checkpoint).await.unwrap();

        // Load Alice's checkpoint
        let alice_loaded = storage
            .get_latest_checkpoint(&canonical_actor_id("account-alice"))
            .await
            .unwrap();
        let alice_state: serde_json::Value =
            serde_json::from_slice(&alice_loaded.state_data).unwrap();
        assert_eq!(alice_state["balance"], 1000);

        // Load Bob's checkpoint
        let bob_loaded = storage
            .get_latest_checkpoint(&canonical_actor_id("account-bob"))
            .await
            .unwrap();
        let bob_state: serde_json::Value = serde_json::from_slice(&bob_loaded.state_data).unwrap();
        assert_eq!(bob_state["balance"], 500);
    }

    /// Test: State schema versioning
    #[tokio::test]
    async fn test_checkpoint_schema_versioning() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        let actor_id = canonical_actor_id("test-actor-versioned");

        // Save v1 checkpoint
        let checkpoint_v1 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: None,
            state_data: br#"{"version":1,"balance":100}"#.to_vec(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&checkpoint_v1).await.unwrap();

        // Upgrade to v2 checkpoint with additional fields
        let checkpoint_v2 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 2,
            timestamp: None,
            state_data: br#"{"version":2,"balance":100,"currency":"USD"}"#.to_vec(),
            compression: 0,
            state_schema_version: 2,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&checkpoint_v2).await.unwrap();

        // Load and verify version 2
        let loaded = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        assert_eq!(loaded.state_schema_version, 2);

        let state: serde_json::Value = serde_json::from_slice(&loaded.state_data).unwrap();
        assert_eq!(state["version"], 2);
        assert_eq!(state["currency"], "USD");
    }

    /// Test: Large state checkpoint (bank account with many transactions)
    #[tokio::test]
    async fn test_large_state_checkpoint() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        let actor_id = canonical_actor_id("test-actor-large");

        // Create state with 100 transactions
        let transactions: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "id": i,
                    "type": if i % 2 == 0 { "deposit" } else { "withdraw" },
                    "amount": (i + 1) * 10
                })
            })
            .collect();

        let state = serde_json::json!({
            "balance": 5500,
            "transactions": transactions
        });

        let state_bytes = serde_json::to_vec(&state).unwrap();

        // Save checkpoint with large state
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 100,
            timestamp: None,
            state_data: state_bytes.clone(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        // Load and verify
        let loaded = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        assert_eq!(loaded.state_data.len(), state_bytes.len());

        let loaded_state: serde_json::Value = serde_json::from_slice(&loaded.state_data).unwrap();
        assert_eq!(loaded_state["balance"], 5500);
        assert_eq!(loaded_state["transactions"].as_array().unwrap().len(), 100);
    }
}

/// Integration tests that simulate WASM actor restart scenarios
#[cfg(test)]
mod restart_scenario_tests {
    use plexspaces_core::ActorId;
    use plexspaces_core::journal_storage::{Checkpoint, JournalStorage};
    use plexspaces_journaling::sql::SqliteJournalStorage;

    fn canonical_actor_id(name: &str) -> String {
        ActorId::new(name, "gen_server", "default", "test-node")
            .unwrap()
            .to_string()
    }

    /// Simulates bank account actor restart scenario
    ///
    /// This test simulates:
    /// 1. Actor starts fresh (no checkpoint)
    /// 2. Operations performed (deposits, withdrawals)
    /// 3. Actor shuts down (checkpoint saved)
    /// 4. Actor restarts (checkpoint loaded)
    /// 5. State is verified to match pre-shutdown state
    #[tokio::test]
    async fn test_bank_account_restart_scenario() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        let actor_id = canonical_actor_id("bank-account-alice");

        // === PHASE 1: First run (fresh start) ===

        // Simulate actor init - no checkpoint exists
        let init_result = storage.get_latest_checkpoint(&actor_id).await;
        assert!(
            init_result.is_err(),
            "Should have no checkpoint on first run"
        );

        // Simulate operations: deposit $1000, withdraw $200
        let state_after_ops = serde_json::json!({
            "balance": 800,
            "transactions": [
                {"type": "deposit", "amount": 1000},
                {"type": "withdraw", "amount": 200}
            ]
        });

        // Simulate actor terminate - save checkpoint
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 2,
            timestamp: None,
            state_data: serde_json::to_vec(&state_after_ops).unwrap(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage
            .save_checkpoint(&checkpoint)
            .await
            .expect("Failed to save checkpoint on shutdown");

        // === PHASE 2: Restart (checkpoint restored) ===

        // Simulate actor init - checkpoint should exist
        let loaded_checkpoint = storage
            .get_latest_checkpoint(&actor_id)
            .await
            .expect("Should have checkpoint after restart");

        // Verify restored state matches
        let restored_state: serde_json::Value =
            serde_json::from_slice(&loaded_checkpoint.state_data)
                .expect("Invalid checkpoint state");

        assert_eq!(restored_state["balance"], 800, "Balance should be restored");
        assert_eq!(
            restored_state["transactions"].as_array().unwrap().len(),
            2,
            "Transaction history should be restored"
        );

        // === PHASE 3: Continue operations after restart ===

        // Simulate more operations: deposit $500
        let state_after_more_ops = serde_json::json!({
            "balance": 1300,
            "transactions": [
                {"type": "deposit", "amount": 1000},
                {"type": "withdraw", "amount": 200},
                {"type": "deposit", "amount": 500}
            ]
        });

        // Save updated checkpoint
        let updated_checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 3,
            timestamp: None,
            state_data: serde_json::to_vec(&state_after_more_ops).unwrap(),
            compression: 0,
            state_schema_version: 1,
            metadata: std::collections::HashMap::new(),
        };
        storage
            .save_checkpoint(&updated_checkpoint)
            .await
            .expect("Failed to save updated checkpoint");

        // Verify final state
        let final_checkpoint = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        let final_state: serde_json::Value =
            serde_json::from_slice(&final_checkpoint.state_data).unwrap();
        assert_eq!(final_state["balance"], 1300);
        assert_eq!(final_state["transactions"].as_array().unwrap().len(), 3);
    }

    /// Simulates multiple bank accounts with independent checkpoints
    #[tokio::test]
    async fn test_multiple_bank_accounts_restart() {
        let storage = SqliteJournalStorage::new(":memory:")
            .await
            .expect("Failed to create storage");

        // Alice: $1000
        let alice_actor_id = "account-alice//account::default@node";
        let alice_state = serde_json::json!({"balance": 1000, "name": "alice"});
        storage
            .save_checkpoint(&Checkpoint {
                actor_id: alice_actor_id.to_string(),
                sequence: 1,
                timestamp: None,
                state_data: serde_json::to_vec(&alice_state).unwrap(),
                compression: 0,
                state_schema_version: 1,
                metadata: std::collections::HashMap::new(),
            })
            .await
            .unwrap();

        // Bob: $500
        let bob_actor_id = "account-bob//account::default@node";
        let bob_state = serde_json::json!({"balance": 500, "name": "bob"});
        storage
            .save_checkpoint(&Checkpoint {
                actor_id: bob_actor_id.to_string(),
                sequence: 1,
                timestamp: None,
                state_data: serde_json::to_vec(&bob_state).unwrap(),
                compression: 0,
                state_schema_version: 1,
                metadata: std::collections::HashMap::new(),
            })
            .await
            .unwrap();

        // Charlie: $250
        let charlie_actor_id = "account-charlie//account::default@node";
        let charlie_state = serde_json::json!({"balance": 250, "name": "charlie"});
        storage
            .save_checkpoint(&Checkpoint {
                actor_id: charlie_actor_id.to_string(),
                sequence: 1,
                timestamp: None,
                state_data: serde_json::to_vec(&charlie_state).unwrap(),
                compression: 0,
                state_schema_version: 1,
                metadata: std::collections::HashMap::new(),
            })
            .await
            .unwrap();

        // Simulate restart - all accounts should restore independently
        let alice_restored: serde_json::Value = serde_json::from_slice(
            &storage
                .get_latest_checkpoint(alice_actor_id)
                .await
                .unwrap()
                .state_data,
        )
        .unwrap();
        let bob_restored: serde_json::Value = serde_json::from_slice(
            &storage
                .get_latest_checkpoint(bob_actor_id)
                .await
                .unwrap()
                .state_data,
        )
        .unwrap();
        let charlie_restored: serde_json::Value = serde_json::from_slice(
            &storage
                .get_latest_checkpoint(charlie_actor_id)
                .await
                .unwrap()
                .state_data,
        )
        .unwrap();

        assert_eq!(alice_restored["balance"], 1000);
        assert_eq!(bob_restored["balance"], 500);
        assert_eq!(charlie_restored["balance"], 250);
    }
}
