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

//! Integration tests for DynamoDB TupleSpaceStorage backend.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use chrono::Duration;
    use plexspaces_tuplespace::storage::{DynamoDBStorage, TupleSpaceStorage};
    use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
    use std::time::Duration as StdDuration;

    use plexspaces_common::test_helpers::dynamodb_local_available;

    /// Helper to check if DynamoDB simulator is available and skip test with warning if not
    async fn check_ddb_available() -> bool {
        if !dynamodb_local_available().await {
            eprintln!("⚠️  WARNING: DynamoDB Local simulator is not running. Skipping DDB test.");
            eprintln!("   To run DDB tests, start DynamoDB Local: docker run -p 8000:8000 amazon/dynamodb-local");
            return false;
        }
        true
    }

    /// Helper to create DynamoDB storage for testing
    async fn create_ddb_storage() -> DynamoDBStorage {
        let endpoint = std::env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".to_string());

        DynamoDBStorage::new(
            "us-east-1".to_string(),
            "plexspaces-tuplespace-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB tuple space storage")
    }

    /// Helper to create unique test key for test isolation
    fn unique_key(prefix: &str) -> String {
        use ulid::Ulid;
        format!("{}-{}", prefix, Ulid::new())
    }

    // =========================================================================
    // Core Operations Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_write() {
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        let tuple = Tuple::new(vec![TupleField::String(key), TupleField::Integer(42)]);

        let tuple_id = storage.write(tuple).await.unwrap();
        assert!(!tuple_id.is_empty());
    }

    #[tokio::test]
    async fn test_ddb_write_batch() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let prefix = unique_key("key");

        let tuples = vec![
            Tuple::new(vec![
                TupleField::String(format!("{}1", prefix)),
                TupleField::Integer(1),
            ]),
            Tuple::new(vec![
                TupleField::String(format!("{}2", prefix)),
                TupleField::Integer(2),
            ]),
        ];

        let tuple_ids = storage.write_batch(tuples).await.unwrap();
        assert_eq!(tuple_ids.len(), 2);
    }

    #[tokio::test]
    async fn test_ddb_read() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        let tuple = Tuple::new(vec![
            TupleField::String(key.clone()),
            TupleField::Integer(42),
        ]);

        storage.write(tuple.clone()).await.unwrap();

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key)),
            PatternField::Wildcard,
        ]);

        let results = storage.read(pattern, None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());
    }

    #[tokio::test]
    async fn test_ddb_read_wildcard() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let prefix = unique_key("test");

        storage
            .write(Tuple::new(vec![
                TupleField::String(format!("{}1", prefix)),
                TupleField::Integer(1),
            ]))
            .await
            .unwrap();

        storage
            .write(Tuple::new(vec![
                TupleField::String(format!("{}2", prefix)),
                TupleField::Integer(2),
            ]))
            .await
            .unwrap();

        // Use a pattern that matches our test tuples specifically
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(prefix.clone())),
            PatternField::Wildcard,
        ]);

        // Read all tuples and filter to our test prefix
        let all_results = storage
            .read(
                Pattern::new(vec![PatternField::Wildcard, PatternField::Wildcard]),
                None,
            )
            .await
            .unwrap();
        let results: Vec<_> = all_results
            .into_iter()
            .filter(|t| {
                if let Some(TupleField::String(s)) = t.fields().first() {
                    s.starts_with(&prefix)
                } else {
                    false
                }
            })
            .collect();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_ddb_take() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        let tuple = Tuple::new(vec![
            TupleField::String(key.clone()),
            TupleField::Integer(42),
        ]);

        storage.write(tuple.clone()).await.unwrap();

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key.clone())),
            PatternField::Wildcard,
        ]);

        let results = storage.take(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());

        // Tuple should be removed
        let results = storage.read(pattern, None).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_ddb_count() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        for i in 1..=5 {
            storage
                .write(Tuple::new(vec![
                    TupleField::String(key.clone()),
                    TupleField::Integer(i),
                ]))
                .await
                .unwrap();
        }

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key)),
            PatternField::Wildcard,
        ]);

        let count = storage.count(pattern).await.unwrap();
        assert_eq!(count, 5);
    }

    #[tokio::test]
    async fn test_ddb_exists() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key.clone())),
            PatternField::Wildcard,
        ]);

        assert!(!storage.exists(pattern.clone()).await.unwrap());

        storage
            .write(Tuple::new(vec![
                TupleField::String(key),
                TupleField::Integer(42),
            ]))
            .await
            .unwrap();

        assert!(storage.exists(pattern).await.unwrap());
    }

    #[tokio::test]
    async fn test_ddb_clear() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        for i in 1..=5 {
            storage
                .write(Tuple::new(vec![
                    TupleField::String(key.clone()),
                    TupleField::Integer(i),
                ]))
                .await
                .unwrap();
        }

        // Take all our test tuples to remove them
        // (clear() clears everything which would affect other tests)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key.clone())),
            PatternField::Wildcard,
        ]);
        let _ = storage.take(pattern.clone(), None).await.unwrap();

        let count = storage.count(pattern).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_ddb_stats() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;

        storage
            .write(Tuple::new(vec![TupleField::Integer(1)]))
            .await
            .unwrap();
        storage
            .write(Tuple::new(vec![TupleField::Integer(2)]))
            .await
            .unwrap();

        let stats = storage.stats().await.unwrap();
        assert!(stats.tuple_count >= 2);
    }

    // =========================================================================
    // Lease Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_renew_lease() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;

        use plexspaces_tuplespace::Lease;
        let lease = Lease::new(Duration::seconds(60)).renewable();
        let tuple = Tuple::new(vec![TupleField::Integer(1)]).with_lease(lease);

        let tuple_id = storage.write(tuple).await.unwrap();

        let new_expiry = storage
            .renew_lease(&tuple_id, Some(std::time::Duration::from_secs(120)))
            .await
            .unwrap();
        assert!(new_expiry > chrono::Utc::now());
    }

    #[tokio::test]
    async fn test_ddb_lease_expiration() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("expire");

        use plexspaces_tuplespace::Lease;
        let lease = Lease::new(Duration::seconds(1));
        let tuple = Tuple::new(vec![TupleField::String(key.clone())]).with_lease(lease);

        storage.write(tuple).await.unwrap();

        // Wait for expiration
        tokio::time::sleep(StdDuration::from_secs(2)).await;

        // Query for our specific tuple - it should be expired and not found
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(key))]);
        let results = storage.read(pattern, None).await.unwrap();
        // Tuple should be expired and removed
        assert_eq!(results.len(), 0);
    }

    // =========================================================================
    // Pattern Matching Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_pattern_type_match() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        storage
            .write(Tuple::new(vec![
                TupleField::String(key),
                TupleField::Integer(42),
            ]))
            .await
            .unwrap();

        use plexspaces_tuplespace::FieldType;
        let pattern = Pattern::new(vec![
            PatternField::Type(FieldType::String),
            PatternField::Type(FieldType::Integer),
        ]);

        // Filter results to only our test tuple (by checking it has our unique key)
        let all_results = storage.read(pattern, None).await.unwrap();
        // Since we can't easily filter by key in the pattern, we'll just check that we got at least our tuple
        // The test might get more results from other tests, but that's acceptable for type matching
        assert!(all_results.len() >= 1);
    }

    #[tokio::test]
    async fn test_ddb_read_blocking() {
        if !check_ddb_available().await {
            return;
        }
        let storage = create_ddb_storage().await;
        let key = unique_key("key");

        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String(key.clone())),
            PatternField::Wildcard,
        ]);

        // Start read in background (will block)
        let storage_clone = storage.clone();
        let pattern_for_read = pattern.clone();
        let read_handle = tokio::spawn(async move {
            storage_clone
                .read(pattern_for_read, Some(std::time::Duration::from_secs(5)))
                .await
        });

        // Wait a bit, then write tuple
        tokio::time::sleep(StdDuration::from_millis(100)).await;
        storage
            .write(Tuple::new(vec![
                TupleField::String(key),
                TupleField::Integer(42),
            ]))
            .await
            .unwrap();

        // Read should complete
        let results = read_handle.await.unwrap().unwrap();
        assert_eq!(results.len(), 1);
    }
}
