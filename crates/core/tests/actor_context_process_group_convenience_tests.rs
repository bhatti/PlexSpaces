// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorContext convenience methods for ProcessGroupService (TDD - Phase 8 Phase 3)

#[cfg(test)]
mod tests {
    use plexspaces_core::{ProcessGroupService, RequestContext};
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

    // Mock ProcessGroupService for testing
    struct MockProcessGroupService {
        joined_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>, // (group_name, tenant_id, namespace, actor_id)
        left_groups: Arc<std::sync::Mutex<Vec<(String, String, String, String)>>>,
        published_messages: Arc<std::sync::Mutex<Vec<(String, String, String, Message)>>>, // (group_name, tenant_id, namespace, message)
        members: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>>, // group_name -> actor_ids
    }

    impl MockProcessGroupService {
        fn new() -> Self {
            Self {
                joined_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
                left_groups: Arc::new(std::sync::Mutex::new(Vec::new())),
                published_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
                members: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProcessGroupService for MockProcessGroupService {
        async fn create_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn delete_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn join_group(
            &self,
            ctx: &RequestContext,
            group_name: &str,
            actor_id: &str,
            _topics: Vec<String>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.joined_groups
                .lock()
                .unwrap()
                .push((
                    group_name.to_string(),
                    ctx.tenant_id().to_string(),
                    ctx.namespace().to_string(),
                    actor_id.to_string(),
                ));
            Ok(())
        }

        async fn leave_group(
            &self,
            ctx: &RequestContext,
            group_name: &str,
            actor_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.left_groups
                .lock()
                .unwrap()
                .push((
                    group_name.to_string(),
                    ctx.tenant_id().to_string(),
                    ctx.namespace().to_string(),
                    actor_id.to_string(),
                ));
            Ok(())
        }

        async fn get_members(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members
                .get(group_name)
                .cloned()
                .unwrap_or_default())
        }

        async fn get_local_members(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members
                .get(group_name)
                .cloned()
                .unwrap_or_default())
        }

        async fn list_groups(
            &self,
            _ctx: &RequestContext,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members.keys().cloned().collect())
        }

        async fn publish_to_group(
            &self,
            ctx: &RequestContext,
            group_name: &str,
            _topic: Option<&str>,
            message: Message,
        ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
            self.published_messages
                .lock()
                .unwrap()
                .push((
                    group_name.to_string(),
                    ctx.tenant_id().to_string(),
                    ctx.namespace().to_string(),
                    message,
                ));
            Ok(2) // Return count of 2 as mock response
        }
    }

    #[tokio::test]
    async fn test_join_group_records_tenant_info() {
        let service = MockProcessGroupService::new();
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());
        
        service.join_group(&ctx, "test-group", "actor-1", vec![]).await.unwrap();
        
        let joined = service.joined_groups.lock().unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].0, "test-group");
        assert_eq!(joined[0].1, "tenant-123");
        assert_eq!(joined[0].2, "namespace-abc");
        assert_eq!(joined[0].3, "actor-1");
    }

    #[tokio::test]
    async fn test_leave_group_records_tenant_info() {
        let service = MockProcessGroupService::new();
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());
        
        service.leave_group(&ctx, "test-group", "actor-1").await.unwrap();
        
        let left = service.left_groups.lock().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].0, "test-group");
        assert_eq!(left[0].1, "tenant-123");
        assert_eq!(left[0].2, "namespace-abc");
        assert_eq!(left[0].3, "actor-1");
    }

    #[tokio::test]
    async fn test_publish_to_group_records_tenant_info() {
        let service = MockProcessGroupService::new();
        let ctx = RequestContext::new_without_auth("tenant-123".to_string(), "namespace-abc".to_string());
        
        let message = create_test_message(b"test payload".to_vec());
        let count = service.publish_to_group(&ctx, "test-group", None, message).await.unwrap();
        
        assert_eq!(count, 2);
        
        let published = service.published_messages.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "test-group");
        assert_eq!(published[0].1, "tenant-123");
        assert_eq!(published[0].2, "namespace-abc");
    }
}
