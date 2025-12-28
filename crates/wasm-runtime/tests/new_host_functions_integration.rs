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

//! Integration tests for new WASM host functions (KeyValue, ProcessGroups, Locks, Registry)
//! Validates that all new abstractions are available via WASM and working correctly
//!
//! NOTE: These tests are designed to run offline without network access or SSL.
//! All tests use in-memory services (InMemoryKVStore, MemoryLockManager, MemoryJournalStorage)
//! and do not require external services or network connectivity.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::{
        KeyValueImpl, ProcessGroupsImpl, LocksImpl, RegistryImpl,
    };
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::{
        keyvalue::Host as KeyValueHost,
        process_groups::Host as ProcessGroupsHost,
        locks::Host as LocksHost,
        registry::{Host as RegistryHost, ObjectType, Label},
        types::Context,
    };
    use plexspaces_core::ActorId;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use plexspaces_locks::memory::MemoryLockManager;
    use plexspaces_object_registry::ObjectRegistry;
    use std::sync::Arc;
    use plexspaces_wasm_runtime::HostFunctions;

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    fn create_test_host_functions_with_services() -> Arc<HostFunctions> {
        // Create in-memory services for testing
        let kv_store = Arc::new(InMemoryKVStore::new());
        let process_group_registry = Arc::new(ProcessGroupRegistry::new(
            "test-node".to_string(),
            kv_store.clone(),
        ));
        let lock_manager = Arc::new(MemoryLockManager::new());
        let object_registry = Arc::new(ObjectRegistry::new(kv_store.clone()));

        // Create default in-memory journal storage for testing
        use plexspaces_journaling::{JournalStorage, MemoryJournalStorage};
        let journal_storage: Arc<dyn JournalStorage> = Arc::new(MemoryJournalStorage::new());

        Arc::new(HostFunctions::with_all_services(
            None, // No message sender
            None, // No channel service
            Some(kv_store),
            Some(process_group_registry),
            Some(lock_manager),
            Some(object_registry),
            Some(journal_storage),
            None, // No blob service
        ))
    }

    #[tokio::test]
    async fn test_keyvalue_impl_get_put() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Put a value
        let result = kv.put(test_context("", ""), "test-key".to_string(), b"test-value".to_vec()).await;
        assert!(result.is_ok(), "put should succeed");

        // ACT: Get the value
        let result = kv.get(test_context("", ""), "test-key".to_string()).await;
        assert!(result.is_ok(), "get should succeed");
        let value = result.unwrap();
        assert_eq!(value, Some(b"test-value".to_vec()));
    }

    #[tokio::test]
    async fn test_keyvalue_impl_delete_exists() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Put a value
        kv.put(test_context("", ""), "test-key".to_string(), b"test-value".to_vec()).await.unwrap();

        // ACT: Check exists
        let result = kv.exists(test_context("", ""), "test-key".to_string()).await;
        assert!(result.is_ok(), "exists should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: Delete the value
        let result = kv.delete(test_context("", ""), "test-key".to_string()).await;
        assert!(result.is_ok(), "delete should succeed");

        // ACT: Check exists again
        let result = kv.exists(test_context("", ""), "test-key".to_string()).await;
        assert!(result.is_ok(), "exists should succeed");
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_increment() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Increment (creates key with 0 if not exists)
        let result = kv.increment(test_context("", ""), "counter".to_string(), 5).await;
        assert!(result.is_ok(), "increment should succeed");
        assert_eq!(result.unwrap(), 5);

        // ACT: Increment again
        let result = kv.increment(test_context("", ""), "counter".to_string(), 3).await;
        assert!(result.is_ok(), "increment should succeed");
        assert_eq!(result.unwrap(), 8);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_compare_and_swap() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: CAS with None (key must not exist)
        let result = kv.compare_and_swap(
            test_context("", ""),
            "cas-key".to_string(),
            None,
            b"new-value".to_vec(),
        ).await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: CAS with Some (key must equal expected)
        let result = kv.compare_and_swap(
            test_context("", ""),
            "cas-key".to_string(),
            Some(b"new-value".to_vec()),
            b"updated-value".to_vec(),
        ).await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: CAS with wrong expected value (should fail)
        let result = kv.compare_and_swap(
            test_context("", ""),
            "cas-key".to_string(),
            Some(b"wrong-value".to_vec()),
            b"another-value".to_vec(),
        ).await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_without_service() {
        // ARRANGE: HostFunctions without KeyValueStore
        let host_functions = Arc::new(HostFunctions::new());
        let actor_id = ActorId::from("test-actor".to_string());
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions,
        };

        // ACT: Try to get (should fail with Internal error)
        let result = kv.get(test_context("", ""), "test-key".to_string()).await;
        assert!(result.is_err(), "get should fail when KeyValueStore not configured");
        let error = result.unwrap_err();
        assert!(error.message.contains("not configured"));
    }

    #[tokio::test]
    async fn test_process_groups_impl_create_join_leave() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut pg = ProcessGroupsImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Create group
        // Use "test" namespace which is in the search list for internal context
        let result = pg.create_group(test_context("", "test"), "test-group".to_string(), "test".to_string()).await;
        assert!(result.is_ok(), "create_group should succeed");

        // ACT: Join group
        let result = pg.join_group(
            test_context("", "test"),
            "test-group".to_string(),
            "test".to_string(),
            vec!["topic1".to_string(), "topic2".to_string()],
        ).await;
        assert!(result.is_ok(), "join_group should succeed");

        // ACT: Get members
        // get_members uses RequestContext::internal() which searches across namespaces
        // including "test", so the group should be found
        let result = pg.get_members(test_context("", "test"), "test-group".to_string()).await;
        assert!(result.is_ok(), "get_members should succeed");
        let members = result.unwrap();
        // Convert ActorId to string for comparison
        let actor_id_str = actor_id.to_string();
        let member_strings: Vec<String> = members.iter().map(|id| id.to_string()).collect();
        assert!(member_strings.contains(&actor_id_str), "actor should be in group");

        // ACT: Leave group
        let result = pg.leave_group(test_context("", "test"), "test-group".to_string()).await;
        assert!(result.is_ok(), "leave_group should succeed");
    }

    #[tokio::test]
    async fn test_process_groups_impl_publish_to_group() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut pg = ProcessGroupsImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Create group and join
        // Use "test" namespace which is in the search list for internal context
        pg.create_group(test_context("", "test"), "test-group".to_string(), "test".to_string()).await.unwrap();
        pg.join_group(test_context("", "test"), "test-group".to_string(), "test".to_string(), vec![]).await.unwrap();

        // ACT: Publish to group
        let result = pg.publish_to_group(
            test_context("", "test"),
            "test-group".to_string(),
            Some("topic1".to_string()),
            b"test-message".to_vec(),
        ).await;
        assert!(result.is_ok(), "publish_to_group should succeed");
        let recipients = result.unwrap();
        // Convert ActorId to string for comparison
        let actor_id_str = actor_id.to_string();
        let recipient_strings: Vec<String> = recipients.iter().map(|id| id.to_string()).collect();
        // Actor should receive message since it joined with empty topics list (receives all)
        assert!(recipient_strings.contains(&actor_id_str), "actor should receive message");
    }

    #[tokio::test]
    async fn test_locks_impl_acquire_release() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Acquire lock
        let result = locks.acquire(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-1".to_string(),
            30000, // 30 seconds
        ).await;
        assert!(result.is_ok(), "acquire should succeed");
        let lock = result.unwrap();
        assert_eq!(lock.lock_key, "test-lock");
        assert_eq!(lock.holder_id, "holder-1");
        assert!(lock.locked);

        // ACT: Release lock
        let result = locks.release(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-1".to_string(),
            lock.version,
            false, // Don't delete
        ).await;
        assert!(result.is_ok(), "release should succeed");
    }

    #[tokio::test]
    async fn test_locks_impl_renew() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Acquire lock
        let lock = locks.acquire(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-1".to_string(),
            30000,
        ).await.unwrap();

        // ACT: Renew lock
        let result = locks.renew(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-1".to_string(),
            lock.version.clone(),
            60000, // 60 seconds
        ).await;
        assert!(result.is_ok(), "renew should succeed");
        let renewed = result.unwrap();
        assert_ne!(renewed.version, lock.version, "version should change after renew");
    }

    #[tokio::test]
    async fn test_locks_impl_try_acquire() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Try acquire (should succeed)
        let result = locks.try_acquire(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-1".to_string(),
            30000,
        ).await;
        assert!(result.is_ok(), "try_acquire should succeed");
        assert!(result.unwrap().is_some(), "lock should be acquired");

        // ACT: Try acquire again (should return None - lock already held)
        let result = locks.try_acquire(
            test_context("", ""),
            "test-lock".to_string(),
            "holder-2".to_string(),
            30000,
        ).await;
        assert!(result.is_ok(), "try_acquire should succeed");
        assert!(result.unwrap().is_none(), "lock should not be acquired (already held)");
    }

    #[tokio::test]
    async fn test_registry_impl_register_lookup() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Register object
        let result = registry.register(
            test_context("", ""),
            "test-object".to_string(),
            ObjectType::Actor,
            "http://test:8000".to_string(),
            Some("GenServer".to_string()),
            vec!["persistent".to_string()],
            vec![
                Label {
                    key: "env".to_string(),
                    value: "test".to_string(),
                },
            ],
        ).await;
        assert!(result.is_ok(), "register should succeed");

        // ACT: Lookup object
        let result = registry.lookup(
            test_context("", ""),
            ObjectType::Actor,
            "test-object".to_string(),
        ).await;
        assert!(result.is_ok(), "lookup should succeed");
        let registration = result.unwrap();
        assert!(registration.is_some(), "object should be found");
        let reg = registration.unwrap();
        assert_eq!(reg.object_id, "test-object");
        assert_eq!(reg.grpc_address, "http://test:8000");
    }

    #[tokio::test]
    async fn test_registry_impl_unregister() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Register then unregister
        registry.register(
            test_context("", ""),
            "test-object".to_string(),
            ObjectType::Actor,
            "http://test:8000".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        let result = registry.unregister(
            test_context("", ""),
            ObjectType::Actor,
            "test-object".to_string(),
        ).await;
        assert!(result.is_ok(), "unregister should succeed");

        // ACT: Lookup should return None
        let result = registry.lookup(
            test_context("", ""),
            ObjectType::Actor,
            "test-object".to_string(),
        ).await;
        assert!(result.is_ok(), "lookup should succeed");
        assert!(result.unwrap().is_none(), "object should not be found after unregister");
    }

    #[tokio::test]
    async fn test_registry_impl_discover() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Register multiple objects
        registry.register(
            test_context("", ""),
            "actor1".to_string(),
            ObjectType::Actor,
            "http://test:8000".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        registry.register(
            test_context("", ""),
            "actor2".to_string(),
            ObjectType::Actor,
            "http://test:8001".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        // ACT: Discover all actors
        let result = registry.discover(
            test_context("", ""),
            Some(ObjectType::Actor),
            None,
            vec![],
            vec![],
            None,
            0,
            100,
        ).await;
        assert!(result.is_ok(), "discover should succeed");
        let objects = result.unwrap();
        assert!(objects.len() >= 2, "should find at least 2 actors");
    }

    #[tokio::test]
    async fn test_registry_impl_heartbeat() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services();
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Register object
        registry.register(
            test_context("", ""),
            "test-object".to_string(),
            ObjectType::Actor,
            "http://test:8000".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        // ACT: Send heartbeat
        let result = registry.heartbeat(
            test_context("", ""),
            ObjectType::Actor,
            "test-object".to_string(),
        ).await;
        assert!(result.is_ok(), "heartbeat should succeed");
    }
}

