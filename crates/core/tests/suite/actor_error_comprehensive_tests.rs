// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive tests for ActorError to improve coverage

use plexspaces_core::ActorError;

#[test]
fn test_actor_error_mailbox_error() {
    let error = ActorError::MailboxError("mailbox full".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("mailbox full"));
}

#[test]
fn test_actor_error_behavior_error() {
    let error = ActorError::BehaviorError("behavior failed".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("behavior failed"));
}

#[test]
fn test_actor_error_journal_error() {
    let error = ActorError::JournalError("journal failed".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("journal failed"));
}

#[test]
fn test_actor_error_not_found() {
    let error = ActorError::NotFound("actor not found".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("actor not found"));
}

#[test]
fn test_actor_error_already_exists() {
    let error = ActorError::AlreadyExists("actor exists".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("actor exists"));
}

#[test]
fn test_actor_error_invalid_state() {
    let error = ActorError::InvalidState("invalid state".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("invalid state"));
}

#[test]
fn test_actor_error_no_behavior_to_restore() {
    let error = ActorError::NoBehaviorToRestore;
    let error_msg = error.to_string();
    assert!(!error_msg.is_empty());
}

#[test]
fn test_actor_error_timeout() {
    let error = ActorError::Timeout;
    let error_msg = error.to_string();
    assert!(!error_msg.is_empty());
}

#[test]
fn test_actor_error_facet_error() {
    let error = ActorError::FacetError("facet failed".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("facet failed"));
}

#[test]
fn test_actor_error_journal_io_error() {
    // Test JournalError with various io error messages
    let error = ActorError::JournalError("io error: failed to read file".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("io error"));
}

#[test]
fn test_actor_error_journal_serialization_error() {
    // Test JournalError with serialization error messages
    let error = ActorError::JournalError("serialization error: invalid format".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("serialization error"));
}

#[test]
fn test_actor_error_journal_sequence_mismatch() {
    // Test JournalError with sequence mismatch messages
    let error = ActorError::JournalError("sequence mismatch: expected 10, got 5".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("sequence mismatch"));
}

#[test]
fn test_actor_error_journal_not_found() {
    // Test JournalError with not found messages
    let error = ActorError::JournalError("journal not found".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("not found"));
}

#[test]
fn test_actor_error_journal_corrupted() {
    // Test JournalError with corrupted data messages
    let error = ActorError::JournalError("corrupted data: checksum mismatch".to_string());
    let error_msg = error.to_string();
    assert!(error_msg.contains("corrupted"));
}

#[test]
fn test_actor_error_debug_format() {
    let error = ActorError::NotFound("test".to_string());
    let debug_str = format!("{:?}", error);
    assert!(debug_str.contains("NotFound"));
}

#[test]
fn test_actor_error_partial_eq() {
    let error1 = ActorError::Timeout;
    let error2 = ActorError::Timeout;
    // Both have the same string representation
    assert_eq!(error1.to_string(), error2.to_string());
}
