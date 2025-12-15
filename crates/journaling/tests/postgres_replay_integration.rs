// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// PostgreSQL integration tests for Phase 9.1: Deterministic Replay
//
// Purpose: Verify deterministic replay works with PostgreSQL backend
// Coverage: Same tests as SQLite but using PostgreSQL

#[cfg(feature = "postgres-backend")]
mod postgres_integration_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::PostgresJournalStorage;
    use plexspaces_facet::Facet;
    use plexspaces_proto::prost_types;
    use plexspaces_mailbox::Message;
    use plexspaces_core::{ActorContext, ServiceLocator};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use async_trait::async_trait;
    use serde_json::Value as JsonValue;

    /// Helper to create a test PostgreSQL storage
    /// Uses test database URL from environment or defaults to local test DB
    async fn create_test_storage() -> PostgresJournalStorage {
        let db_url = std::env::var("TEST_POSTGRES_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/plexspaces_test".to_string());
        PostgresJournalStorage::new(&db_url).await.unwrap()
    }

    /// Helper to create durability config
    fn create_durability_config(
        checkpoint_interval: u64,
        replay_on_activation: bool,
    ) -> DurabilityConfig {
        DurabilityConfig {
            backend: JournalBackend::JournalBackendPostgres as i32,
            checkpoint_interval,
            checkpoint_timeout: None,
            replay_on_activation,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        }
    }

    /// Test actor: Simple counter
    struct CounterActor {
        count: u64,
    }

    impl CounterActor {
        fn new() -> Self {
            Self { count: 0 }
        }

        fn get_count(&self) -> u64 {
            self.count
        }

        fn increment(&mut self) {
            self.count += 1;
        }
    }

    /// ReplayHandler implementation for testing
    struct TestReplayHandler {
        counter: Arc<RwLock<CounterActor>>,
    }

    #[async_trait]
    impl ReplayHandler for TestReplayHandler {
        async fn replay_message(
            &self,
            message: Message,
            _context: &ActorContext,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            if message.message_type == "increment" {
                let mut counter = self.counter.write().await;
                counter.increment();
            }
            Ok(())
        }
    }

    /// StateLoader implementation for CounterActor
    struct CounterStateLoader {
        counter: Arc<RwLock<CounterActor>>,
    }

    #[async_trait]
    impl StateLoader for CounterStateLoader {
        fn deserialize(&self, state_data: &[u8]) -> JournalResult<JsonValue> {
            if state_data.len() < 8 {
                return Ok(serde_json::json!({ "count": 0 }));
            }
            let count = u64::from_le_bytes(
                state_data[0..8].try_into().map_err(|_| {
                    JournalError::Serialization("Invalid state data length".to_string())
                })?,
            );
            Ok(serde_json::json!({ "count": count }))
        }

        async fn restore_state(&self, state: &JsonValue) -> JournalResult<()> {
            let count = state["count"]
                .as_u64()
                .ok_or_else(|| JournalError::Serialization("Invalid state format".to_string()))?;
            let mut counter = self.counter.write().await;
            counter.count = count;
            Ok(())
        }

        fn schema_version(&self) -> u32 {
            1
        }
    }

    /// Test 1: Full replay (no checkpoint) - Happy path
    #[tokio::test]
    #[ignore] // Requires PostgreSQL database
    async fn test_full_replay_no_checkpoint() {
        let storage = create_test_storage().await;
        let config = create_durability_config(1000, true);
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "counter-1";

        facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        for i in 1..=5 {
            let method = "increment";
            let payload = format!("{}", i).into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("count = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 10, "Should have 10 entries (5 messages * 2)");

        facet.on_detach(actor_id).await.unwrap();

        let counter = Arc::new(RwLock::new(CounterActor::new()));
        let handler = TestReplayHandler {
            counter: Arc::clone(&counter),
        };

        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.set_replay_handler(Box::new(handler)).await;
        new_facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries_after.len(), 10, "Entries should persist after restart");
    }

    /// Test 2: Checkpoint + Delta Replay
    #[tokio::test]
    #[ignore] // Requires PostgreSQL database
    async fn test_checkpoint_delta_replay() {
        let storage = create_test_storage().await;
        let config = create_durability_config(50, true);
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "counter-2";

        facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        for i in 1..=100 {
            let method = "increment";
            let payload = format!("{}", i).into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("count = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();

            if i == 50 {
                storage.flush().await.unwrap();
                let checkpoint = Checkpoint {
                    actor_id: actor_id.to_string(),
                    sequence: 100,
                    timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                    state_data: 50u64.to_le_bytes().to_vec(),
                    compression: CompressionType::CompressionTypeNone as i32,
                    metadata: Default::default(),
                    state_schema_version: 1,
                };
                storage.save_checkpoint(&checkpoint).await.unwrap();
            }
        }

        storage.flush().await.unwrap();

        let checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert!(checkpoint.sequence >= 100, "Checkpoint should be at sequence >= 100");

        facet.on_detach(actor_id).await.unwrap();

        let counter = Arc::new(RwLock::new(CounterActor::new()));
        let state_loader = CounterStateLoader {
            counter: Arc::clone(&counter),
        };

        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.set_state_loader(Box::new(state_loader)).await;
        new_facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        let loaded_checkpoint = new_facet.get_latest_checkpoint().await.unwrap();
        assert!(loaded_checkpoint.is_some(), "Checkpoint should be available");
    }

    /// Test 3: Schema Version Validation
    #[tokio::test]
    #[ignore] // Requires PostgreSQL database
    async fn test_schema_version_validation() {
        let storage = create_test_storage().await;
        let mut config = create_durability_config(100, true);
        config.state_schema_version = 1;

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "counter-3";

        facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
            state_data: vec![],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 2,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        facet.on_detach(actor_id).await.unwrap();

        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        let result = new_facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await;

        assert!(result.is_err(), "Should fail with schema version mismatch");
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.contains("Incompatible checkpoint schema version"),
            "Error should mention schema version"
        );
    }

    /// Test 4: Automatic State Loading
    #[tokio::test]
    #[ignore] // Requires PostgreSQL database
    async fn test_automatic_state_loading() {
        let storage = create_test_storage().await;
        let config = create_durability_config(100, true);
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "counter-4";

        facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
            state_data: 42u64.to_le_bytes().to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        facet.on_detach(actor_id).await.unwrap();

        let counter = Arc::new(RwLock::new(CounterActor::new()));
        let state_loader = CounterStateLoader {
            counter: Arc::clone(&counter),
        };

        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.set_state_loader(Box::new(state_loader)).await;
        new_facet.on_attach(actor_id, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        let count = counter.read().await.get_count();
        assert_eq!(count, 42, "State should be automatically restored to 42");
    }
}
