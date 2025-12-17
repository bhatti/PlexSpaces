// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext ChannelService integration (TDD - Phase 2)

#[cfg(test)]
mod tests {
    use plexspaces_core::{ActorContext, ChannelService, ActorService, ObjectRegistry, TupleSpaceProvider};
    use plexspaces_mailbox::Message;
    use std::sync::Arc;
    use std::time::Duration;
    use futures::StreamExt;
    use async_trait::async_trait;

    // Mock ChannelService for testing
    struct MockChannelService {
        sent_to_queue: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
        published_to_topic: Arc<std::sync::Mutex<Vec<(String, Message)>>>,
        queue_messages: Arc<std::sync::Mutex<std::collections::VecDeque<Message>>>,
    }

    #[async_trait::async_trait]
    impl ChannelService for MockChannelService {
        async fn send_to_queue(
            &self,
            queue_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            self.sent_to_queue
                .lock()
                .unwrap()
                .push((queue_name.to_string(), message));
            Ok("msg-id".to_string())
        }

        async fn publish_to_topic(
            &self,
            topic_name: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            self.published_to_topic
                .lock()
                .unwrap()
                .push((topic_name.to_string(), message));
            Ok("msg-id".to_string())
        }

        async fn subscribe_to_topic(
            &self,
            _topic_name: &str,
        ) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            // Return empty stream for now
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }

        async fn receive_from_queue(
            &self,
            _queue_name: &str,
            _timeout: Option<Duration>,
        ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            let mut queue = self.queue_messages.lock().unwrap();
            Ok(queue.pop_front())
        }
    }

    struct MockActorService;

    #[async_trait::async_trait]
    impl plexspaces_core::ActorService for MockActorService {
        async fn spawn_actor(
            &self,
            _actor_id: &str,
            _actor_type: &str,
            _initial_state: Vec<u8>,
        ) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }

        async fn send(
            &self,
            _actor_id: &str,
            _message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
    }

    struct MockObjectRegistry;

    #[async_trait::async_trait]
    impl plexspaces_core::ObjectRegistry for MockObjectRegistry {
        async fn lookup(
            &self,
            _tenant_id: &str,
            _object_id: &str,
            _namespace: &str,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn lookup_full(
            &self,
            _tenant_id: &str,
            _namespace: &str,
            _object_type: plexspaces_proto::object_registry::v1::ObjectType,
            _object_id: &str,
        ) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn register(
            &self,
            _registration: plexspaces_core::ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    struct MockTupleSpaceProvider;

    #[async_trait::async_trait]
    impl plexspaces_core::TupleSpaceProvider for MockTupleSpaceProvider {
        async fn write(
            &self,
            _tuple: plexspaces_tuplespace::Tuple,
        ) -> Result<(), plexspaces_tuplespace::TupleSpaceError> {
            Ok(())
        }

        async fn read(
            &self,
            _pattern: &plexspaces_tuplespace::Pattern,
        ) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError>
        {
            Ok(vec![])
        }

        async fn take(
            &self,
            _pattern: &plexspaces_tuplespace::Pattern,
        ) -> Result<
            Option<plexspaces_tuplespace::Tuple>,
            plexspaces_tuplespace::TupleSpaceError,
        > {
            Ok(None)
        }

        async fn count(
            &self,
            _pattern: &plexspaces_tuplespace::Pattern,
        ) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
            Ok(0)
        }
    }
}

