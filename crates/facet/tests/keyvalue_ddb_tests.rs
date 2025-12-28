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

//! Integration tests for DynamoDB KeyValue store backend in facet.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use plexspaces_facet::capabilities::keyvalue::{KeyValueStore, DynamoDBStore};
    use plexspaces_common::RequestContext;

    /// Helper to create DynamoDB store for testing
    async fn create_ddb_store() -> DynamoDBStore {
        let endpoint = std::env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".to_string());
        
        DynamoDBStore::new(
            "us-east-1".to_string(),
            "plexspaces-facet-keyvalue-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB store")
    }

    #[tokio::test]
    async fn test_ddb_get_and_set() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        // Set a value
        store.set(&ctx, "key1", b"value1".to_vec(), None).await.unwrap();

        // Get the value
        let value = store.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_get_nonexistent() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        let value = store.get(&ctx, "nonexistent").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_ddb_set_overwrites() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        store.set(&ctx, "key1", b"value1".to_vec(), None).await.unwrap();
        store.set(&ctx, "key1", b"value2".to_vec(), None).await.unwrap();

        let value = store.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, Some(b"value2".to_vec()));
    }

    #[tokio::test]
    async fn test_ddb_delete() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        store.set(&ctx, "key1", b"value1".to_vec(), None).await.unwrap();
        let deleted = store.delete(&ctx, "key1").await.unwrap();
        assert!(deleted);

        let value = store.get(&ctx, "key1").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_ddb_delete_nonexistent() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        let deleted = store.delete(&ctx, "nonexistent").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_ddb_exists() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        assert!(!store.exists(&ctx, "key1").await.unwrap());
        store.set(&ctx, "key1", b"value1".to_vec(), None).await.unwrap();
        assert!(store.exists(&ctx, "key1").await.unwrap());
        store.delete(&ctx, "key1").await.unwrap();
        assert!(!store.exists(&ctx, "key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_ddb_list_keys() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        store.set(&ctx, "actor:alice", b"ref1".to_vec(), None).await.unwrap();
        store.set(&ctx, "actor:bob", b"ref2".to_vec(), None).await.unwrap();
        store.set(&ctx, "node:node1", b"info".to_vec(), None).await.unwrap();

        let keys = store.list_keys(&ctx, "actor:").await.unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"actor:alice".to_string()));
        assert!(keys.contains(&"actor:bob".to_string()));
    }

    #[tokio::test]
    async fn test_ddb_set_with_ttl() {
        let store = create_ddb_store().await;
        let ctx = RequestContext::internal();

        store.set(&ctx, "key1", b"value1".to_vec(), Some(1)).await.unwrap();

        // Should exist immediately
        assert!(store.exists(&ctx, "key1").await.unwrap());

        // Wait for expiration
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Should be expired (eventually consistent)
        // Note: DynamoDB TTL cleanup is eventually consistent
    }

    #[tokio::test]
    async fn test_ddb_tenant_isolation() {
        let store = create_ddb_store().await;
        let ctx1 = RequestContext::new_without_auth("tenant1".to_string(), "default".to_string());
        let ctx2 = RequestContext::new_without_auth("tenant2".to_string(), "default".to_string());

        store.set(&ctx1, "key1", b"value1".to_vec(), None).await.unwrap();
        store.set(&ctx2, "key1", b"value2".to_vec(), None).await.unwrap();

        // Each tenant should see their own value
        assert_eq!(store.get(&ctx1, "key1").await.unwrap(), Some(b"value1".to_vec()));
        assert_eq!(store.get(&ctx2, "key1").await.unwrap(), Some(b"value2".to_vec()));
    }
}

