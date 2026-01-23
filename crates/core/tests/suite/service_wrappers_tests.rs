// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive tests for service_wrappers module

use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
use plexspaces_core::TupleSpaceProvider;
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField, TupleSpace, TupleSpaceError};
use std::sync::Arc;

#[tokio::test]
async fn test_tuplespace_provider_wrapper_new() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());
    
    // Wrapper should be created successfully - test by using it
    let tuple = Tuple::new(vec![
        TupleField::String("test".to_string()),
        TupleField::Integer(42),
    ]);
    let result = wrapper.write(tuple).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_write() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    let tuple = Tuple::new(vec![
        TupleField::String("test".to_string()),
        TupleField::Integer(42),
    ]);

    let result = wrapper.write(tuple.clone()).await;
    assert!(result.is_ok());

    // Verify tuple was written
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("test".to_string())),
        PatternField::Wildcard,
    ]);
    let results = tuplespace.read_all(pattern).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_read() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Write a tuple first
    let tuple = Tuple::new(vec![
        TupleField::String("read-test".to_string()),
        TupleField::Integer(100),
    ]);
    tuplespace.write(tuple.clone()).await.unwrap();

    // Read using wrapper
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("read-test".to_string())),
        PatternField::Wildcard,
    ]);
    let results = wrapper.read(&pattern).await;

    assert!(results.is_ok());
    let tuples = results.unwrap();
    assert_eq!(tuples.len(), 1);
    assert_eq!(tuples[0].fields()[0], TupleField::String("read-test".to_string()));
    assert_eq!(tuples[0].fields()[1], TupleField::Integer(100));
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_read_empty() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("non-existent".to_string())),
        PatternField::Wildcard,
    ]);
    let results = wrapper.read(&pattern).await;

    assert!(results.is_ok());
    let tuples = results.unwrap();
    assert_eq!(tuples.len(), 0);
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_take() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Write a tuple first
    let tuple = Tuple::new(vec![
        TupleField::String("take-test".to_string()),
        TupleField::Integer(200),
    ]);
    tuplespace.write(tuple.clone()).await.unwrap();

    // Take using wrapper
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("take-test".to_string())),
        PatternField::Wildcard,
    ]);
    let result = wrapper.take(&pattern).await;

    assert!(result.is_ok());
    let taken = result.unwrap();
    assert!(taken.is_some());
    assert_eq!(taken.unwrap().fields()[0], TupleField::String("take-test".to_string()));

    // Verify tuple was removed
    let results = tuplespace.read_all(pattern.clone()).await.unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_take_empty() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("non-existent".to_string())),
        PatternField::Wildcard,
    ]);
    let result = wrapper.take(&pattern).await;

    assert!(result.is_ok());
    let taken = result.unwrap();
    assert!(taken.is_none());
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_count() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Write multiple tuples
    for i in 0..5 {
        let tuple = Tuple::new(vec![
            TupleField::String("count-test".to_string()),
            TupleField::Integer(i),
        ]);
        tuplespace.write(tuple).await.unwrap();
    }

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("count-test".to_string())),
        PatternField::Wildcard,
    ]);
    let count = wrapper.count(&pattern).await;

    assert!(count.is_ok());
    assert_eq!(count.unwrap(), 5);
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_count_zero() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("non-existent".to_string())),
        PatternField::Wildcard,
    ]);
    let count = wrapper.count(&pattern).await;

    assert!(count.is_ok());
    assert_eq!(count.unwrap(), 0);
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper_multiple_operations() {
    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Write
    let tuple1 = Tuple::new(vec![
        TupleField::String("multi-test".to_string()),
        TupleField::Integer(1),
    ]);
    wrapper.write(tuple1).await.unwrap();

    // Read
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("multi-test".to_string())),
        PatternField::Wildcard,
    ]);
    let count1 = wrapper.count(&pattern).await.unwrap();
    assert_eq!(count1, 1);

    // Write another
    let tuple2 = Tuple::new(vec![
        TupleField::String("multi-test".to_string()),
        TupleField::Integer(2),
    ]);
    wrapper.write(tuple2).await.unwrap();

    // Count again
    let count2 = wrapper.count(&pattern).await.unwrap();
    assert_eq!(count2, 2);

    // Take one
    let taken = wrapper.take(&pattern).await.unwrap();
    assert!(taken.is_some());

    // Count should be 1 now
    let count3 = wrapper.count(&pattern).await.unwrap();
    assert_eq!(count3, 1);
}

