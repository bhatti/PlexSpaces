// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for idempotency key deduplication and LRU cache

use plexspaces_mailbox::{mailbox_config_default, Mailbox, Message};
use prost_types::Duration as ProtoDuration;
use std::time::Duration;

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    let proto_msg = plexspaces_actor::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    };
    proto_msg.into()
}

/// Helper to create a test message with idempotency key
fn create_test_message_with_idempotency(payload: Vec<u8>, idempotency_key: String) -> Message {
    let proto_msg = plexspaces_actor::Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        idempotency_key,
        ..Default::default()
    };
    proto_msg.into()
}

#[tokio::test]
async fn test_idempotency_key_deduplication() {
    // Test: Messages with same idempotency key are deduplicated
    let mailbox = Mailbox::new(mailbox_config_default(), "test-mailbox".to_string())
        .await
        .unwrap();

    let idempotency_key = "test-key-123".to_string();

    // Send first message with idempotency key
    let msg1 = create_test_message_with_idempotency(b"first".to_vec(), idempotency_key.clone());
    mailbox.enqueue(msg1).await.unwrap();

    // Process first message to add idempotency key to cache
    let msg1_dequeued = mailbox.dequeue().await.unwrap();
    assert_eq!(msg1_dequeued.payload, b"first");

    // Send second message with same idempotency key (should be deduplicated)
    let msg2 = create_test_message_with_idempotency(b"second".to_vec(), idempotency_key.clone());
    mailbox.enqueue(msg2).await.unwrap();

    // Second message should be deduplicated (mailbox should be empty)
    // Note: Idempotency deduplication happens on enqueue, so message is skipped
    let result = tokio::time::timeout(Duration::from_millis(100), mailbox.dequeue()).await;
    assert!(result.is_err()); // Timeout = no message (deduplicated)
}

#[tokio::test]
async fn test_idempotency_key_different_keys() {
    // Test: Messages with different idempotency keys are not deduplicated
    let mailbox = Mailbox::new(mailbox_config_default(), "test-mailbox-2".to_string())
        .await
        .unwrap();

    // Send messages with different idempotency keys
    let msg1 = create_test_message_with_idempotency(b"first".to_vec(), "key-1".to_string());
    mailbox.enqueue(msg1).await.unwrap();

    let msg2 = create_test_message_with_idempotency(b"second".to_vec(), "key-2".to_string());
    mailbox.enqueue(msg2).await.unwrap();

    // Both messages should be in mailbox
    let msg1 = mailbox.dequeue().await.unwrap();
    assert_eq!(msg1.payload, b"first");

    let msg2 = mailbox.dequeue().await.unwrap();
    assert_eq!(msg2.payload, b"second");
}

#[tokio::test]
async fn test_idempotency_key_without_key() {
    // Test: Messages without idempotency key are not deduplicated
    let mailbox = Mailbox::new(mailbox_config_default(), "test-mailbox-3".to_string())
        .await
        .unwrap();

    // Send messages without idempotency key
    let msg1 = create_test_message(b"first".to_vec());
    mailbox.enqueue(msg1).await.unwrap();

    let msg2 = create_test_message(b"second".to_vec());
    mailbox.enqueue(msg2).await.unwrap();

    // Both messages should be in mailbox
    let msg1 = mailbox.dequeue().await.unwrap();
    assert_eq!(msg1.payload, b"first");

    let msg2 = mailbox.dequeue().await.unwrap();
    assert_eq!(msg2.payload, b"second");
}

#[tokio::test]
async fn test_idempotency_key_lru_eviction() {
    // Test: LRU cache evicts old entries when full
    let mut config = mailbox_config_default();
    config.idempotency_cache_size = 2; // Small cache for testing

    let mailbox = Mailbox::new(config, "test-mailbox-4".to_string())
        .await
        .unwrap();

    // Fill cache with 2 entries
    let msg1 = create_test_message_with_idempotency(b"first".to_vec(), "key-1".to_string());
    mailbox.enqueue(msg1).await.unwrap();
    let _ = mailbox.dequeue().await; // Process to add to cache

    let msg2 = create_test_message_with_idempotency(b"second".to_vec(), "key-2".to_string());
    mailbox.enqueue(msg2).await.unwrap();
    let _ = mailbox.dequeue().await; // Process to add to cache

    // Add third entry (should evict first)
    let msg3 = create_test_message_with_idempotency(b"third".to_vec(), "key-3".to_string());
    mailbox.enqueue(msg3).await.unwrap();
    let _ = mailbox.dequeue().await; // Process to add to cache

    // Now key-1 should be evicted, so we can send it again
    let msg1_again =
        create_test_message_with_idempotency(b"first-again".to_vec(), "key-1".to_string());
    mailbox.enqueue(msg1_again).await.unwrap();

    // Should be able to dequeue (not deduplicated because evicted)
    let msg = mailbox.dequeue().await.unwrap();
    assert_eq!(msg.payload, b"first-again");
}

#[tokio::test]
async fn test_idempotency_key_expiration() {
    // Test: Idempotency keys expire after deduplication window
    let mut config = mailbox_config_default();
    config.deduplication_window = Some(ProtoDuration {
        seconds: 0,
        nanos: 100_000_000, // 100ms
    });

    let mailbox = Mailbox::new(config, "test-mailbox-5".to_string())
        .await
        .unwrap();

    let idempotency_key = "expiring-key".to_string();

    // Send first message
    let msg1 = create_test_message_with_idempotency(b"first".to_vec(), idempotency_key.clone());
    mailbox.enqueue(msg1).await.unwrap();
    let _ = mailbox.dequeue().await; // Process

    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Send second message with same key (should NOT be deduplicated after expiration)
    let msg2 = create_test_message_with_idempotency(b"second".to_vec(), idempotency_key.clone());
    mailbox.enqueue(msg2).await.unwrap();

    // Should be able to dequeue (not deduplicated because expired)
    let msg = mailbox.dequeue().await.unwrap();
    assert_eq!(msg.payload, b"second");
}
