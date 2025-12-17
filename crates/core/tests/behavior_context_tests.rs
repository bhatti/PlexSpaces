// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for BehaviorContext and related types

use plexspaces_core::{ActorContext, BehaviorContext, BehaviorType, BehaviorError};
use plexspaces_mailbox::Message;
use std::sync::Arc;

#[tokio::test]
async fn test_behavior_context_creation() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(), // namespace
        "test-tenant".to_string(), // tenant_id (required)
    );
    let ctx_arc = Arc::new(ctx);

    let message = Message::new(vec![1, 2, 3]);
    let behavior_ctx = BehaviorContext {
        actor_context: ctx_arc.clone(),
        message: message.clone(),
        sender: None,
        correlation_id: None,
        #[cfg(feature = "tracing")]
        span: None,
    };

    // actor_id removed from ActorContext
    assert_eq!(behavior_ctx.message.payload(), message.payload());
    assert!(behavior_ctx.sender.is_none());
    assert_eq!(behavior_ctx.correlation_id, None);
}

#[tokio::test]
async fn test_behavior_context_with_sender() {
    use plexspaces_core::ActorRef;
    use plexspaces_mailbox::{Mailbox, mailbox_config_default};

    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(), // namespace
        "test-tenant".to_string(), // tenant_id (required)
    );
    let ctx_arc = Arc::new(ctx);

    let sender_mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "sender@node1".to_string()).await.expect("Failed to create mailbox"));
    let sender = ActorRef::new("sender@node1".to_string()).unwrap();

    let message = Message::new(vec![1, 2, 3]);
    let behavior_ctx = BehaviorContext {
        actor_context: ctx_arc.clone(),
        message: message.clone(),
        sender: Some(sender),
        correlation_id: Some("corr-123".to_string()),
        #[cfg(feature = "tracing")]
        span: None,
    };

    assert!(behavior_ctx.sender.is_some());
    assert_eq!(behavior_ctx.sender.as_ref().unwrap().id(), "sender@node1");
    assert_eq!(behavior_ctx.correlation_id, Some("corr-123".to_string()));
}

#[tokio::test]
async fn test_behavior_type_variants() {
    let types = vec![
        BehaviorType::GenServer,
        BehaviorType::GenEvent,
        BehaviorType::GenStateMachine,
        BehaviorType::Workflow,
        BehaviorType::Custom("custom-behavior".to_string()),
    ];

    for behavior_type in types {
        match behavior_type {
            BehaviorType::GenServer => assert_eq!(behavior_type, BehaviorType::GenServer),
            BehaviorType::GenEvent => assert_eq!(behavior_type, BehaviorType::GenEvent),
            BehaviorType::GenStateMachine => assert_eq!(behavior_type, BehaviorType::GenStateMachine),
            BehaviorType::Workflow => assert_eq!(behavior_type, BehaviorType::Workflow),
            BehaviorType::Custom(ref s) => assert_eq!(s, "custom-behavior"),
        }
    }
}

#[tokio::test]
async fn test_behavior_type_serialization() {
    use serde_json;

    let types = vec![
        BehaviorType::GenServer,
        BehaviorType::GenEvent,
        BehaviorType::GenStateMachine,
        BehaviorType::Workflow,
        BehaviorType::Custom("custom".to_string()),
    ];

    for behavior_type in types {
        let serialized = serde_json::to_string(&behavior_type).unwrap();
        let deserialized: BehaviorType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(behavior_type, deserialized);
    }
}

#[tokio::test]
async fn test_behavior_error_display() {
    let errors = vec![
        BehaviorError::HandlerNotFound("handler1".to_string()),
        BehaviorError::UnsupportedMessage,
        BehaviorError::TransitionFailed("state1".to_string()),
        BehaviorError::ProcessingError("error1".to_string()),
    ];

    for error in errors {
        let error_msg = error.to_string();
        assert!(!error_msg.is_empty());
    }
}

#[tokio::test]
async fn test_behavior_error_handler_not_found() {
    let error = BehaviorError::HandlerNotFound("handler1".to_string());
    assert!(error.to_string().contains("handler1"));
}

#[tokio::test]
async fn test_behavior_error_transition_failed() {
    let error = BehaviorError::TransitionFailed("state1".to_string());
    assert!(error.to_string().contains("state1"));
}

#[tokio::test]
async fn test_behavior_error_processing_error() {
    let error = BehaviorError::ProcessingError("error1".to_string());
    assert!(error.to_string().contains("error1"));
}

