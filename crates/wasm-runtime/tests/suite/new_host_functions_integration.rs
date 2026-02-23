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
    use plexspaces_keyvalue::SqliteKVStore;
    use plexspaces_process_groups::ProcessGroupRegistry;
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use std::sync::Arc;
    use std::collections::HashMap;
    use tokio::sync::RwLock;
    use std::time::Duration;
    use plexspaces_wasm_runtime::HostFunctions;

    /// Simple in-memory KeyValueStore for testing (implements plexspaces_core::KeyValueStore)
    struct TestMemoryKVStore {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl TestMemoryKVStore {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl plexspaces_core::KeyValueStore for TestMemoryKVStore {
        async fn get(&self, _ctx: &plexspaces_core::RequestContext, key: &str) -> plexspaces_core::KeyValueStoreResult<Option<Vec<u8>>> {
            Ok(self.data.read().await.get(key).cloned())
        }

        async fn put(&self, _ctx: &plexspaces_core::RequestContext, key: &str, value: Vec<u8>) -> plexspaces_core::KeyValueStoreResult<()> {
            self.data.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn put_with_ttl(&self, _ctx: &plexspaces_core::RequestContext, key: &str, value: Vec<u8>, _ttl: Duration) -> plexspaces_core::KeyValueStoreResult<()> {
            self.data.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn delete(&self, _ctx: &plexspaces_core::RequestContext, key: &str) -> plexspaces_core::KeyValueStoreResult<()> {
            self.data.write().await.remove(key);
            Ok(())
        }

        async fn exists(&self, _ctx: &plexspaces_core::RequestContext, key: &str) -> plexspaces_core::KeyValueStoreResult<bool> {
            Ok(self.data.read().await.contains_key(key))
        }

        async fn list_keys(&self, _ctx: &plexspaces_core::RequestContext, prefix: &str) -> plexspaces_core::KeyValueStoreResult<Vec<String>> {
            Ok(self.data.read().await.keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }

        async fn cas(&self, _ctx: &plexspaces_core::RequestContext, key: &str, expected: Option<Vec<u8>>, new_value: Vec<u8>) -> plexspaces_core::KeyValueStoreResult<bool> {
            let mut data = self.data.write().await;
            let current = data.get(key).cloned();
            if current == expected {
                data.insert(key.to_string(), new_value);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn increment(&self, _ctx: &plexspaces_core::RequestContext, key: &str, delta: i64) -> plexspaces_core::KeyValueStoreResult<i64> {
            let mut data = self.data.write().await;
            let current = data.get(key)
                .and_then(|v| String::from_utf8(v.clone()).ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let new_val = current + delta;
            data.insert(key.to_string(), new_val.to_string().into_bytes());
            Ok(new_val)
        }
    }

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            headers: vec![],
        }
    }

    async fn create_test_host_functions_with_services() -> Arc<HostFunctions> {
        // Create in-memory services for testing (using SQLite :memory: backends)
        // SqliteKVStore implements plexspaces_keyvalue::KeyValueStore (for ProcessGroupRegistry)
        // TestMemoryKVStore implements plexspaces_core::KeyValueStore (for HostFunctions)
        let kv_store_for_pg: Arc<dyn plexspaces_keyvalue::KeyValueStore> =
            Arc::new(SqliteKVStore::new(":memory:").await.unwrap());
        let kv_store_for_host: Arc<dyn plexspaces_core::KeyValueStore> = Arc::new(TestMemoryKVStore::new());
        let process_group_registry = Arc::new(ProcessGroupRegistry::new(
            "test-node".to_string(),
            kv_store_for_pg,
        ));
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));

        // Create default in-memory journal storage for testing
        use plexspaces_journaling::{JournalStorage, SqliteJournalStorage};
        let journal_storage: Arc<dyn JournalStorage + Send + Sync> = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        Arc::new(HostFunctions::with_all_services(
            None, // No message sender
            None, // No channel service
            Some(kv_store_for_host),
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        // actor-error is now a string (JSON), not a record
        assert!(error.contains("not configured"), "Error should mention not configured, got: {}", error);
    }

    #[tokio::test]
    async fn test_process_groups_impl_create_join_leave() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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

    /// Leader election: 2 actors, same lock (tenant/namespace/key), 2 different holder_ids.
    /// Must enforce single holder: first acquires, second fails until first releases.
    /// Uses same (tenant, namespace, lock_key) as simple_component_host for lock_id "leader".
    #[tokio::test]
    async fn test_leader_election_two_actors_same_lock() {
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let host_functions = Arc::new(HostFunctions::with_all_services(
            None,
            None,
            Some(Arc::new(TestMemoryKVStore::new())),
            Some(Arc::new(ProcessGroupRegistry::new(
                "test-node".to_string(),
                {
                    let kv: Arc<dyn plexspaces_keyvalue::KeyValueStore> =
                        Arc::new(SqliteKVStore::new(":memory:").await.unwrap());
                    kv
                },
            ))),
            Some(lock_manager),
            Some(Arc::new(ObjectRegistryImpl::new(
                Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap()),
            ))),
            Some(Arc::new(
                plexspaces_journaling::SqliteJournalStorage::new(":memory:").await.unwrap(),
            )),
            None,
        ));
        let ctx_leader = test_context("", "leader-election");
        let lock_key = "leader".to_string();
        let holder_term1 = "LeaderElection:leader-election-term1@test-node".to_string();
        let holder_term2 = "LeaderElection:leader-election-term2@test-node".to_string();

        let mut locks1 = LocksImpl {
            actor_id: ActorId::from(holder_term1.clone()),
            host_functions: host_functions.clone(),
        };
        let mut locks2 = LocksImpl {
            actor_id: ActorId::from(holder_term2.clone()),
            host_functions: host_functions.clone(),
        };

        let lease_ms = 30_000u64;

        // Term1 try_acquire -> must succeed
        let r1 = locks1.try_acquire(ctx_leader.clone(), lock_key.clone(), holder_term1.clone(), lease_ms).await.unwrap();
        let lock1 = r1.expect("term1 must acquire leader lock");
        assert!(lock1.locked);
        assert_eq!(lock1.holder_id, holder_term1);

        // Term2 try_acquire -> must fail (same lock, different holder)
        let r2 = locks2.try_acquire(ctx_leader.clone(), lock_key.clone(), holder_term2.clone(), lease_ms).await.unwrap();
        assert!(r2.is_none(), "term2 must NOT acquire while term1 holds lock");

        // Term1 renew -> success
        let renewed = locks1.renew(
            ctx_leader.clone(),
            lock_key.clone(),
            holder_term1.clone(),
            lock1.version,
            lease_ms,
        ).await.unwrap();
        assert!(!renewed.version.is_empty());

        // Term2 try_acquire again -> still none
        let r2b = locks2.try_acquire(ctx_leader.clone(), lock_key.clone(), holder_term2.clone(), lease_ms).await.unwrap();
        assert!(r2b.is_none());

        // Term1 release
        locks1.release(ctx_leader.clone(), lock_key.clone(), holder_term1.clone(), renewed.version, false).await.unwrap();

        // Term2 can now acquire
        let r2c = locks2.try_acquire(ctx_leader.clone(), lock_key.clone(), holder_term2.clone(), lease_ms).await.unwrap();
        let lock2 = r2c.expect("term2 must acquire after term1 release");
        assert!(lock2.locked);
        assert_eq!(lock2.holder_id, holder_term2);
    }

    #[tokio::test]
    async fn test_registry_impl_register_lookup() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
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
        let host_functions = create_test_host_functions_with_services().await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Register multiple objects (use non-empty tenant/namespace for proper key generation)
        let ctx = test_context("default", "default");
        registry.register(
            ctx.clone(),
            "actor1".to_string(),
            ObjectType::Actor,
            "http://test:8000".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        registry.register(
            ctx.clone(),
            "actor2".to_string(),
            ObjectType::Actor,
            "http://test:8001".to_string(),
            None,
            vec![],
            vec![],
        ).await.unwrap();

        // ACT: Discover all actors (use same context as registration)
        let result = registry.discover(
            ctx,
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
        assert!(objects.len() >= 2, "should find at least 2 actors, found {}", objects.len());
    }

    #[tokio::test]
    async fn test_registry_impl_heartbeat() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions_with_services().await;
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

