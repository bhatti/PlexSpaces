// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ProcessGroupService trait (TDD - Phase 8 Phase 3)

#[cfg(test)]
mod tests {
    use plexspaces_core::Message;
    use plexspaces_core::{ProcessGroupService, RequestContext};
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
        joined_groups: Arc<std::sync::Mutex<Vec<(String, String)>>>, // (group_name, actor_id)
        left_groups: Arc<std::sync::Mutex<Vec<(String, String)>>>,
        published_messages: Arc<std::sync::Mutex<Vec<(String, Message)>>>, // (group_name, message)
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
            _ctx: &RequestContext,
            group_name: &str,
            actor_id: &str,
            _topics: Vec<String>,
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
            _ctx: &RequestContext,
            group_name: &str,
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

        async fn get_members(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members.get(group_name).cloned().unwrap_or_default())
        }

        async fn get_local_members(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let members = self.members.lock().unwrap();
            Ok(members.get(group_name).cloned().unwrap_or_default())
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
            _ctx: &RequestContext,
            group_name: &str,
            _topic: Option<&str>,
            message: Message,
        ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
            self.published_messages
                .lock()
                .unwrap()
                .push((group_name.to_string(), message));

            // Return count of members
            let members = self.members.lock().unwrap();
            Ok(members.get(group_name).map(|v| v.len() as u32).unwrap_or(0))
        }
    }

    #[tokio::test]
    async fn test_mock_join_group() {
        let service = MockProcessGroupService::new();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        service
            .join_group(&ctx, "test-group", "actor-1", vec![])
            .await
            .unwrap();

        let joined = service.joined_groups.lock().unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0], ("test-group".to_string(), "actor-1".to_string()));
    }

    #[tokio::test]
    async fn test_mock_leave_group() {
        let service = MockProcessGroupService::new();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        service
            .join_group(&ctx, "test-group", "actor-1", vec![])
            .await
            .unwrap();
        service
            .leave_group(&ctx, "test-group", "actor-1")
            .await
            .unwrap();

        let left = service.left_groups.lock().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0], ("test-group".to_string(), "actor-1".to_string()));

        let members = service.get_members(&ctx, "test-group").await.unwrap();
        assert!(members.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_members() {
        let service = MockProcessGroupService::new();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        service
            .join_group(&ctx, "test-group", "actor-1", vec![])
            .await
            .unwrap();
        service
            .join_group(&ctx, "test-group", "actor-2", vec![])
            .await
            .unwrap();

        let members = service.get_members(&ctx, "test-group").await.unwrap();
        assert_eq!(members.len(), 2);
        assert!(members.contains(&"actor-1".to_string()));
        assert!(members.contains(&"actor-2".to_string()));
    }

    #[tokio::test]
    async fn test_mock_publish_to_group() {
        let service = MockProcessGroupService::new();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        service
            .join_group(&ctx, "test-group", "actor-1", vec![])
            .await
            .unwrap();
        service
            .join_group(&ctx, "test-group", "actor-2", vec![])
            .await
            .unwrap();

        let message = create_test_message(b"test payload".to_vec());
        let count = service
            .publish_to_group(&ctx, "test-group", None, message)
            .await
            .unwrap();

        assert_eq!(count, 2);

        let published = service.published_messages.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "test-group");
    }

    #[tokio::test]
    async fn test_mock_list_groups() {
        let service = MockProcessGroupService::new();
        let ctx =
            RequestContext::new_without_auth("test-tenant".to_string(), "default".to_string());

        service
            .join_group(&ctx, "group-1", "actor-1", vec![])
            .await
            .unwrap();
        service
            .join_group(&ctx, "group-2", "actor-2", vec![])
            .await
            .unwrap();

        let groups = service.list_groups(&ctx).await.unwrap();
        assert_eq!(groups.len(), 2);
        assert!(groups.contains(&"group-1".to_string()));
        assert!(groups.contains(&"group-2".to_string()));
    }
}
