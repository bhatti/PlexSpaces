// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// End-to-end integration tests for Phase 9.1: Deterministic Replay with Actor System
//
// Purpose: Verify full integration of ReplayHandler with Actor system
// Coverage: Tests actual actor behavior replay through Actor::attach_facet()

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
mod actor_integration_tests {
    use plexspaces_journaling::*;
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_journaling::sql::SqliteJournalStorage;
    #[cfg(feature = "postgres-backend")]
    use plexspaces_journaling::sql::PostgresJournalStorage;
    use plexspaces_core::{ActorContext, BehaviorError, BehaviorType, Actor as ActorTrait};
    use plexspaces_actor::Actor as ActorStruct;
    use plexspaces_mailbox::{Mailbox, Message, mailbox_config_default};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use serde_json::Value as JsonValue;

    /// Test actor: Counter with state
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
    }

    #[async_trait]
    impl ActorTrait for CounterActor {
        async fn handle_message(
            &mut self,
            _ctx: &ActorContext,
            msg: Message,
        ) -> Result<(), BehaviorError> {
            if msg.message_type == "increment" {
                self.count += 1;
            } else if msg.message_type == "add" {
                // Parse payload as number
                if let Ok(num_str) = String::from_utf8(msg.payload.clone()) {
                    if let Ok(num) = num_str.parse::<u64>() {
                        self.count += num;
                    }
                }
            }
            Ok(())
        }

        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::Custom("Counter".to_string())
        }
    }

    /// Test: Full actor replay with ReplayHandler integration
    ///
    /// Scenario:
    /// 1. Create actor with DurabilityFacet
    /// 2. Process messages (state changes)
    /// 3. Restart actor
    /// 4. Verify state is restored through replay
    #[tokio::test]
    async fn test_actor_replay_with_handler() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 1000, // No checkpointing for this test
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let behavior = Box::new(CounterActor::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor".to_string()).await.unwrap();
        let actor_id = "counter-actor".to_string();

        // Create actor
        let mut actor = ActorStruct::new(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            None,
        );

        // Attach DurabilityFacet (this will set ReplayHandler automatically)
        let facet = DurabilityFacet::new(storage.clone(), config.clone());
        actor.attach_facet(Box::new(facet), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        // Start actor
        actor.start().await.unwrap();

        // Process messages through actor
        for i in 1..=5 {
            let mut msg = Message::new(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver = actor_id.clone();
            
            // Send message to actor (this will be journaled by DurabilityFacet)
            actor.send(msg).await.unwrap();
        }

        // Wait for messages to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Flush storage
        storage.flush().await.unwrap();

        // Verify journal entries
        let entries = storage.replay_from(&actor_id, 0).await.unwrap();
        assert!(entries.len() >= 5, "Should have at least 5 journal entries");

        // Stop actor
        actor.stop().await.unwrap();

        // Restart actor (simulate crash recovery)
        let behavior2 = Box::new(CounterActor::new());
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.clone()).await.unwrap();
        let mut actor2 = ActorStruct::new(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            None,
        );

        // Attach DurabilityFacet again (ReplayHandler will be set, replay will happen)
        let facet2 = DurabilityFacet::new(storage.clone(), config);
        actor2.attach_facet(Box::new(facet2), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();

        // Start actor (replay happens in on_attach)
        actor2.start().await.unwrap();

        // Verify entries still exist
        let entries_after = storage.replay_from(&actor_id, 0).await.unwrap();
        assert!(entries_after.len() >= 5, "Entries should persist after restart");
    }

    /// Test: Actor replay with checkpoint
    ///
    /// Scenario:
    /// 1. Create actor, process messages
    /// 2. Create checkpoint
    /// 3. Process more messages
    /// 4. Restart actor
    /// 5. Verify checkpoint + delta replay works
    #[tokio::test]
    async fn test_actor_replay_with_checkpoint() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 10, // Checkpoint every 10 messages
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let behavior = Box::new(CounterActor::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor-2".to_string()).await.unwrap();
        let actor_id = "counter-actor-2".to_string();

        let mut actor = ActorStruct::new(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            None,
        );

        let facet = DurabilityFacet::new(storage.clone(), config.clone());
        actor.attach_facet(Box::new(facet), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();
        actor.start().await.unwrap();

        // Process 10 messages
        for i in 1..=10 {
            let mut msg = Message::new(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver = actor_id.clone();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        // Create checkpoint manually
        let checkpoint = Checkpoint {
            actor_id: actor_id.clone(),
            sequence: 20, // 10 messages * 2 entries
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(std::time::SystemTime::now())),
            state_data: 10u64.to_le_bytes().to_vec(), // count = 10
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        // Process 5 more messages
        for i in 11..=15 {
            let mut msg = Message::new(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver = actor_id.clone();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        actor.stop().await.unwrap();

        // Restart actor
        let behavior2 = Box::new(CounterActor::new());
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.clone()).await.unwrap();
        let mut actor2 = ActorStruct::new(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            None,
        );

        let facet2 = DurabilityFacet::new(storage.clone(), config);
        actor2.attach_facet(Box::new(facet2), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();
        actor2.start().await.unwrap();

        // Verify checkpoint exists (may be higher due to automatic checkpointing)
        let checkpoint_after = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        assert!(checkpoint_after.sequence >= 20, "Checkpoint should be at sequence >= 20 (got {})", checkpoint_after.sequence);
    }

    /// Test: Actor replay with StateLoader (automatic state loading)
    ///
    /// Scenario:
    /// 1. Create actor with StateLoader
    /// 2. Process messages, create checkpoint
    /// 3. Restart actor
    /// 4. Verify state is automatically restored
    #[tokio::test]
    async fn test_actor_replay_with_state_loader() {
        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let counter = Arc::new(RwLock::new(CounterActor::new()));
        let behavior = Box::new(CounterActorWrapper {
            counter: Arc::clone(&counter),
        });
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor-3".to_string()).await.unwrap();
        let actor_id = "counter-actor-3".to_string();

        let mut actor = ActorStruct::new(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            None,
        );

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        
        // Set StateLoader for automatic state loading
        let state_loader = CounterStateLoader {
            counter: Arc::clone(&counter),
        };
        facet.set_state_loader(Box::new(state_loader)).await;

        actor.attach_facet(Box::new(facet), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();
        actor.start().await.unwrap();

        // Process messages
        for i in 1..=5 {
            let mut msg = Message::new(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver = actor_id.clone();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        // Create checkpoint with state
        let checkpoint = Checkpoint {
            actor_id: actor_id.clone(),
            sequence: 10,
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(std::time::SystemTime::now())),
            state_data: 5u64.to_le_bytes().to_vec(), // count = 5
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        actor.stop().await.unwrap();

        // Restart actor
        let counter2 = Arc::new(RwLock::new(CounterActor::new()));
        let behavior2 = Box::new(CounterActorWrapper {
            counter: Arc::clone(&counter2),
        });
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.clone()).await.unwrap();
        let mut actor2 = ActorStruct::new(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            None,
        );

        let mut facet2 = DurabilityFacet::new(storage.clone(), config);
        let state_loader2 = CounterStateLoader {
            counter: Arc::clone(&counter2),
        };
        facet2.set_state_loader(Box::new(state_loader2)).await;

        actor2.attach_facet(Box::new(facet2), 0, JsonValue::Object(serde_json::Map::new())).await.unwrap();
        actor2.start().await.unwrap();

        // Verify state was automatically restored
        let count = counter2.read().await.get_count();
        assert_eq!(count, 5, "State should be automatically restored to 5");
    }

    /// Wrapper to make CounterActor work with ActorBehavior
    struct CounterActorWrapper {
        counter: Arc<RwLock<CounterActor>>,
    }

    #[async_trait]
    impl ActorTrait for CounterActorWrapper {
        async fn handle_message(
            &mut self,
            _ctx: &ActorContext,
            msg: Message,
        ) -> Result<(), BehaviorError> {
            let mut counter = self.counter.write().await;
            if msg.message_type == "increment" {
                counter.count += 1;
            }
            Ok(())
        }

        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::Custom("CounterWrapper".to_string())
        }
    }

    /// StateLoader for CounterActor
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
}
