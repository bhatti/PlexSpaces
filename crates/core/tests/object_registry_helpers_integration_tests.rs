// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for object_registry_helpers with SQLite backend
// Tests discovery operations with caching and multi-node scenarios

#[cfg(feature = "sql-backend")]
mod tests {
    use plexspaces_core::RequestContext;
    use plexspaces_core::object_registry_helpers::*;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_keyvalue::SqliteKVStore;
    use plexspaces_proto::object_registry::v1::ObjectType;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    /// Helper to create a SQLite database for testing
    async fn create_test_db() -> Arc<SqliteKVStore> {
        // Use in-memory database for each test
        Arc::new(SqliteKVStore::new(":memory:").await.unwrap())
    }

    /// Helper to create test registry
    async fn create_test_registry() -> Arc<ObjectRegistry> {
        let kv = create_test_db().await;
        Arc::new(ObjectRegistry::new(kv))
    }
    

    #[tokio::test]
    async fn test_discover_nodes_basic() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register multiple nodes
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", Some("cluster1")).await.unwrap();
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", Some("cluster1")).await.unwrap();
        register_node(&registry, &ctx, "node3", "http://127.0.0.1:9003", Some("cluster2")).await.unwrap();
        
        // Discover all nodes
        let nodes = discover_nodes(&registry, &ctx).await.unwrap();
        assert_eq!(nodes.len(), 3);
        
        let node_ids: Vec<String> = nodes.iter().map(|n| n.object_id.clone()).collect();
        assert!(node_ids.contains(&"node1".to_string()));
        assert!(node_ids.contains(&"node2".to_string()));
        assert!(node_ids.contains(&"node3".to_string()));
    }

    #[tokio::test]
    async fn test_discover_nodes_caching() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register a node
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        
        // First discovery - should query registry
        let nodes1 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes1.len(), 1);
        
        // Second discovery immediately - should use cache
        let nodes2 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes2.len(), 1);
        assert_eq!(nodes1[0].object_id, nodes2[0].object_id);
    }

    #[tokio::test]
    async fn test_discover_nodes_cache_invalidation() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register and discover
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        let nodes1 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes1.len(), 1);
        
        // Register another node - should invalidate cache
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Next discovery should see both nodes (cache was invalidated)
        let nodes2 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes2.len(), 2);
    }

    #[tokio::test]
    async fn test_discover_nodes_cache_expiration() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register a node
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        
        // First discovery
        let nodes1 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes1.len(), 1);
        
        // Register another node after cache is populated
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Wait for cache to expire (60 seconds) - but we'll test invalidation instead
        // Since register_node invalidates cache, we should see both nodes
        let nodes2 = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes2.len(), 2);
    }

    #[tokio::test]
    async fn test_discover_application_nodes_basic() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register nodes
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Register application on multiple nodes
        register_application(&registry, &ctx, "myapp", "1.0.0", "node1", "http://127.0.0.1:9001").await.unwrap();
        register_application(&registry, &ctx, "myapp", "1.0.0", "node2", "http://127.0.0.1:9002").await.unwrap();
        
        // Discover application nodes
        let nodes = discover_application_nodes(&trait_registry, &ctx, "myapp").await.unwrap();
        assert_eq!(nodes.len(), 2);
        
        let node_ids: Vec<String> = nodes.iter().map(|n| n.node_id.clone()).collect();
        assert!(node_ids.contains(&"node1".to_string()));
        assert!(node_ids.contains(&"node2".to_string()));
    }

    #[tokio::test]
    async fn test_discover_application_nodes_caching() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_application(&registry, &ctx, "myapp", "1.0.0", "node1", "http://127.0.0.1:9001").await.unwrap();
        
        // First discovery
        let nodes1 = discover_application_nodes(&trait_registry, &ctx, "myapp").await.unwrap();
        assert_eq!(nodes1.len(), 1);
        
        // Second discovery - should use cache
        let nodes2 = discover_application_nodes(&trait_registry, &ctx, "myapp").await.unwrap();
        assert_eq!(nodes2.len(), 1);
        assert_eq!(nodes1[0].object_id, nodes2[0].object_id);
    }

    #[tokio::test]
    async fn test_discover_workflow_nodes_basic() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register nodes
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Register workflows with same definition on different nodes
        register_workflow(&registry, &ctx, "workflow1", "def1", "node1", "http://127.0.0.1:9001").await.unwrap();
        register_workflow(&registry, &ctx, "workflow2", "def1", "node2", "http://127.0.0.1:9002").await.unwrap();
        
        // Discover workflow nodes
        let nodes = discover_workflow_nodes(&trait_registry, &ctx, "def1").await.unwrap();
        assert_eq!(nodes.len(), 2);
        
        let node_ids: Vec<String> = nodes.iter().map(|n| n.node_id.clone()).collect();
        assert!(node_ids.contains(&"node1".to_string()));
        assert!(node_ids.contains(&"node2".to_string()));
    }

    #[tokio::test]
    async fn test_discover_workflow_nodes_caching() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_workflow(&registry, &ctx, "workflow1", "def1", "node1", "http://127.0.0.1:9001").await.unwrap();
        
        // First discovery
        let nodes1 = discover_workflow_nodes(&trait_registry, &ctx, "def1").await.unwrap();
        assert_eq!(nodes1.len(), 1);
        
        // Second discovery - should use cache
        let nodes2 = discover_workflow_nodes(&trait_registry, &ctx, "def1").await.unwrap();
        assert_eq!(nodes2.len(), 1);
        assert_eq!(nodes1[0].object_id, nodes2[0].object_id);
    }

    #[tokio::test]
    async fn test_multi_tenant_isolation() {
        let registry = create_test_registry().await;
        
        // Register nodes in different tenants
        let ctx1 = RequestContext::new_without_auth("tenant1".to_string(), "namespace1".to_string());
        register_node(&registry, &ctx1, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        
        let ctx2 = RequestContext::new_without_auth("tenant2".to_string(), "namespace2".to_string());
        register_node(&registry, &ctx2, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Discover nodes in tenant1 - should only see node1
        let nodes1 = discover_nodes(&trait_registry, &ctx1).await.unwrap();
        assert_eq!(nodes1.len(), 1);
        assert_eq!(nodes1[0].object_id, "node1");
        
        // Discover nodes in tenant2 - should only see node2
        let nodes2 = discover_nodes(&trait_registry, &ctx2).await.unwrap();
        assert_eq!(nodes2.len(), 1);
        assert_eq!(nodes2[0].object_id, "node2");
    }

    #[tokio::test]
    async fn test_discover_empty_results() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Discover nodes when none are registered
        let nodes = discover_nodes(&trait_registry, &ctx).await.unwrap();
        assert_eq!(nodes.len(), 0);
        
        // Discover application that doesn't exist
        let apps = discover_application_nodes(&trait_registry, &ctx, "nonexistent").await.unwrap();
        assert_eq!(apps.len(), 0);
        
        // Discover workflow that doesn't exist
        let workflows = discover_workflow_nodes(&trait_registry, &ctx, "nonexistent").await.unwrap();
        assert_eq!(workflows.len(), 0);
    }

    #[tokio::test]
    async fn test_register_node_with_cluster() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        let result = register_node(
            &trait_registry,
            &ctx,
            "node1",
            "http://127.0.0.1:9001",
            Some("cluster1"),
        ).await;
        
        assert!(result.is_ok());
        
        // Verify registration
        let registration = registry.lookup_full(&ctx, ObjectType::ObjectTypeNode, "node1").await.unwrap();
        assert!(registration.is_some());
        let reg = registration.unwrap();
        assert_eq!(reg.object_id, "node1");
        assert_eq!(reg.grpc_address, "http://127.0.0.1:9001");
        assert!(reg.labels.contains(&"cluster1".to_string()));
    }

    #[tokio::test]
    async fn test_register_application() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        
        let result = register_application(
            &trait_registry,
            &ctx,
            "myapp",
            "1.0.0",
            "node1",
            "http://127.0.0.1:9001",
        ).await;
        
        assert!(result.is_ok());
        
        // Verify registration
        let registration = registry.lookup_full(&ctx, ObjectType::ObjectTypeApplication, "myapp@node1").await.unwrap();
        assert!(registration.is_some());
        let reg = registration.unwrap();
        assert_eq!(reg.object_name, "myapp");
        assert_eq!(reg.version, "1.0.0");
        assert_eq!(reg.node_id, "node1");
    }

    #[tokio::test]
    async fn test_register_workflow() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        
        let result = register_workflow(
            &trait_registry,
            &ctx,
            "workflow1",
            "def1",
            "node1",
            "http://127.0.0.1:9001",
        ).await;
        
        assert!(result.is_ok());
        
        // Verify registration
        let registration = registry.lookup_full(&ctx, ObjectType::ObjectTypeWorkflow, "workflow1").await.unwrap();
        assert!(registration.is_some());
        let reg = registration.unwrap();
        assert_eq!(reg.object_id, "workflow1");
        assert_eq!(reg.object_category, "def1");
        assert_eq!(reg.node_id, "node1");
    }

    #[tokio::test]
    async fn test_discover_multiple_applications() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Register different applications
        register_application(&registry, &ctx, "app1", "1.0.0", "node1", "http://127.0.0.1:9001").await.unwrap();
        register_application(&registry, &ctx, "app2", "1.0.0", "node1", "http://127.0.0.1:9001").await.unwrap();
        register_application(&registry, &ctx, "app1", "1.0.0", "node2", "http://127.0.0.1:9002").await.unwrap();
        
        // Discover app1 nodes - should find both node1 and node2
        let app1_nodes = discover_application_nodes(&trait_registry, &ctx, "app1").await.unwrap();
        assert_eq!(app1_nodes.len(), 2);
        
        // Discover app2 nodes - should find only node1
        let app2_nodes = discover_application_nodes(&trait_registry, &ctx, "app2").await.unwrap();
        assert_eq!(app2_nodes.len(), 1);
        assert_eq!(app2_nodes[0].node_id, "node1");
    }

    #[tokio::test]
    async fn test_discover_multiple_workflows() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        register_node(&registry, &ctx, "node1", "http://127.0.0.1:9001", None).await.unwrap();
        register_node(&registry, &ctx, "node2", "http://127.0.0.1:9002", None).await.unwrap();
        
        // Register workflows with different definitions
        register_workflow(&registry, &ctx, "workflow1", "def1", "node1", "http://127.0.0.1:9001").await.unwrap();
        register_workflow(&registry, &ctx, "workflow2", "def1", "node2", "http://127.0.0.1:9002").await.unwrap();
        register_workflow(&registry, &ctx, "workflow3", "def2", "node1", "http://127.0.0.1:9001").await.unwrap();
        
        // Discover def1 workflows - should find both node1 and node2
        let def1_nodes = discover_workflow_nodes(&trait_registry, &ctx, "def1").await.unwrap();
        assert_eq!(def1_nodes.len(), 2);
        
        // Discover def2 workflows - should find only node1
        let def2_nodes = discover_workflow_nodes(&trait_registry, &ctx, "def2").await.unwrap();
        assert_eq!(def2_nodes.len(), 1);
        assert_eq!(def2_nodes[0].node_id, "node1");
    }

    #[tokio::test]
    async fn test_discover_nodes_pagination() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register 10 nodes
        for i in 1..=10 {
            register_node(&registry, &ctx, &format!("node{}", i), &format!("http://127.0.0.1:{}", 9000 + i), None).await.unwrap();
        }
        
        // Test pagination: first page (offset=0, limit=5)
        let nodes1 = registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeNode),
            None,
            None,
            None,
            None,
            0, // offset
            5, // limit
        ).await.unwrap();
        assert_eq!(nodes1.len(), 5);
        
        // Test pagination: second page (offset=5, limit=5)
        let nodes2 = registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeNode),
            None,
            None,
            None,
            None,
            5, // offset
            5, // limit
        ).await.unwrap();
        assert_eq!(nodes2.len(), 5);
        
        // Test pagination: last page (offset=8, limit=5) - should return 2
        let nodes3 = registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeNode),
            None,
            None,
            None,
            None,
            8, // offset
            5, // limit
        ).await.unwrap();
        assert_eq!(nodes3.len(), 2);
        
        // Verify no overlap
        let ids1: Vec<String> = nodes1.iter().map(|n| n.object_id.clone()).collect();
        let ids2: Vec<String> = nodes2.iter().map(|n| n.object_id.clone()).collect();
        let ids3: Vec<String> = nodes3.iter().map(|n| n.object_id.clone()).collect();
        
        // Check no duplicates
        let all_ids: Vec<String> = [ids1, ids2, ids3].concat();
        let unique_ids: std::collections::HashSet<String> = all_ids.iter().cloned().collect();
        assert_eq!(unique_ids.len(), 10); // All 10 nodes should be present
    }

    #[tokio::test]
    async fn test_discover_application_nodes_pagination() {
        let registry = create_test_registry().await;
        let ctx = RequestContext::internal();
        
        // Register nodes
        for i in 1..=5 {
            register_node(&registry, &ctx, &format!("node{}", i), &format!("http://127.0.0.1:{}", 9000 + i), None).await.unwrap();
        }
        
        // Register application on all nodes
        for i in 1..=5 {
            register_application(&registry, &ctx, "myapp", "1.0.0", &format!("node{}", i), &format!("http://127.0.0.1:{}", 9000 + i)).await.unwrap();
        }
        
        // Test pagination: first page
        let nodes1 = registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeApplication),
            Some("myapp".to_string()),
            None,
            None,
            None,
            0, // offset
            3, // limit
        ).await.unwrap();
        assert_eq!(nodes1.len(), 3);
        
        // Test pagination: second page
        let nodes2 = registry.discover(
            &ctx,
            Some(ObjectType::ObjectTypeApplication),
            Some("myapp".to_string()),
            None,
            None,
            None,
            3, // offset
            3, // limit
        ).await.unwrap();
        assert_eq!(nodes2.len(), 2);
    }
}

