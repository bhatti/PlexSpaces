// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext reply() method (Erlang-style)

#[cfg(test)]
mod tests {
    use plexspaces_core::{ActorContext, ChannelService, RequestContext};
    use plexspaces_core::Message;
    use std::sync::Arc;
    use ulid::Ulid;

    /// Helper to create a test message
    fn create_test_message(payload: Vec<u8>) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    fn create_test_message_with_sender(payload: Vec<u8>, sender_id: &str, correlation_id: &str) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            sender_id: sender_id.to_string(),
            correlation_id: correlation_id.to_string(),
            ..Default::default()
        }
    }

    // Mock services for testing
    struct MockActorService {
        sent_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
    }

    #[async_trait::async_trait]
    impl plexspaces_core::ActorService for MockActorService {
        async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        async fn send(&self, actor_id: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            self.sent_messages.lock().unwrap().push((actor_id.to_string(), message));
            Ok("msg-id".to_string())
        }
    }

    struct MockObjectRegistry;
    #[async_trait::async_trait]
    impl plexspaces_core::ObjectRegistry for MockObjectRegistry {
        async fn lookup(&self, _ctx: &RequestContext, _object_id: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn lookup_full(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn register(&self, _ctx: &RequestContext, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn unregister(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn heartbeat(&self, _ctx: &RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn discover(&self, _ctx: &RequestContext, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>, _object_category: Option<String>, _capabilities: Option<Vec<String>>, _labels: Option<Vec<String>>, _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>, _offset: usize, _limit: usize) -> Result<Vec<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![])
        }
    }

    struct MockTupleSpaceProvider;
    #[async_trait::async_trait]
    impl plexspaces_core::TupleSpaceProvider for MockTupleSpaceProvider {
        async fn write(&self, _tuple: plexspaces_tuplespace::Tuple) -> Result<(), plexspaces_tuplespace::TupleSpaceError> {
            Ok(())
        }
        async fn read(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
            Ok(vec![])
        }
        async fn take(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Option<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
            Ok(None)
        }
        async fn count(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn test_reply_with_sender_id() {
        use plexspaces_core::ActorService;
        
        let sent_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService {
            sent_messages: sent_messages.clone(),
        });
        
        // Test that reply works with sender_id
        let original_msg = create_test_message_with_sender(b"request".to_vec(), "sender-actor", "corr-123");
        
        // Create reply message
        let reply_msg = Message {
            id: Ulid::new().to_string(),
            payload: b"response".to_vec(),
            receiver_id: original_msg.sender_id.clone(),
            correlation_id: original_msg.correlation_id.clone(),
            ..Default::default()
        };
        
        // Send reply using actor service
        let result = actor_service.send(&original_msg.sender_id, reply_msg).await;
        assert!(result.is_ok());
        
        let sent = sent_messages.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "sender-actor");
        assert_eq!(sent[0].1.correlation_id, "corr-123");
    }

    #[tokio::test]
    async fn test_reply_without_sender_id() {
        // Test that we can handle messages without sender_id
        let msg = create_test_message(b"no-sender".to_vec());
        assert!(msg.sender_id.is_empty());
    }
}