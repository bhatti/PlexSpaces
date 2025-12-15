// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext reply() method (Erlang-style)

#[cfg(test)]
mod tests {
    use plexspaces_core::{ActorContext, ChannelService};
    use plexspaces_mailbox::Message;
    use std::sync::Arc;

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
        async fn lookup(&self, _tenant_id: &str, _object_id: &str, _namespace: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn lookup_full(&self, _tenant_id: &str, _namespace: &str, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn register(&self, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
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

