// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Unit tests for tuplespace host function implementations
//! Tests the tuplespace host function bindings for WASM components

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::TuplespaceImpl;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::types::Context;
    use plexspaces_core::ActorId;
    use std::sync::Arc;
    use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    fn create_test_tuplespace() -> Arc<dyn plexspaces_core::TupleSpaceProvider> {
        // Create an in-memory tuplespace for testing
        let tuplespace = TupleSpace::new();
        Arc::new(plexspaces_core::TupleSpaceProviderWrapper::new(Arc::new(tuplespace)))
    }

    #[tokio::test]
    async fn test_tuplespace_impl_write() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Convert test tuple to WIT format
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);
        
        // Convert to WIT format (simulate what component would send)
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_tuple = vec![
            types::TupleField::StringVal("test".to_string()),
            types::TupleField::IntVal(42),
        ];

        // ACT
        let result = tuplespace_impl.write(test_context("", ""), wit_tuple).await;

        // ASSERT
        assert!(result.is_ok(), "write should succeed");
        
        // Verify tuple was written by reading it back
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let read_result = tuplespace_provider.read(&pattern).await;
        assert!(read_result.is_ok());
        let tuples = read_result.unwrap();
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0].fields()[0], TupleField::String("test".to_string()));
        assert_eq!(tuples[0].fields()[1], TupleField::Integer(42));
    }

    #[tokio::test]
    async fn test_tuplespace_impl_write_without_provider() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(None, actor_id);

        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_tuple = vec![
            types::TupleField::StringVal("test".to_string()),
        ];

        // ACT
        let result = tuplespace_impl.write(test_context("", ""), wit_tuple).await;

        // ASSERT: Should return NotImplemented error
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(
                e.code,
                types::ErrorCode::NotImplemented
            );
        }
    }

    #[tokio::test]
    async fn test_tuplespace_impl_read() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Write a tuple first
        let tuple = Tuple::new(vec![
            TupleField::String("read-test".to_string()),
            TupleField::Integer(100),
        ]);
        tuplespace_provider.write(tuple).await.unwrap();

        // Create pattern in WIT format
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("read-test".to_string())),
            types::PatternField::Any,
        ];

        // ACT
        let result = tuplespace_impl.read(test_context("", ""), wit_pattern).await;

        // ASSERT
        assert!(result.is_ok());
        let opt_tuple = result.unwrap();
        assert!(opt_tuple.is_some(), "should find matching tuple");
        let found_tuple = opt_tuple.unwrap();
        assert_eq!(found_tuple.len(), 2);
        match &found_tuple[0] {
            types::TupleField::StringVal(s) => assert_eq!(s, "read-test"),
            _ => panic!("Expected StringVal"),
        }
    }

    #[tokio::test]
    async fn test_tuplespace_impl_read_no_match() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider),
            actor_id,
        );

        // Create pattern that won't match anything
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("nonexistent".to_string())),
        ];

        // ACT
        let result = tuplespace_impl.read(test_context("", ""), wit_pattern).await;

        // ASSERT
        assert!(result.is_ok());
        let opt_tuple = result.unwrap();
        assert!(opt_tuple.is_none(), "should not find matching tuple");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_take() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Write a tuple first
        let tuple = Tuple::new(vec![
            TupleField::String("take-test".to_string()),
            TupleField::Integer(200),
        ]);
        tuplespace_provider.write(tuple).await.unwrap();

        // Create pattern in WIT format
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("take-test".to_string())),
            types::PatternField::Any,
        ];

        // ACT: Take the tuple
        let result = tuplespace_impl.take(test_context("", ""), wit_pattern.clone()).await;

        // ASSERT: Should get the tuple
        assert!(result.is_ok());
        let opt_tuple = result.unwrap();
        assert!(opt_tuple.is_some(), "should find and take tuple");

        // Verify tuple was removed (take again should return None)
        let result2 = tuplespace_impl.take(test_context("", ""), wit_pattern).await;
        assert!(result2.is_ok());
        assert!(result2.unwrap().is_none(), "tuple should be removed after take");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_count() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Write multiple tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![
                TupleField::String("count-test".to_string()),
                TupleField::Integer(i),
            ]);
            tuplespace_provider.write(tuple).await.unwrap();
        }

        // Create pattern in WIT format
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("count-test".to_string())),
            types::PatternField::Any,
        ];

        // ACT
        let result = tuplespace_impl.count(test_context("", ""), wit_pattern).await;

        // ASSERT
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5, "should count 5 matching tuples");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_read_all() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Write multiple tuples
        for i in 0..3 {
            let tuple = Tuple::new(vec![
                TupleField::String("read-all-test".to_string()),
                TupleField::Integer(i),
            ]);
            tuplespace_provider.write(tuple).await.unwrap();
        }

        // Create pattern in WIT format
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("read-all-test".to_string())),
            types::PatternField::Any,
        ];

        // ACT
        let result = tuplespace_impl.read_all(test_context("", ""), wit_pattern, 0).await; // 0 = no limit

        // ASSERT
        assert!(result.is_ok());
        let tuples = result.unwrap();
        assert_eq!(tuples.len(), 3, "should read all 3 tuples");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_read_all_with_limit() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        // Write multiple tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![
                TupleField::String("limit-test".to_string()),
                TupleField::Integer(i),
            ]);
            tuplespace_provider.write(tuple).await.unwrap();
        }

        // Create pattern in WIT format
        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("limit-test".to_string())),
            types::PatternField::Any,
        ];

        // ACT: Read with limit of 2
        let result = tuplespace_impl.read_all(test_context("", ""), wit_pattern, 2).await;

        // ASSERT
        assert!(result.is_ok());
        let tuples = result.unwrap();
        assert_eq!(tuples.len(), 2, "should respect limit");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_write_with_ttl() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider.clone()),
            actor_id.clone(),
        );

        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        let wit_tuple = vec![
            types::TupleField::StringVal("ttl-test".to_string()),
        ];

        // ACT: Write with TTL of 100ms
        let result = tuplespace_impl.write_with_ttl(test_context("", ""), wit_tuple, 100).await;

        // ASSERT
        assert!(result.is_ok(), "write_with_ttl should succeed");
        
        // Verify tuple exists
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("ttl-test".to_string())),
        ]);
        let read_result = tuplespace_provider.read(&pattern).await;
        assert!(read_result.is_ok());
        let tuples = read_result.unwrap();
        assert_eq!(tuples.len(), 1, "tuple should exist before TTL expires");
    }

    #[tokio::test]
    async fn test_tuplespace_type_conversion() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider),
            actor_id,
        );

        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        
        // Test all field types
        let wit_tuple = vec![
            types::TupleField::StringVal("string".to_string()),
            types::TupleField::IntVal(42),
            types::TupleField::FloatVal(3.14),
            types::TupleField::BytesVal(vec![1, 2, 3]),
            types::TupleField::BoolVal(true),
            types::TupleField::NullVal,
        ];

        // ACT: Convert to internal tuple
        let result = tuplespace_impl.convert_wit_tuple_to_internal(&wit_tuple);

        // ASSERT
        assert!(result.is_ok(), "type conversion should succeed");
        let tuple = result.unwrap();
        assert_eq!(tuple.fields().len(), 6);
        
        // Verify field types
        match tuple.fields()[0] {
            TupleField::String(ref s) => assert_eq!(s, "string"),
            _ => panic!("Expected String field"),
        }
        match tuple.fields()[1] {
            TupleField::Integer(i) => assert_eq!(i, 42),
            _ => panic!("Expected Integer field"),
        }
        match tuple.fields()[4] {
            TupleField::Boolean(b) => assert!(b),
            _ => panic!("Expected Boolean field"),
        }
        match tuple.fields()[5] {
            TupleField::Null => {},
            _ => panic!("Expected Null field"),
        }
    }

    #[tokio::test]
    async fn test_tuplespace_pattern_conversion() {
        // ARRANGE
        let tuplespace_provider = create_test_tuplespace();
        let actor_id = ActorId::from("test-actor".to_string());
        let tuplespace_impl = TuplespaceImpl::new(
            Some(tuplespace_provider),
            actor_id,
        );

        use plexspaces_wasm_runtime::generated::plexspaces_actor::plexspaces::actor::types;
        
        // Test pattern with exact, any, and typed fields
        let wit_pattern = vec![
            types::PatternField::Exact(types::TupleField::StringVal("exact".to_string())),
            types::PatternField::Any,
            types::PatternField::Typed(types::FieldType::IntType),
        ];

        // ACT: Convert to internal pattern
        let result = tuplespace_impl.convert_wit_pattern_to_internal(&wit_pattern);

        // ASSERT
        assert!(result.is_ok(), "pattern conversion should succeed");
        let pattern = result.unwrap();
        assert_eq!(pattern.fields().len(), 3);
    }
}

