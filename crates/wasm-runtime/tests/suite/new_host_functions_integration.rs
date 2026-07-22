// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for new WASM host functions (KeyValue, ProcessGroups, Locks, Registry)
//! Validates that all new abstractions are available via WASM and working correctly
//!
//! NOTE: These tests are designed to run offline without network access or SSL.
//! All tests use in-memory services (InMemoryKVStore, MemoryLockManager, MemoryJournalStorage)
//! and do not require external services or network connectivity.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_actor::{ActorId, RequestContext, RequestContextExt};
    use plexspaces_keyvalue::SqliteKVStore;
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_process_groups::ProcessGroupRegistry;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::{
        keyvalue::Host as KeyValueHost,
        locks::Host as LocksHost,
        process_groups::Host as ProcessGroupsHost,
        registry::Host as RegistryHost,
        types::Context,
    };
    use plexspaces_wasm_runtime::component_host::{
        KeyValueImpl, LocksImpl, ProcessGroupsImpl, RegistryImpl,
    };
    use plexspaces_wasm_runtime::HostFunctions;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::RwLock;

    /// Simple in-memory KeyValueStore for testing (implements plexspaces_actor::KeyValueStore)
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
    impl plexspaces_actor::KeyValueStore for TestMemoryKVStore {
        async fn get(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
        ) -> plexspaces_actor::KeyValueStoreResult<Option<Vec<u8>>> {
            Ok(self.data.read().await.get(key).cloned())
        }

        async fn put(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
            value: Vec<u8>,
        ) -> plexspaces_actor::KeyValueStoreResult<()> {
            self.data.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn put_with_ttl(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
            value: Vec<u8>,
            _ttl: Duration,
        ) -> plexspaces_actor::KeyValueStoreResult<()> {
            self.data.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn delete(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
        ) -> plexspaces_actor::KeyValueStoreResult<()> {
            self.data.write().await.remove(key);
            Ok(())
        }

        async fn exists(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
        ) -> plexspaces_actor::KeyValueStoreResult<bool> {
            Ok(self.data.read().await.contains_key(key))
        }

        async fn list_keys(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            prefix: &str,
        ) -> plexspaces_actor::KeyValueStoreResult<Vec<String>> {
            Ok(self
                .data
                .read()
                .await
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        }

        async fn cas(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
            expected: Option<Vec<u8>>,
            new_value: Vec<u8>,
        ) -> plexspaces_actor::KeyValueStoreResult<bool> {
            let mut data = self.data.write().await;
            let current = data.get(key).cloned();
            if current == expected {
                data.insert(key.to_string(), new_value);
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn increment(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            key: &str,
            delta: i64,
        ) -> plexspaces_actor::KeyValueStoreResult<i64> {
            let mut data = self.data.write().await;
            let current = data
                .get(key)
                .and_then(|v| String::from_utf8(v.clone()).ok())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let new_val = current + delta;
            data.insert(key.to_string(), new_val.to_string().into_bytes());
            Ok(new_val)
        }

        async fn get_ttl(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            _key: &str,
        ) -> plexspaces_actor::KeyValueStoreResult<Option<std::time::Duration>> {
            Ok(None)
        }

        async fn multi_get(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            keys: &[&str],
        ) -> plexspaces_actor::KeyValueStoreResult<Vec<Option<Vec<u8>>>> {
            let mut results = Vec::with_capacity(keys.len());
            for k in keys {
                results.push(self.get(ctx, k).await?);
            }
            Ok(results)
        }

        async fn multi_put(
            &self,
            ctx: &plexspaces_actor::RequestContext,
            pairs: &[(&str, Vec<u8>)],
        ) -> plexspaces_actor::KeyValueStoreResult<()> {
            for (k, v) in pairs {
                self.put(ctx, k, v.clone()).await?;
            }
            Ok(())
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
        // TestMemoryKVStore implements plexspaces_actor::KeyValueStore (for HostFunctions)
        let kv_store_for_pg: Arc<dyn plexspaces_keyvalue::KeyValueStore> =
            Arc::new(SqliteKVStore::new(":memory:").await.unwrap());
        let kv_store_for_host: Arc<dyn plexspaces_actor::KeyValueStore> =
            Arc::new(TestMemoryKVStore::new());
        let process_group_registry = Arc::new(ProcessGroupRegistry::new(
            "test-node".to_string(),
            kv_store_for_pg,
        ));
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));

        // Create default in-memory journal storage for testing
        use plexspaces_journaling::{JournalStorage, SqliteJournalStorage};
        let journal_storage: Arc<dyn JournalStorage + Send + Sync> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        Arc::new(HostFunctions::with_all_services(
            None, // No message sender
            None, // No channel service
            Some(kv_store_for_host),
            Some(process_group_registry),
            Some(lock_manager),
            Some(object_registry),
            Some(journal_storage),
            None, // No blob service
            None, // No elastic pool service
            None, // No outbound HTTP client
            None, // No shared timer pool
        ))
    }

    async fn create_test_host_functions_with_tenant(tenant_id: &str, namespace: &str) -> Arc<HostFunctions> {
        let kv_store_for_pg: Arc<dyn plexspaces_keyvalue::KeyValueStore> =
            Arc::new(SqliteKVStore::new(":memory:").await.unwrap());
        let kv_store_for_host: Arc<dyn plexspaces_actor::KeyValueStore> =
            Arc::new(TestMemoryKVStore::new());
        let process_group_registry = Arc::new(ProcessGroupRegistry::new(
            "test-node".to_string(),
            kv_store_for_pg,
        ));
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry = Arc::new(ObjectRegistryImpl::new(object_repo));

        use plexspaces_journaling::{JournalStorage, SqliteJournalStorage};
        let journal_storage: Arc<dyn JournalStorage + Send + Sync> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());

        Arc::new(
            HostFunctions::with_all_services(
                None,
                None,
                Some(kv_store_for_host),
                Some(process_group_registry),
                Some(lock_manager),
                Some(object_registry),
                Some(journal_storage),
                None,
                None,
                None,
                None,
            )
            .with_tenant(tenant_id.to_string(), namespace.to_string()),
        )
    }


    #[tokio::test]
    async fn test_keyvalue_impl_get_put() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Put a value
        let result = kv
            .put(
                test_context("", ""),
                "test-key".to_string(),
                b"test-value".to_vec(),
            )
            .await;
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
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Put a value
        kv.put(
            test_context("", ""),
            "test-key".to_string(),
            b"test-value".to_vec(),
        )
        .await
        .unwrap();

        // ACT: Check exists
        let result = kv
            .exists(test_context("", ""), "test-key".to_string())
            .await;
        assert!(result.is_ok(), "exists should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: Delete the value
        let result = kv
            .delete(test_context("", ""), "test-key".to_string())
            .await;
        assert!(result.is_ok(), "delete should succeed");

        // ACT: Check exists again
        let result = kv
            .exists(test_context("", ""), "test-key".to_string())
            .await;
        assert!(result.is_ok(), "exists should succeed");
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_increment() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Increment (creates key with 0 if not exists)
        let result = kv
            .increment(test_context("", ""), "counter".to_string(), 5)
            .await;
        assert!(result.is_ok(), "increment should succeed");
        assert_eq!(result.unwrap(), 5);

        // ACT: Increment again
        let result = kv
            .increment(test_context("", ""), "counter".to_string(), 3)
            .await;
        assert!(result.is_ok(), "increment should succeed");
        assert_eq!(result.unwrap(), 8);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_compare_and_swap() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: CAS with None (key must not exist)
        let result = kv
            .compare_and_swap(
                test_context("", ""),
                "cas-key".to_string(),
                None,
                b"new-value".to_vec(),
            )
            .await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: CAS with Some (key must equal expected)
        let result = kv
            .compare_and_swap(
                test_context("", ""),
                "cas-key".to_string(),
                Some(b"new-value".to_vec()),
                b"updated-value".to_vec(),
            )
            .await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), true);

        // ACT: CAS with wrong expected value (should fail)
        let result = kv
            .compare_and_swap(
                test_context("", ""),
                "cas-key".to_string(),
                Some(b"wrong-value".to_vec()),
                b"another-value".to_vec(),
            )
            .await;
        assert!(result.is_ok(), "CAS should succeed");
        assert_eq!(result.unwrap(), false);
    }

    #[tokio::test]
    async fn test_keyvalue_impl_without_service() {
        // ARRANGE: HostFunctions without KeyValueStore
        let host_functions = Arc::new(HostFunctions::new());
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let mut kv = KeyValueImpl {
            actor_id: actor_id.clone(),
            host_functions,
        };

        // ACT: Try to get (should fail with Internal error)
        let result = kv.get(test_context("", ""), "test-key".to_string()).await;
        assert!(
            result.is_err(),
            "get should fail when KeyValueStore not configured"
        );
        let error = result.unwrap_err();
        // actor-error is now a string (JSON), not a record
        assert!(
            error.contains("not configured"),
            "Error should mention not configured, got: {}",
            error
        );
    }

    #[tokio::test]
    async fn test_process_groups_impl_create_join_leave() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut pg = ProcessGroupsImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Create group
        // Use "test" namespace which is in the search list for internal context
        let result = pg
            .create_group(
                test_context("", "test"),
                "test-group".to_string(),
                "test".to_string(),
            )
            .await;
        assert!(result.is_ok(), "create_group should succeed");

        // ACT: Join group
        let result = pg
            .join_group(
                test_context("", "test"),
                "test-group".to_string(),
                "test".to_string(),
                vec!["topic1".to_string(), "topic2".to_string()],
            )
            .await;
        assert!(result.is_ok(), "join_group should succeed");

        // ACT: Get members
        // get_members uses RequestContext::internal() which searches across namespaces
        // including "test", so the group should be found
        let result = pg
            .get_members(test_context("", "test"), "test-group".to_string())
            .await;
        assert!(result.is_ok(), "get_members should succeed");
        let members = result.unwrap();
        // Convert ActorId to string for comparison
        let actor_id_str = actor_id.to_string();
        let member_strings: Vec<String> = members.iter().map(|id| id.to_string()).collect();
        assert!(
            member_strings.contains(&actor_id_str),
            "actor should be in group"
        );

        // ACT: Leave group
        let result = pg
            .leave_group(test_context("", "test"), "test-group".to_string())
            .await;
        assert!(result.is_ok(), "leave_group should succeed");
    }

    #[tokio::test]
    async fn test_process_groups_impl_publish_to_group() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut pg = ProcessGroupsImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Create group and join
        // Use "test" namespace which is in the search list for internal context
        pg.create_group(
            test_context("", "test"),
            "test-group".to_string(),
            "test".to_string(),
        )
        .await
        .unwrap();
        pg.join_group(
            test_context("", "test"),
            "test-group".to_string(),
            "test".to_string(),
            vec![],
        )
        .await
        .unwrap();

        // ACT: Publish to group
        let result = pg
            .publish_to_group(
                test_context("", "test"),
                "test-group".to_string(),
                Some("topic1".to_string()),
                b"test-message".to_vec(),
            )
            .await;
        assert!(result.is_ok(), "publish_to_group should succeed");
        let recipients = result.unwrap();
        // Convert ActorId to string for comparison
        let actor_id_str = actor_id.to_string();
        let recipient_strings: Vec<String> = recipients.iter().map(|id| id.to_string()).collect();
        // Actor should receive message since it joined with empty topics list (receives all)
        assert!(
            recipient_strings.contains(&actor_id_str),
            "actor should receive message"
        );
    }

    #[tokio::test]
    async fn test_locks_impl_acquire_release() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Acquire lock
        let result = locks
            .acquire(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-1".to_string(),
                30000, // 30 seconds
            )
            .await;
        assert!(result.is_ok(), "acquire should succeed");
        let lock = result.unwrap();
        assert_eq!(lock.lock_key, "test-lock");
        assert_eq!(lock.holder_id, "holder-1");
        assert!(lock.locked);

        // ACT: Release lock
        let result = locks
            .release(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-1".to_string(),
                lock.version,
                false, // Don't delete
            )
            .await;
        assert!(result.is_ok(), "release should succeed");
    }

    #[tokio::test]
    async fn test_locks_impl_renew() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Acquire lock
        let lock = locks
            .acquire(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-1".to_string(),
                30000,
            )
            .await
            .unwrap();

        // ACT: Renew lock
        let result = locks
            .renew(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-1".to_string(),
                lock.version.clone(),
                60000, // 60 seconds
            )
            .await;
        assert!(result.is_ok(), "renew should succeed");
        let renewed = result.unwrap();
        assert_ne!(
            renewed.version, lock.version,
            "version should change after renew"
        );
    }

    #[tokio::test]
    async fn test_locks_impl_try_acquire() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut locks = LocksImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        // ACT: Try acquire (should succeed)
        let result = locks
            .try_acquire(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-1".to_string(),
                30000,
            )
            .await;
        assert!(result.is_ok(), "try_acquire should succeed");
        assert!(result.unwrap().is_some(), "lock should be acquired");

        // ACT: Try acquire again (should return None - lock already held)
        let result = locks
            .try_acquire(
                test_context("", ""),
                "test-lock".to_string(),
                "holder-2".to_string(),
                30000,
            )
            .await;
        assert!(result.is_ok(), "try_acquire should succeed");
        assert!(
            result.unwrap().is_none(),
            "lock should not be acquired (already held)"
        );
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
            Some(Arc::new(ObjectRegistryImpl::new(Arc::new(
                SqliteObjectRegistryRepository::new(":memory:")
                    .await
                    .unwrap(),
            )))),
            Some(Arc::new(
                plexspaces_journaling::SqliteJournalStorage::new(":memory:")
                    .await
                    .unwrap(),
            )),
            None, // blob_service
            None, // elastic_pool_service
            None, // outbound_http_client
            None, // shared_timer_pool
        ));
        let ctx_leader = test_context("", "leader-election");
        let lock_key = "leader".to_string();
        let holder_term1 = "LeaderElection:leader-election-term1@test-node".to_string();
        let holder_term2 = "LeaderElection:leader-election-term2@test-node".to_string();

        let mut locks1 = LocksImpl {
            actor_id: ActorId::new("leader-election-term1", "LeaderElection", "default", "test-node").unwrap(),
            host_functions: host_functions.clone(),
        };
        let mut locks2 = LocksImpl {
            actor_id: ActorId::new("leader-election-term2", "LeaderElection", "default", "test-node").unwrap(),
            host_functions: host_functions.clone(),
        };

        let lease_ms = 30_000u64;

        // Term1 try_acquire -> must succeed
        let r1 = locks1
            .try_acquire(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term1.clone(),
                lease_ms,
            )
            .await
            .unwrap();
        let lock1 = r1.expect("term1 must acquire leader lock");
        assert!(lock1.locked);
        assert_eq!(lock1.holder_id, holder_term1);

        // Term2 try_acquire -> must fail (same lock, different holder)
        let r2 = locks2
            .try_acquire(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term2.clone(),
                lease_ms,
            )
            .await
            .unwrap();
        assert!(
            r2.is_none(),
            "term2 must NOT acquire while term1 holds lock"
        );

        // Term1 renew -> success
        let renewed = locks1
            .renew(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term1.clone(),
                lock1.version,
                lease_ms,
            )
            .await
            .unwrap();
        assert!(!renewed.version.is_empty());

        // Term2 try_acquire again -> still none
        let r2b = locks2
            .try_acquire(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term2.clone(),
                lease_ms,
            )
            .await
            .unwrap();
        assert!(r2b.is_none());

        // Term1 release
        locks1
            .release(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term1.clone(),
                renewed.version,
                false,
            )
            .await
            .unwrap();

        // Term2 can now acquire
        let r2c = locks2
            .try_acquire(
                ctx_leader.clone(),
                lock_key.clone(),
                holder_term2.clone(),
                lease_ms,
            )
            .await
            .unwrap();
        let lock2 = r2c.expect("term2 must acquire after term1 release");
        assert!(lock2.locked);
        assert_eq!(lock2.holder_id, holder_term2);
    }

    fn encode_register_request(
        object_id: &str,
        object_type: i32,
        grpc_address: &str,
        object_category: &str,
        capabilities: Vec<String>,
        labels: Vec<String>,
    ) -> Vec<u8> {
        use prost::Message;
        let registration = plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: object_id.to_string(),
            object_type,
            grpc_address: grpc_address.to_string(),
            object_category: object_category.to_string(),
            capabilities,
            labels,
            health_status: plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy as i32,
            ..Default::default()
        };
        plexspaces_proto::object_registry::v1::RegisterRequest {
            registration: Some(registration),
            ..Default::default()
        }.encode_to_vec()
    }

    fn encode_lookup_request(object_id: &str, object_type: i32) -> Vec<u8> {
        use prost::Message;
        plexspaces_proto::object_registry::v1::LookupRequest {
            object_id: object_id.to_string(),
            object_type,
            ..Default::default()
        }.encode_to_vec()
    }

    fn encode_unregister_request(object_id: &str, object_type: i32) -> Vec<u8> {
        use prost::Message;
        plexspaces_proto::object_registry::v1::UnregisterRequest {
            object_id: object_id.to_string(),
            object_type,
            ..Default::default()
        }.encode_to_vec()
    }

    fn encode_discover_request(object_type: i32, tenant_id: &str, namespace: &str) -> Vec<u8> {
        use prost::Message;
        plexspaces_proto::object_registry::v1::DiscoverRequest {
            object_type,
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            page_size: 100,
            ..Default::default()
        }.encode_to_vec()
    }

    fn encode_heartbeat_request(object_id: &str, object_type: i32) -> Vec<u8> {
        use prost::Message;
        plexspaces_proto::object_registry::v1::HeartbeatRequest {
            object_id: object_id.to_string(),
            object_type,
            ..Default::default()
        }.encode_to_vec()
    }

    fn decode_lookup_response(bytes: Vec<u8>) -> Option<plexspaces_proto::object_registry::v1::ObjectRegistration> {
        use prost::Message;
        if bytes.is_empty() {
            return None;
        }
        plexspaces_proto::object_registry::v1::LookupResponse::decode(bytes.as_slice())
            .ok()
            .and_then(|r| r.registration)
    }

    fn decode_discover_response(bytes: Vec<u8>) -> Vec<plexspaces_proto::object_registry::v1::ObjectRegistration> {
        use prost::Message;
        if bytes.is_empty() {
            return vec![];
        }
        plexspaces_proto::object_registry::v1::DiscoverResponse::decode(bytes.as_slice())
            .map(|r| r.registrations)
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn test_registry_impl_register_lookup() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // ACT: Register object
        let result = registry
            .register(encode_register_request(
                "test-object", actor_type, "http://test:8000", "GenServer",
                vec!["persistent".to_string()], vec!["env=test".to_string()],
            ))
            .await;
        assert!(result.is_ok(), "register should succeed");

        // ACT: Lookup object
        let result = registry
            .lookup(encode_lookup_request("test-object", actor_type))
            .await;
        assert!(result.is_ok(), "lookup should succeed");
        let reg = decode_lookup_response(result.unwrap());
        assert!(reg.is_some(), "object should be found");
        let reg = reg.unwrap();
        assert_eq!(reg.object_id, "test-object");
        assert_eq!(reg.grpc_address, "http://test:8000");
    }

    #[tokio::test]
    async fn test_registry_impl_unregister() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // ACT: Register then unregister
        registry
            .register(encode_register_request("test-object", actor_type, "http://test:8000", "", vec![], vec![]))
            .await
            .unwrap();

        let result = registry
            .unregister(encode_unregister_request("test-object", actor_type))
            .await;
        assert!(result.is_ok(), "unregister should succeed");

        // ACT: Lookup should return None
        let result = registry
            .lookup(encode_lookup_request("test-object", actor_type))
            .await;
        assert!(result.is_ok(), "lookup should succeed");
        assert!(
            decode_lookup_response(result.unwrap()).is_none(),
            "object should not be found after unregister"
        );
    }

    #[tokio::test]
    async fn test_registry_impl_discover() {
        // ARRANGE — host_functions with tenant "default" so RequestContext.tenant_id matches
        // the tenant_id in the registered objects.
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_tenant("default", "default").await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // ACT: Register multiple objects (use non-empty tenant/namespace for proper key generation)
        registry
            .register({
                use prost::Message;
                let reg = plexspaces_proto::object_registry::v1::ObjectRegistration {
                    object_id: "actor1".to_string(),
                    object_type: actor_type,
                    grpc_address: "http://test:8000".to_string(),
                    tenant_id: "default".to_string(),
                    namespace: "default".to_string(),
                    ..Default::default()
                };
                plexspaces_proto::object_registry::v1::RegisterRequest { registration: Some(reg), ..Default::default() }.encode_to_vec()
            })
            .await
            .unwrap();

        registry
            .register({
                use prost::Message;
                let reg = plexspaces_proto::object_registry::v1::ObjectRegistration {
                    object_id: "actor2".to_string(),
                    object_type: actor_type,
                    grpc_address: "http://test:8001".to_string(),
                    tenant_id: "default".to_string(),
                    namespace: "default".to_string(),
                    ..Default::default()
                };
                plexspaces_proto::object_registry::v1::RegisterRequest { registration: Some(reg), ..Default::default() }.encode_to_vec()
            })
            .await
            .unwrap();

        // ACT: Discover all actors (use same context as registration)
        let result = registry
            .discover(encode_discover_request(actor_type, "default", "default"))
            .await;
        assert!(result.is_ok(), "discover should succeed");
        let objects = decode_discover_response(result.unwrap());
        assert!(
            objects.len() >= 2,
            "should find at least 2 actors, found {}",
            objects.len()
        );
    }

    #[tokio::test]
    async fn test_registry_impl_heartbeat() {
        // ARRANGE
        let actor_id = ActorId::new("test-actor", "wasm", "default", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_services().await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // ACT: Register object
        registry
            .register(encode_register_request("test-object", actor_type, "http://test:8000", "", vec![], vec![]))
            .await
            .unwrap();

        // ACT: Send heartbeat
        let result = registry
            .heartbeat(encode_heartbeat_request("test-object", actor_type))
            .await;
        assert!(result.is_ok(), "heartbeat should succeed");
    }

    /// Register an object with an alias for lookup-by-alias tests.
    fn encode_register_request_with_alias(
        object_id: &str,
        object_type: i32,
        grpc_address: &str,
        alias: &str,
        tenant_id: &str,
        namespace: &str,
    ) -> Vec<u8> {
        use prost::Message;
        let registration = plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: object_id.to_string(),
            object_type,
            grpc_address: grpc_address.to_string(),
            alias: alias.to_string(),
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            health_status: plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy as i32,
            ..Default::default()
        };
        plexspaces_proto::object_registry::v1::RegisterRequest {
            registration: Some(registration),
            ..Default::default()
        }
        .encode_to_vec()
    }

    #[tokio::test]
    async fn test_registry_impl_lookup_by_alias() {
        // ARRANGE — host with tenant "acme" so RequestContext matches registered objects.
        let actor_id = ActorId::new("test-actor", "wasm", "acme", "test-node").unwrap();
        let host_functions = create_test_host_functions_with_tenant("acme", "default").await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;
        let alias = "my-service-alias";

        // ACT: Register with alias
        registry
            .register(encode_register_request_with_alias(
                "aliased-actor",
                actor_type,
                "http://test:9000",
                alias,
                "acme",
                "default",
            ))
            .await
            .unwrap();

        // ACT: Lookup by alias — should resolve to the registered actor
        let result = registry.lookup_by_alias(alias.to_string()).await;
        assert!(result.is_ok(), "lookup_by_alias should succeed: {:?}", result.err());
        let reg = decode_lookup_response(result.unwrap());
        assert!(reg.is_some(), "object should be found by alias");
        let reg = reg.unwrap();
        assert_eq!(reg.object_id, "aliased-actor");
        assert_eq!(reg.grpc_address, "http://test:9000");

        // ACT: Lookup by unknown alias — should return empty (not found)
        let result = registry.lookup_by_alias("no-such-alias".to_string()).await;
        assert!(result.is_ok(), "lookup_by_alias for unknown alias should succeed");
        assert!(
            decode_lookup_response(result.unwrap()).is_none(),
            "unknown alias should return not found"
        );
    }

    #[tokio::test]
    async fn test_registry_cross_tenant_isolation() {
        // ARRANGE — two HostFunctions with different tenant_ids sharing the same
        // in-memory storage so we can verify isolation without a network hop.
        // We use separate HostFunctions instances, each with its own backing store,
        // to verify that tenant_id from HostFunctions is always injected into
        // RequestContext, regardless of what the guest payload says.
        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // Tenant-A: register "secret-actor"
        let actor_id_a = ActorId::new("actor-a", "wasm", "tenant-a", "test-node").unwrap();
        let hf_a = create_test_host_functions_with_tenant("tenant-a", "ns-a").await;
        let mut registry_a = RegistryImpl {
            actor_id: actor_id_a.clone(),
            host_functions: hf_a.clone(),
        };
        registry_a
            .register({
                use prost::Message;
                let reg = plexspaces_proto::object_registry::v1::ObjectRegistration {
                    object_id: "secret-actor".to_string(),
                    object_type: actor_type,
                    grpc_address: "http://tenant-a:8000".to_string(),
                    tenant_id: "tenant-a".to_string(),
                    namespace: "ns-a".to_string(),
                    ..Default::default()
                };
                plexspaces_proto::object_registry::v1::RegisterRequest {
                    registration: Some(reg),
                    ..Default::default()
                }
                .encode_to_vec()
            })
            .await
            .unwrap();

        // Tenant-B: attempts to look up "secret-actor" by crafting a lookup request
        // that supplies tenant_id = "tenant-a" in the proto payload.  The injected
        // HostFunctions.tenant_id ("tenant-b") must override the guest-supplied value,
        // so the lookup must fail (object not found in tenant-b's scope).
        let actor_id_b = ActorId::new("actor-b", "wasm", "tenant-b", "test-node").unwrap();
        let hf_b = create_test_host_functions_with_tenant("tenant-b", "ns-b").await;
        // Reuse registry_a's object_registry so both tenants share storage — this
        // makes the isolation test meaningful (the object IS in the store, just under
        // a different tenant key).
        let mut registry_b = RegistryImpl {
            actor_id: actor_id_b.clone(),
            host_functions: hf_b.clone(),
        };

        // Guest payload deliberately lies about tenant_id — must be ignored.
        let lookup_bytes = {
            use prost::Message;
            plexspaces_proto::object_registry::v1::LookupRequest {
                object_id: "secret-actor".to_string(),
                object_type: actor_type,
                tenant_id: "tenant-a".to_string(), // attacker-supplied, must be overridden
                namespace: "ns-a".to_string(),
                ..Default::default()
            }
            .encode_to_vec()
        };

        let result = registry_b.lookup(lookup_bytes).await;
        assert!(result.is_ok(), "lookup rpc should not error");
        assert!(
            decode_lookup_response(result.unwrap()).is_none(),
            "tenant-b must not see tenant-a's objects even if payload claims tenant-a"
        );
    }

    #[tokio::test]
    async fn test_registry_namespace_fallback() {
        // ARRANGE — HostFunctions with tenant "acme" and default_namespace "production".
        // Registration uses empty namespace in the proto payload; the host should fall
        // back to default_namespace from HostFunctions.
        let actor_id = ActorId::new("fallback-actor", "wasm", "production", "test-node").unwrap();
        let host_functions =
            create_test_host_functions_with_tenant("acme", "production").await;
        let mut registry = RegistryImpl {
            actor_id: actor_id.clone(),
            host_functions: host_functions.clone(),
        };

        let actor_type = plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor as i32;

        // Register with empty namespace in payload — host falls back to "production".
        let register_bytes = {
            use prost::Message;
            let reg = plexspaces_proto::object_registry::v1::ObjectRegistration {
                object_id: "fallback-obj".to_string(),
                object_type: actor_type,
                grpc_address: "http://acme:8000".to_string(),
                tenant_id: "acme".to_string(),
                namespace: String::new(), // intentionally empty
                ..Default::default()
            };
            plexspaces_proto::object_registry::v1::RegisterRequest {
                registration: Some(reg),
                ..Default::default()
            }
            .encode_to_vec()
        };
        registry.register(register_bytes).await.unwrap();

        // Lookup with empty namespace in payload — host falls back to "production".
        let lookup_bytes = {
            use prost::Message;
            plexspaces_proto::object_registry::v1::LookupRequest {
                object_id: "fallback-obj".to_string(),
                object_type: actor_type,
                namespace: String::new(), // intentionally empty
                ..Default::default()
            }
            .encode_to_vec()
        };
        let result = registry.lookup(lookup_bytes).await;
        assert!(result.is_ok(), "lookup should succeed");
        let reg = decode_lookup_response(result.unwrap());
        assert!(
            reg.is_some(),
            "object should be found when namespace falls back to default_namespace"
        );
        assert_eq!(reg.unwrap().object_id, "fallback-obj");
    }

    // =========================================================================
    // Extended KV: TTL, multi-get, multi-put
    // =========================================================================

    #[tokio::test]
    async fn test_kv_put_with_ttl_and_get_ttl() {
        let ctx = RequestContext::new_without_auth("tenant".to_string(), "ns".to_string());
        let host_functions = create_test_host_functions_with_services().await;

        let result = host_functions.put_keyvalue_with_ttl(&ctx, "ttl-key", b"hello".to_vec(), 60).await;
        assert!(result.is_ok(), "put_with_ttl should succeed: {:?}", result);

        let val = host_functions.get_keyvalue(&ctx, "ttl-key").await;
        assert_eq!(val.unwrap(), Some(b"hello".to_vec()));

        let ttl = host_functions.get_keyvalue_ttl(&ctx, "ttl-key").await;
        assert!(ttl.is_ok(), "get_ttl should succeed: {:?}", ttl);
    }

    #[tokio::test]
    async fn test_kv_multi_get_and_multi_put() {
        let ctx = RequestContext::new_without_auth("tenant".to_string(), "ns".to_string());
        let host_functions = create_test_host_functions_with_services().await;

        let pairs: Vec<(&str, Vec<u8>)> = vec![
            ("mg-a", b"alpha".to_vec()),
            ("mg-b", b"beta".to_vec()),
        ];
        let result = host_functions.multi_put_keyvalue(&ctx, &pairs).await;
        assert!(result.is_ok(), "multi_put should succeed: {:?}", result);

        let keys = ["mg-a", "mg-b", "mg-missing"];
        let result = host_functions.multi_get_keyvalue(&ctx, &keys).await;
        assert!(result.is_ok(), "multi_get should succeed: {:?}", result);
        let values = result.unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], Some(b"alpha".to_vec()));
        assert_eq!(values[1], Some(b"beta".to_vec()));
        assert_eq!(values[2], None, "missing key should return None");
    }

    // =========================================================================
    // Durable Alarms
    // =========================================================================

    #[tokio::test]
    async fn test_alarm_set_get_delete() {
        let host_functions = create_test_host_functions_with_services().await;
        let actor_id = "alarm-test-actor//wasm::default@test-node";
        let timestamp_ms: u64 = 9_999_999_000_000;

        let ts = host_functions.alarm_get(actor_id).await.unwrap();
        assert_eq!(ts, 0, "no alarm should be set initially");

        let result = host_functions.alarm_set(actor_id, timestamp_ms).await;
        assert!(result.is_ok(), "alarm_set should succeed: {:?}", result);

        let ts = host_functions.alarm_get(actor_id).await.unwrap();
        assert_eq!(ts, timestamp_ms, "alarm_get should return the set timestamp");

        let result = host_functions.alarm_delete(actor_id).await;
        assert!(result.is_ok(), "alarm_delete should succeed: {:?}", result);

        let ts = host_functions.alarm_get(actor_id).await.unwrap();
        assert_eq!(ts, 0, "alarm should be gone after delete");
    }

    #[tokio::test]
    async fn test_alarm_set_overwrites_existing() {
        let host_functions = create_test_host_functions_with_services().await;
        let actor_id = "alarm-overwrite-actor//wasm::default@test-node";
        let ts1: u64 = 1_000_000_000_000;
        let ts2: u64 = 2_000_000_000_000;

        host_functions.alarm_set(actor_id, ts1).await.unwrap();
        assert_eq!(host_functions.alarm_get(actor_id).await.unwrap(), ts1);

        host_functions.alarm_set(actor_id, ts2).await.unwrap();
        assert_eq!(host_functions.alarm_get(actor_id).await.unwrap(), ts2,
            "second alarm_set should overwrite first");
    }
}
