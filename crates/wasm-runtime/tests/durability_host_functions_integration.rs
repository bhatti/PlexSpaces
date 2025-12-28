// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for WASM durability host functions
// Validates that all durability functions work correctly with JournalStorage
//
// NOTE: These tests are designed to run offline without network access or SSL.
// All tests use MemoryJournalStorage (in-memory) and do not require external services.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::DurabilityImpl;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::durability::Host;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::types as actor_types;
    use plexspaces_core::ActorId;
    use plexspaces_journaling::{JournalStorage, MemoryJournalStorage};
    use std::sync::Arc;
    use plexspaces_wasm_runtime::HostFunctions;

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> actor_types::Context {
        actor_types::Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    fn create_test_host_functions_with_journal() -> Arc<HostFunctions> {
        let journal_storage: Arc<dyn JournalStorage> = Arc::new(MemoryJournalStorage::new());
        
        Arc::new(HostFunctions::with_all_services(
            None, // No message sender
            None, // No channel service
            None, // No keyvalue store
            None, // No process group registry
            None, // No lock manager
            None, // No object registry
            Some(journal_storage),
            None, // No blob service
        ))
    }

    #[tokio::test]
    async fn test_durability_impl_persist() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist an event
        let result = durability.persist(
            test_context("", ""),
            "test.event".to_string(),
            b"test-payload".to_vec(),
        ).await;

        // ASSERT: Should succeed and return sequence number
        assert!(result.is_ok(), "persist should succeed");
        let sequence = result.unwrap();
        assert_eq!(sequence, 1, "First event should have sequence 1");
    }

    #[tokio::test]
    async fn test_durability_impl_persist_batch() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist batch of events
        let events = vec![
            ("event1".to_string(), b"payload1".to_vec()),
            ("event2".to_string(), b"payload2".to_vec()),
            ("event3".to_string(), b"payload3".to_vec()),
        ];
        let result = durability.persist_batch(test_context("", ""), events).await;

        // ASSERT: Should succeed and return first sequence
        // MemoryJournalStorage assigns sequences starting from 1 for the first event
        assert!(result.is_ok(), "persist_batch should succeed");
        let first_sequence = result.unwrap();
        assert_eq!(first_sequence, 1, "First event in batch should have sequence 1, got {}", first_sequence);
    }

    #[tokio::test]
    async fn test_durability_impl_get_sequence() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist some events
        durability.persist(test_context("", ""), "event1".to_string(), b"payload1".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event2".to_string(), b"payload2".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event3".to_string(), b"payload3".to_vec()).await.unwrap();

        // ACT: Get sequence
        let result = durability.get_sequence(test_context("", "")).await;

        // ASSERT: Should return highest sequence
        assert!(result.is_ok(), "get_sequence should succeed");
        let sequence = result.unwrap();
        assert_eq!(sequence, 3, "Sequence should be 3 after 3 events");
    }

    #[tokio::test]
    async fn test_durability_impl_checkpoint() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist some events
        durability.persist(test_context("", ""), "event1".to_string(), b"payload1".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event2".to_string(), b"payload2".to_vec()).await.unwrap();

        // ACT: Create checkpoint
        let result = durability.checkpoint(test_context("", "")).await;

        // ASSERT: Should succeed and return sequence
        assert!(result.is_ok(), "checkpoint should succeed");
        let checkpoint_sequence = result.unwrap();
        assert_eq!(checkpoint_sequence, 2, "Checkpoint should be at sequence 2");
    }

    #[tokio::test]
    async fn test_durability_impl_get_checkpoint_sequence() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist events and create checkpoint
        durability.persist(test_context("", ""), "event1".to_string(), b"payload1".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event2".to_string(), b"payload2".to_vec()).await.unwrap();
        durability.checkpoint(test_context("", "")).await.unwrap();

        // ACT: Get checkpoint sequence
        let result = durability.get_checkpoint_sequence(test_context("", "")).await;

        // ASSERT: Should return checkpoint sequence
        assert!(result.is_ok(), "get_checkpoint_sequence should succeed");
        let checkpoint_sequence = result.unwrap();
        assert_eq!(checkpoint_sequence, 2, "Checkpoint sequence should be 2");
    }

    #[tokio::test]
    async fn test_durability_impl_is_replaying() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Check replay mode (should be false by default)
        let result = durability.is_replaying(test_context("", "")).await;

        // ASSERT: Should return false
        assert!(result.is_ok(), "is_replaying should succeed");
        assert_eq!(result.unwrap(), false, "Should not be in replay mode by default");
    }

    #[tokio::test]
    async fn test_durability_impl_cache_side_effect() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Cache a side effect
        let result = durability.cache_side_effect(
            test_context("", ""),
            "test-key".to_string(),
            b"test-result".to_vec(),
        ).await;

        // ASSERT: Should succeed and return the value
        assert!(result.is_ok(), "cache_side_effect should succeed");
        let cached_value = result.unwrap();
        assert_eq!(cached_value, b"test-result".to_vec(), "Should return cached value");
    }

    #[tokio::test]
    async fn test_durability_impl_read_journal() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist some events
        durability.persist(test_context("", ""), "event1".to_string(), b"payload1".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event2".to_string(), b"payload2".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event3".to_string(), b"payload3".to_vec()).await.unwrap();

        // ACT: Read journal
        let result = durability.read_journal(test_context("", ""), 1, 0, 10).await;

        // ASSERT: Should succeed and return entries
        assert!(result.is_ok(), "read_journal should succeed");
        let entries = result.unwrap();
        assert_eq!(entries.len(), 3, "Should return 3 entries");
        assert_eq!(entries[0].sequence, 1, "First entry should have sequence 1");
        assert_eq!(entries[0].event_type, "event1", "First entry should have event_type 'event1'");
    }

    #[tokio::test]
    async fn test_durability_impl_compact() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_journal();
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Persist events and create checkpoint
        durability.persist(test_context("", ""), "event1".to_string(), b"payload1".to_vec()).await.unwrap();
        durability.persist(test_context("", ""), "event2".to_string(), b"payload2".to_vec()).await.unwrap();
        durability.checkpoint(test_context("", "")).await.unwrap();

        // ACT: Compact journal
        let result = durability.compact(test_context("", ""), 2).await;

        // ASSERT: Should succeed
        assert!(result.is_ok(), "compact should succeed");
    }

    #[tokio::test]
    async fn test_durability_impl_without_storage() {
        // ARRANGE: Create host functions without journal storage
        let host_functions = Arc::new(HostFunctions::new());
        let actor_id = ActorId::from("test-actor".to_string());
        let mut durability = DurabilityImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Try to persist (should fail)
        let result = durability.persist(
            test_context("", ""),
            "test.event".to_string(),
            b"test-payload".to_vec(),
        ).await;

        // ASSERT: Should return error
        assert!(result.is_err(), "persist should fail without storage");
        let error = result.unwrap_err();
        assert_eq!(
            error.code,
            actor_types::ErrorCode::Internal,
            "Error code should be Internal"
        );
        assert!(
            error.message.contains("Journal storage not configured"),
            "Error message should mention storage not configured"
        );
    }
}

