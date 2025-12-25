// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ChannelService trait and integration with ActorContext

#[cfg(test)]
mod tests {
    use plexspaces_mailbox::Message;
    use std::sync::Arc;
    use futures::StreamExt;
    use async_trait::async_trait;
    use plexspaces_core::{ChannelService, ActorService, ObjectRegistry, TupleSpaceProvider};

    // Test ChannelService implementation
    struct TestChannelService;
    
    #[async_trait::async_trait]
    impl plexspaces_core::ChannelService for TestChannelService {
        async fn send_to_queue(&self, _queue_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
        }
        async fn publish_to_topic(&self, _topic_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
        }
        async fn subscribe_to_topic(&self, _topic_name: &str) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }
        async fn receive_from_queue(&self, _queue_name: &str, _timeout: Option<std::time::Duration>) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
    }

    // Mock services for testing
    struct MockActorService;
    #[async_trait::async_trait]
    impl ActorService for MockActorService {
        async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        async fn send(&self, _actor_id: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        async fn send_reply(
            &self,
            _correlation_id: Option<&str>,
            _sender_id: &plexspaces_core::ActorId,
            _target_actor_id: plexspaces_core::ActorId,
            _reply_message: Message,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    struct MockObjectRegistry;
    #[async_trait::async_trait]
    impl ObjectRegistry for MockObjectRegistry {
        async fn lookup(&self, _ctx: &plexspaces_core::RequestContext, _object_id: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn lookup_full(&self, _ctx: &plexspaces_core::RequestContext, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn register(&self, _ctx: &plexspaces_core::RequestContext, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    struct MockTupleSpaceProvider;
    #[async_trait::async_trait]
    impl TupleSpaceProvider for MockTupleSpaceProvider {
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
}

