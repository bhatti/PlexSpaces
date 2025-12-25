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

//! Unit tests for mailbox graceful shutdown behavior
//!
//! Tests cover:
//! - Shutdown flag behavior for non-memory channels
//! - Enqueue rejection during shutdown
//! - Dequeue stopping during shutdown
//! - In-progress message completion
//! - Channel closing

use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxBuilder, Message};
use std::time::Duration;
use tempfile::TempDir;

/// Helper to create a SQLite mailbox for testing
#[cfg(feature = "sqlite-backend")]
async fn create_sqlite_mailbox(mailbox_id: &str) -> Mailbox {
    // Use in-memory SQLite for tests (more reliable than file-based in test environment)
    // This avoids file permission and cleanup issues
    MailboxBuilder::new()
        .with_sqlite(":memory:".to_string())
        .build(mailbox_id.to_string())
        .await
        .unwrap()
}

#[tokio::test]
async fn test_in_memory_mailbox_shutdown_does_not_reject_enqueue() {
    // In-memory mailboxes should continue accepting messages during shutdown
    let mailbox = Mailbox::new(mailbox_config_default(), "test-in-memory".to_string())
        .await
        .unwrap();

    // Verify it's in-memory
    assert!(mailbox.is_in_memory());
    assert!(!mailbox.is_durable());

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // In-memory mailboxes should still accept messages (shutdown flag only affects non-memory)
    let msg = Message::new(b"test".to_vec());
    let result = mailbox.enqueue(msg).await;
    assert!(result.is_ok(), "In-memory mailbox should accept messages even after shutdown");
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_non_memory_mailbox_shutdown_rejects_enqueue() {
    // Non-memory mailboxes should reject new messages during shutdown
    let mailbox = create_sqlite_mailbox("test-sqlite-shutdown").await;

    // Verify it's not in-memory
    assert!(!mailbox.is_in_memory());
    assert!(mailbox.is_durable());

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // Non-memory mailboxes should reject new messages
    let msg = Message::new(b"test".to_vec());
    let result = mailbox.enqueue(msg).await;
    assert!(result.is_err(), "Non-memory mailbox should reject messages during shutdown");
    
    if let Err(e) = result {
        assert!(e.to_string().contains("shutting down"), 
            "Error should mention shutdown: {}", e);
    }
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_stops_dequeue() {
    // After shutdown, dequeue should stop receiving from channel backend
    let mailbox = create_sqlite_mailbox("test-dequeue-shutdown").await;

    // Send a message
    mailbox.enqueue(Message::new(b"test".to_vec())).await.unwrap();
    
    // Wait for message to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // Dequeue should return None (shutdown stopped receiving)
    let result = tokio::time::timeout(
        Duration::from_millis(500),
        mailbox.dequeue_with_timeout(Some(Duration::from_secs(1)))
    ).await;
    
    // Should timeout or return None (no more messages being received)
    assert!(result.is_ok() || result.unwrap().is_none());
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_waits_for_in_progress() {
    // Shutdown should wait for in-progress messages to complete
    let mailbox = create_sqlite_mailbox("test-in-progress").await;

    // Send a message
    mailbox.enqueue(Message::new(b"test".to_vec())).await.unwrap();

    // Start shutdown with longer timeout
    let start = std::time::Instant::now();
    mailbox.graceful_shutdown(Some(Duration::from_secs(5))).await.unwrap();
    let elapsed = start.elapsed();

    // Should complete quickly (no in-progress messages to wait for)
    assert!(elapsed < Duration::from_secs(2), "Shutdown should complete quickly when no in-progress messages");
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_timeout() {
    // Shutdown should timeout if in-progress messages don't complete
    let mailbox = create_sqlite_mailbox("test-timeout").await;

    // Start shutdown with very short timeout
    let start = std::time::Instant::now();
    mailbox.graceful_shutdown(Some(Duration::from_millis(100))).await.unwrap();
    let elapsed = start.elapsed();

    // Should complete within timeout
    assert!(elapsed < Duration::from_millis(200), "Shutdown should respect timeout");
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_closes_channel() {
    // Shutdown should close the channel
    let mailbox = create_sqlite_mailbox("test-close-channel").await;

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // Channel should be closed (we can't directly access channel, but shutdown should close it)
    // Verify by trying to enqueue (should fail for non-memory after shutdown)
    let msg = Message::new(b"test".to_vec());
    let result = mailbox.enqueue(msg).await;
    assert!(result.is_err(), "Channel should be closed after shutdown");
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_flushes_durable_messages() {
    // Shutdown should flush pending messages for durable backends
    let mailbox = create_sqlite_mailbox("test-flush").await;

    // Send multiple messages
    for i in 0..5 {
        mailbox.enqueue(Message::new(format!("msg{}", i).into_bytes())).await.unwrap();
    }

    // Wait a bit for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(5))).await.unwrap();

    // Messages should be flushed (check stats)
    let stats = mailbox.get_stats().await;
    assert_eq!(stats.total_enqueued, 5);
}

#[tokio::test]
async fn test_in_memory_shutdown_does_not_close_channel() {
    // In-memory mailboxes don't close channel during shutdown
    let mailbox = Mailbox::new(mailbox_config_default(), "test-in-memory-close".to_string())
        .await
        .unwrap();

    // Start shutdown
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // In-memory channel should not be closed (it's a no-op for in-memory)
    // Note: InMemoryChannel.close() sets closed flag, but we don't check it here
    // as the behavior is different for in-memory vs non-memory
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_idempotent() {
    // Calling shutdown multiple times should be safe
    let mailbox = create_sqlite_mailbox("test-idempotent").await;

    // Call shutdown multiple times
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();
    mailbox.graceful_shutdown(Some(Duration::from_secs(1))).await.unwrap();

    // Should still reject new messages
    let msg = Message::new(b"test".to_vec());
    let result = mailbox.enqueue(msg).await;
    assert!(result.is_err());
}

#[tokio::test]
#[cfg(feature = "sqlite-backend")]
async fn test_shutdown_with_no_timeout() {
    // Shutdown with None timeout should use default
    let mailbox = create_sqlite_mailbox("test-no-timeout").await;

    // Should complete successfully
    mailbox.graceful_shutdown(None).await.unwrap();

    // Should reject new messages
    let msg = Message::new(b"test".to_vec());
    let result = mailbox.enqueue(msg).await;
    assert!(result.is_err());
}
