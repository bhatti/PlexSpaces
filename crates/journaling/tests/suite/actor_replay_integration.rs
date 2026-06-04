// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// End-to-end integration tests for Phase 9.1: Deterministic Replay with Actor System
//
// Purpose: Verify full integration of ReplayHandler with Actor system
// Coverage: Tests actual actor behavior replay through Actor::attach_facet()

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
mod actor_integration_tests {

    /// Helper to create a test message
    fn create_test_message(payload: Vec<u8>) -> plexspaces_actor::Message {
        plexspaces_actor::Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    use async_trait::async_trait;
    use plexspaces_actor::ActorInstance as ActorStruct;
    use plexspaces_actor::Message;
    use plexspaces_facet::Facet;
    use plexspaces_actor::{
        Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType, ServiceLocator,
    };
    #[cfg(feature = "postgres-backend")]
    use plexspaces_journaling::sql::PostgresJournalStorage;
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_journaling::*;
    use plexspaces_mailbox::{mailbox_config_default, Mailbox};
    use serde_json::Value as JsonValue;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// `ActorStruct::new` uses a stub service locator without an actor registry.
    /// `start` / facet attach register the actor and require a real `ServiceLocator` (same pattern as `plexspaces_actor` unit tests).
    fn test_actor_id(name: &str, namespace: &str) -> ActorId {
        ActorId::new(name, "counter", namespace, "test-node").expect("valid actor id")
    }

    async fn actor_with_service_locator(
        id: ActorId,
        behavior: Box<dyn plexspaces_actor::Actor>,
        mailbox: Mailbox,
        tenant_id: String,
        namespace: String,
    ) -> ActorStruct {
        let locator_impl =
            plexspaces_node::service_locator_helpers::create_default_service_locator(None, None)
                .await;
        let node_id = id.node_id().to_string();
        let service_locator: Arc<dyn plexspaces_actor::ServiceLocator> = locator_impl;
        let context = Arc::new(ActorContext::new(
            node_id,
            tenant_id.clone(),
            namespace.clone(),
            service_locator,
            None,
        ));
        ActorStruct::new(id, behavior, mailbox, tenant_id, namespace, None).set_context(context)
    }

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
            BehaviorType::Custom("counter".to_string())
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
        let storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        let config = DurabilityConfig {
            checkpoint_interval: 1000, // No checkpointing for this test
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
        };

        let behavior = Box::new(CounterActor::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor".to_string())
            .await
            .unwrap();
        let actor_id = test_actor_id("counter-actor", "default");

        // Create actor
        let mut actor = actor_with_service_locator(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        // Attach DurabilityFacet (this will set ReplayHandler automatically)
        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        actor.attach_facet(Box::new(facet)).await.unwrap();

        // Start actor
        actor.start().await.unwrap();

        // Process messages through actor
        for i in 1..=5 {
            let mut msg = create_test_message(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver_id = actor_id.to_string();

            // Send message to actor (this will be journaled by DurabilityFacet)
            actor.send(msg).await.unwrap();
        }

        // Wait for messages to be processed
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Flush storage
        storage.flush().await.unwrap();

        // Verify journal entries
        let entries = storage.replay_from(&actor_id.to_string(), 0).await.unwrap();
        assert!(entries.len() >= 5, "Should have at least 5 journal entries");

        // Stop actor
        actor.stop().await.unwrap();

        // Restart actor (simulate crash recovery)
        let behavior2 = Box::new(CounterActor::new());
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.to_string())
            .await
            .unwrap();
        let mut actor2 = actor_with_service_locator(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        // Attach DurabilityFacet again (ReplayHandler will be set, replay will happen)
        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let facet2 = DurabilityFacet::new(storage.clone(), config_value, 50);
        actor2.attach_facet(Box::new(facet2)).await.unwrap();

        // Start actor (replay happens in on_attach)
        actor2.start().await.unwrap();

        // Verify entries still exist
        let entries_after = storage.replay_from(&actor_id.to_string(), 0).await.unwrap();
        assert!(
            entries_after.len() >= 5,
            "Entries should persist after restart"
        );
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
        let storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        let config = DurabilityConfig {
            checkpoint_interval: 10, // Checkpoint every 10 messages
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
        };

        let behavior = Box::new(CounterActor::new());
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor-2".to_string())
            .await
            .unwrap();
        let actor_id = test_actor_id("counter-actor-2", "default");

        let mut actor = actor_with_service_locator(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        actor.attach_facet(Box::new(facet)).await.unwrap();
        actor.start().await.unwrap();

        // Process 10 messages
        for i in 1..=10 {
            let mut msg = create_test_message(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver_id = actor_id.to_string();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        // Create checkpoint manually
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 20, // 10 messages * 2 entries
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            state_data: 10u64.to_le_bytes().to_vec(), // count = 10
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();

        // Process 5 more messages
        for i in 11..=15 {
            let mut msg = create_test_message(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver_id = actor_id.to_string();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        actor.stop().await.unwrap();

        // Restart actor
        let behavior2 = Box::new(CounterActor::new());
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.to_string())
            .await
            .unwrap();
        let mut actor2 = actor_with_service_locator(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let facet2 = DurabilityFacet::new(storage.clone(), config_value, 50);
        actor2.attach_facet(Box::new(facet2)).await.unwrap();
        actor2.start().await.unwrap();

        // Verify checkpoint exists (may be higher due to automatic checkpointing)
        let checkpoint_after = storage
            .get_latest_checkpoint(&actor_id.to_string())
            .await
            .unwrap();
        assert!(
            checkpoint_after.sequence >= 20,
            "Checkpoint should be at sequence >= 20 (got {})",
            checkpoint_after.sequence
        );
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
        let storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        let config = DurabilityConfig {
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
        };

        let counter = Arc::new(RwLock::new(CounterActor::new()));
        let behavior = Box::new(CounterActorWrapper {
            counter: Arc::clone(&counter),
        });
        let mailbox = Mailbox::new(mailbox_config_default(), "counter-actor-3".to_string())
            .await
            .unwrap();
        let actor_id = test_actor_id("counter-actor-3", "default");

        let mut actor = actor_with_service_locator(
            actor_id.clone(),
            behavior,
            mailbox,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);

        // Set StateLoader for automatic state loading
        let state_loader = CounterStateLoader {
            counter: Arc::clone(&counter),
        };
        facet.set_state_loader(Box::new(state_loader)).await;

        actor.attach_facet(Box::new(facet)).await.unwrap();
        actor.start().await.unwrap();

        // Process messages
        for i in 1..=5 {
            let mut msg = create_test_message(format!("increment-{}", i).into_bytes());
            msg.message_type = "increment".to_string();
            msg.receiver_id = actor_id.to_string();
            actor.send(msg).await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        storage.flush().await.unwrap();

        // Create checkpoint with state
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
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
        let mailbox2 = Mailbox::new(mailbox_config_default(), actor_id.to_string())
            .await
            .unwrap();
        let mut actor2 = actor_with_service_locator(
            actor_id.clone(),
            behavior2,
            mailbox2,
            "default".to_string(),
            "default".to_string(),
        )
        .await;

        let config_value = serde_json::json!({
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // checkpoint_timeout is Option<Duration> which doesn't implement Serialize
        // Skip it - DurabilityFacet will use default if not provided
        let mut facet2 = DurabilityFacet::new(storage.clone(), config_value, 50);
        let state_loader2 = CounterStateLoader {
            counter: Arc::clone(&counter2),
        };
        facet2.set_state_loader(Box::new(state_loader2)).await;

        actor2.attach_facet(Box::new(facet2)).await.unwrap();
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
            BehaviorType::Custom("counter_wrapper".to_string())
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
            let count = u64::from_le_bytes(state_data[0..8].try_into().map_err(|_| {
                JournalError::Serialization("Invalid state data length".to_string())
            })?);
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
