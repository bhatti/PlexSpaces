// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive tests for ActorError to improve coverage

use plexspaces_core::ActorError;
use plexspaces_persistence::JournalError;

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
fn test_actor_error_from_journal_error_io() {
    let journal_error = JournalError::IoError("io error".to_string());
    let actor_error: ActorError = journal_error.into();
    
    match actor_error {
        ActorError::JournalError(msg) => {
            assert!(msg.contains("io error"));
        },
        _ => panic!("Expected JournalError"),
    }
}

#[test]
fn test_actor_error_from_journal_error_serialization() {
    let journal_error = JournalError::SerializationError("serialization error".to_string());
    let actor_error: ActorError = journal_error.into();
    
    match actor_error {
        ActorError::JournalError(msg) => {
            assert!(msg.contains("serialization error"));
        },
        _ => panic!("Expected JournalError"),
    }
}

#[test]
fn test_actor_error_from_journal_error_sequence_mismatch() {
    let journal_error = JournalError::SequenceMismatch {
        expected: 10,
        actual: 5,
    };
    let actor_error: ActorError = journal_error.into();
    
    match actor_error {
        ActorError::JournalError(msg) => {
            assert!(!msg.is_empty());
        },
        _ => panic!("Expected JournalError"),
    }
}

#[test]
fn test_actor_error_from_journal_error_not_found() {
    let journal_error = JournalError::NotFound;
    let actor_error: ActorError = journal_error.into();
    
    match actor_error {
        ActorError::JournalError(msg) => {
            assert!(!msg.is_empty());
        },
        _ => panic!("Expected JournalError"),
    }
}

#[test]
fn test_actor_error_from_journal_error_corrupted() {
    let journal_error = JournalError::Corrupted("corrupted data".to_string());
    let actor_error: ActorError = journal_error.into();
    
    match actor_error {
        ActorError::JournalError(msg) => {
            assert!(msg.contains("corrupted data"));
        },
        _ => panic!("Expected JournalError"),
    }
}

