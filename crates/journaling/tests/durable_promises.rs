// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for durable promises

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;
    use std::time::Duration;

    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    /// Helper to convert DurabilityConfig to Value
    fn config_to_value(config: &DurabilityConfig) -> serde_json::Value {
        serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "checkpoint_timeout": config.checkpoint_timeout,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        })
    }

    #[tokio::test]
    async fn test_promise_creation_and_persistence() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-1";
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Create a promise
        let promise_id = "promise-1";
        let timeout = Some(Duration::from_secs(30));
        facet
            .create_promise(promise_id, timeout)
            .await
            .unwrap();

        // Flush to ensure entry is written
        storage.flush().await.unwrap();

        // Verify promise was journaled
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let promise_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseCreated(_))
                )
            })
            .collect();

        assert_eq!(promise_entries.len(), 1, "Should have one PromiseCreated entry");
        
        if let Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseCreated(
            promise_created,
        )) = &promise_entries[0].entry
        {
            assert_eq!(promise_created.promise_id, promise_id);
            assert!(promise_created.timeout.is_some(), "Timeout should be set");
        } else {
            panic!("Entry should be PromiseCreated");
        }
    }

    #[tokio::test]
    async fn test_promise_resolution_and_persistence() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-2";
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Create a promise
        let promise_id = "promise-2";
        facet
            .create_promise(promise_id, Some(Duration::from_secs(60)))
            .await
            .unwrap();

        // Resolve the promise
        let result = b"success".to_vec();
        facet
            .resolve_promise(promise_id, Ok(result.clone()))
            .await
            .unwrap();

        // Flush to ensure entries are written
        storage.flush().await.unwrap();

        // Verify both entries were journaled
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let promise_created: Vec<_> = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseCreated(_))
                )
            })
            .collect();
        let promise_resolved: Vec<_> = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseResolved(_))
                )
            })
            .collect();

        assert_eq!(promise_created.len(), 1, "Should have one PromiseCreated entry");
        assert_eq!(promise_resolved.len(), 1, "Should have one PromiseResolved entry");

        // Verify resolution details
        if let Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseResolved(
            promise_resolved_entry,
        )) = &promise_resolved[0].entry
        {
            assert_eq!(promise_resolved_entry.promise_id, promise_id);
            assert_eq!(promise_resolved_entry.result, result);
            assert!(promise_resolved_entry.error.is_empty(), "Error should be empty for success");
        } else {
            panic!("Entry should be PromiseResolved");
        }
    }

    #[tokio::test]
    async fn test_promise_recovery_after_restart() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-3";

        // Phase 1: Create promise and detach
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let promise_id = "promise-3";
        facet
            .create_promise(promise_id, Some(Duration::from_secs(30)))
            .await
            .unwrap();

        storage.flush().await.unwrap();
        facet.on_detach(actor_id).await.unwrap();

        // Phase 2: Restart and verify promise can be queried
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Query for pending promises (should find the one we created)
        let pending_promises = new_facet.get_pending_promises().await.unwrap();
        assert_eq!(pending_promises.len(), 1, "Should have one pending promise");
        assert_eq!(pending_promises[0], promise_id);
    }

    #[tokio::test]
    async fn test_promise_completion_during_replay() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-4";

        // Phase 1: Create and resolve promise, then detach
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let promise_id = "promise-4";
        facet
            .create_promise(promise_id, Some(Duration::from_secs(30)))
            .await
            .unwrap();

        let result = b"completed".to_vec();
        facet
            .resolve_promise(promise_id, Ok(result.clone()))
            .await
            .unwrap();

        storage.flush().await.unwrap();
        facet.on_detach(actor_id).await.unwrap();

        // Phase 2: Restart and verify promise is marked as resolved
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // After replay, completed promises should not be in pending list
        let pending_promises = new_facet.get_pending_promises().await.unwrap();
        assert!(
            !pending_promises.contains(&promise_id.to_string()),
            "Resolved promise should not be in pending list"
        );

        // Verify we can query the promise result
        let promise_result = new_facet.get_promise_result(promise_id).await.unwrap();
        assert!(promise_result.is_some(), "Promise result should be available");
        if let Some((Ok(resolved_result), _)) = promise_result {
            assert_eq!(resolved_result, result);
        } else {
            panic!("Promise should be resolved with success");
        }
    }

    #[tokio::test]
    async fn test_promise_timeout_handling() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-5";
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Create a promise with timeout
        let promise_id = "promise-5";
        facet
            .create_promise(promise_id, Some(Duration::from_secs(10)))
            .await
            .unwrap();

        // Resolve with timeout error
        facet
            .resolve_promise(promise_id, Err("timeout".to_string()))
            .await
            .unwrap();

        storage.flush().await.unwrap();

        // Verify timeout was journaled
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let promise_resolved: Vec<_> = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseResolved(_))
                )
            })
            .collect();

        assert_eq!(promise_resolved.len(), 1, "Should have one PromiseResolved entry");

        if let Some(plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseResolved(
            promise_resolved_entry,
        )) = &promise_resolved[0].entry
        {
            assert_eq!(promise_resolved_entry.promise_id, promise_id);
            assert_eq!(promise_resolved_entry.error, "timeout");
            assert!(promise_resolved_entry.result.is_empty(), "Result should be empty for error");
        } else {
            panic!("Entry should be PromiseResolved");
        }
    }

    #[tokio::test]
    async fn test_multiple_promises_isolation() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-6";
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Create multiple promises
        let promise1 = "promise-1";
        let promise2 = "promise-2";
        let promise3 = "promise-3";

        facet
            .create_promise(promise1, Some(Duration::from_secs(30)))
            .await
            .unwrap();
        facet
            .create_promise(promise2, Some(Duration::from_secs(60)))
            .await
            .unwrap();
        facet
            .create_promise(promise3, Some(Duration::from_secs(90)))
            .await
            .unwrap();

        // Resolve one promise
        facet
            .resolve_promise(promise2, Ok(b"resolved".to_vec()))
            .await
            .unwrap();

        storage.flush().await.unwrap();
        facet.on_detach(actor_id).await.unwrap();

        // Restart and verify state
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let pending_promises = new_facet.get_pending_promises().await.unwrap();
        assert_eq!(pending_promises.len(), 2, "Should have 2 pending promises");
        assert!(pending_promises.contains(&promise1.to_string()));
        assert!(pending_promises.contains(&promise3.to_string()));
        assert!(!pending_promises.contains(&promise2.to_string()), "Resolved promise should not be pending");

        // Verify resolved promise result
        let result = new_facet.get_promise_result(promise2).await.unwrap();
        assert!(result.is_some(), "Resolved promise should have result");
    }
}

