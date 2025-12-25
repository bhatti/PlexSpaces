// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ProcessGroupService trait (TDD - Phase 8 Phase 3)

#[cfg(test)]
mod tests {
    use plexspaces_core::{ActorContext, ProcessGroupService};
    use plexspaces_mailbox::Message;
    use std::sync::Arc;

    // Mock ProcessGroupService for testing
    struct MockProcessGroupService {
        joined_groups: Arc<std::sync::Mutex<Vec<(String, String)>>>, // (group_name, actor_id)
        left_groups: Arc<std::sync::Mutex<Vec<(String, String)>>>,
        published_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>, // (group_name, message)
        members: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>, // group_name -> actor_ids
    }

    #[async_trait::async_trait]
    impl ProcessGroupService for MockProcessGroupService {
        async fn join_group(
            &self,
            group_name: &str,
            tenant_id: &str,
            namespace: &str,
            actor_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.joined_groups
                .lock()
                .unwrap()
                .push((group_name.to_string(), actor_id.to_string()));
            
            // Add to members
            let mut members = self.members.lock().unwrap();
            members
                .entry(group_name.to_string())
                .or_insert_with(Vec::new)
                .push(actor_id.to_string());
            
            Ok(())
        }

        async fn leave_group(
            &self,
            group_name: &str,
            tenant_id: &str,
            namespace: &str,
            actor_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.left_groups
                .lock()
                .unwrap()
                .push((group_name.to_string(), actor_id.to_string()));
            
            // Remove from members
            let mut members = self.members.lock().unwrap();
            if let Some(group_members) = members.get_mut(group_name) {
                group_members.retain(|id| id != actor_id);
            }
            
            Ok(())
        }

        async fn publish_to_group(
            &self,
            group_name: &str,
            tenant_id: &str,
            namespace: &str,
            message: Message,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            self.published_messages
                .lock()
                .unwrap()
                .push((group_name.to_string(), message));
            
            // Return current members
            let members = self.members.lock().unwrap();
            Ok(members
                .get(group_name)
                .cloned()
                .unwrap_or_default())
        }

        async fn get_members(
            &self,
            group_name: &str,
            tenant_id: &str,
            namespace: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members
                .get(group_name)
                .cloned()
                .unwrap_or_default())
        }
    }

    struct MockActorService;
    #[async_trait::async_trait]
    impl plexspaces_core::ActorService for MockActorService {
        async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        async fn send(&self, _actor_id: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
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
    impl plexspaces_core::ObjectRegistry for MockObjectRegistry {
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
}

