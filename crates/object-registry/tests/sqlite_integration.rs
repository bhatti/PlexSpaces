// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ObjectRegistry with SQLite backend
// Tests multi-node scenarios using shared SQLite database

#[cfg(feature = "sql-backend")]
mod tests {
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use plexspaces_keyvalue::SqliteKVStore;
    use std::sync::Arc;

    /// Helper to create a SQLite database for testing
    /// Uses in-memory database for simplicity (all tests share same DB instance)
    async fn create_test_db() -> Arc<SqliteKVStore> {
        Arc::new(SqliteKVStore::new(":memory:").await.unwrap())
    }

    /// Helper to create test registration
    fn create_test_registration(
        object_id: &str,
        object_type: ObjectType,
        node_id: &str,
    ) -> ObjectRegistration {
        ObjectRegistration {
            object_id: object_id.to_string(),
            object_type: object_type as i32,
            grpc_address: format!("http://{}:9001", node_id),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            object_category: "GenServer".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_multi_node_registration() {
        // Simulate multiple nodes sharing the same SQLite database
        let kv = create_test_db().await;

        // Node 1 registers an actor
        let registry1 = ObjectRegistry::new(kv.clone());
        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor, "node1");
        registry1.register(reg1).await.unwrap();

        // Node 2 registers an actor
        let registry2 = ObjectRegistry::new(kv.clone());
        let reg2 = create_test_registration("actor2@node2", ObjectType::ObjectTypeActor, "node2");
        registry2.register(reg2).await.unwrap();

        // Node 3 registers a tuplespace
        let registry3 = ObjectRegistry::new(kv.clone());
        let reg3 = create_test_registration("ts1", ObjectType::ObjectTypeTuplespace, "node3");
        registry3.register(reg3).await.unwrap();

        // Node 1 can discover all actors (cross-node discovery)
        let actors = registry1
            .discover("default", "default", Some(ObjectType::ObjectTypeActor), None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(actors.len(), 2);
        assert!(actors.iter().any(|a| a.object_id == "actor1@node1"));
        assert!(actors.iter().any(|a| a.object_id == "actor2@node2"));

        // Node 2 can discover tuplespaces
        let tuplespaces = registry2
            .discover(Some(ObjectType::ObjectTypeTuplespace), None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(tuplespaces.len(), 1);
        assert_eq!(tuplespaces[0].object_id, "ts1");
    }

    #[tokio::test]
    async fn test_cross_node_lookup() {
        // Test that nodes can look up objects registered by other nodes
        let kv = create_test_db().await;

        // Node 1 registers an actor
        let registry1 = ObjectRegistry::new(kv.clone());
        let reg1 = create_test_registration("counter@node1", ObjectType::ObjectTypeActor, "node1");
        registry1.register(reg1).await.unwrap();

        // Node 2 looks up the actor registered by Node 1
        let registry2 = ObjectRegistry::new(kv.clone());
        let found = registry2
            .lookup("default", "default", ObjectType::ObjectTypeActor, "counter@node1")
            .await
            .unwrap();

        assert!(found.is_some());
        let found_reg = found.unwrap();
        assert_eq!(found_reg.object_id, "counter@node1");
        assert_eq!(found_reg.grpc_address, "http://node1:9001");
    }

    #[tokio::test]
    async fn test_concurrent_registration() {
        // Test concurrent registration from multiple nodes
        let kv = create_test_db().await;

        // Simulate concurrent registration from 3 nodes
        let mut handles = Vec::new();
        for i in 1..=3 {
            let kv_clone = kv.clone();
            let handle = tokio::spawn(async move {
                let registry = ObjectRegistry::new(kv_clone);
                let reg = create_test_registration(
                    &format!("actor{}@node{}", i, i),
                    ObjectType::ObjectTypeActor,
                    &format!("node{}", i),
                );
                registry.register(reg).await
            });
            handles.push(handle);
        }

        // Wait for all registrations
        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        // Verify all were registered
        let registry = ObjectRegistry::new(kv.clone());
        let actors = registry
            .discover("default", "default", Some(ObjectType::ObjectTypeActor), None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(actors.len(), 3);
    }

    #[tokio::test]
    async fn test_heartbeat_across_nodes() {
        // Test that heartbeat updates are visible across nodes
        let kv = create_test_db().await;

        // Node 1 registers an actor
        let registry1 = ObjectRegistry::new(kv.clone());
        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor, "node1");
        registry1.register(reg1).await.unwrap();

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Node 2 updates heartbeat
        let registry2 = ObjectRegistry::new(kv.clone());
        registry2
            .heartbeat("default", "default", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap();

        // Node 1 can see the updated heartbeat
        let found = registry1
            .lookup("default", "default", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap()
            .unwrap();
        assert!(found.last_heartbeat.is_some());
    }

    #[tokio::test]
    async fn test_unregister_from_different_node() {
        // Test that one node can unregister an object registered by another node
        let kv = create_test_db().await;

        // Node 1 registers an actor
        let registry1 = ObjectRegistry::new(kv.clone());
        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor, "node1");
        registry1.register(reg1).await.unwrap();

        // Node 2 unregisters it
        let registry2 = ObjectRegistry::new(kv.clone());
        registry2
            .unregister("default", "default", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap();

        // Node 1 can no longer find it
        let found = registry1
            .lookup("default", "default", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_discover_by_capability() {
        // Test discovering objects by capabilities (using capabilities field, not metadata)
        let kv = create_test_db().await;

        // Register actors with different capabilities
        let registry = ObjectRegistry::new(kv.clone());

        let mut reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor, "node1");
        reg1.capabilities.push("wasm".to_string());
        registry.register(reg1).await.unwrap();

        let mut reg2 = create_test_registration("actor2@node2", ObjectType::ObjectTypeActor, "node2");
        reg2.capabilities.push("firecracker".to_string());
        registry.register(reg2).await.unwrap();

        // Discover all actors (should find both)
        let all_actors = registry
            .discover("default", "default", Some(ObjectType::ObjectTypeActor), None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(all_actors.len(), 2);

        // Verify capabilities are stored and can be retrieved
        let found = registry
            .lookup("default", "default", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap()
            .unwrap();
        assert!(found.capabilities.contains(&"wasm".to_string()));
    }

    #[tokio::test]
    async fn test_tenant_namespace_isolation() {
        // Test that different tenants/namespaces are isolated
        let kv = create_test_db().await;

        let registry = ObjectRegistry::new(kv.clone());

        // Register in tenant1:namespace1
        let mut reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor, "node1");
        reg1.tenant_id = "tenant1".to_string();
        reg1.namespace = "namespace1".to_string();
        registry.register(reg1).await.unwrap();

        // Register in tenant2:namespace2
        let mut reg2 = create_test_registration("actor2@node2", ObjectType::ObjectTypeActor, "node2");
        reg2.tenant_id = "tenant2".to_string();
        reg2.namespace = "namespace2".to_string();
        registry.register(reg2).await.unwrap();

        // Discover in default namespace (should not find either)
        let default_actors = registry
            .discover("default", "default", Some(ObjectType::ObjectTypeActor), None, None, None, None, 100)
            .await
            .unwrap();
        assert_eq!(default_actors.len(), 0);

        // Lookup in tenant1:namespace1 (should find actor1)
        let found1 = registry
            .lookup("tenant1", "namespace1", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap();
        assert!(found1.is_some());

        // Lookup in tenant2:namespace2 (should find actor2)
        let found2 = registry
            .lookup("tenant2", "namespace2", ObjectType::ObjectTypeActor, "actor2@node2")
            .await
            .unwrap();
        assert!(found2.is_some());

        // Lookup actor1 in tenant2:namespace2 (should not find)
        let not_found = registry
            .lookup("tenant2", "namespace2", ObjectType::ObjectTypeActor, "actor1@node1")
            .await
            .unwrap();
        assert!(not_found.is_none());
    }
}

