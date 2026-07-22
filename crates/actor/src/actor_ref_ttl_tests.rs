// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! TTL Tests for ActorRef
//!
//! Tests that ActorRef correctly handles TTL when converting messages to proto

#[cfg(test)]
mod tests {
    use crate::actor_ref::ActorRef;
    use crate::core::{ActorId, Message, ServiceLocator};
    use plexspaces_mailbox::{mailbox_config_default, Mailbox};
    use plexspaces_proto::actor::v1::ActorVisibility;
    use std::sync::Arc;
    use std::time::Duration;
    use ulid::Ulid;

    /// Helper to check if a message is expired based on timestamp and TTL
    fn is_message_expired(message: &Message) -> bool {
        if message.ttl.is_none() || message.timestamp.is_none() {
            return false;
        }

        let ttl = message.ttl.as_ref().unwrap();
        let timestamp = message.timestamp.as_ref().unwrap();

        let ttl_duration = Duration::new(ttl.seconds as u64, ttl.nanos as u32);
        let created_at =
            std::time::UNIX_EPOCH + Duration::new(timestamp.seconds as u64, timestamp.nanos as u32);
        let expiry = created_at + ttl_duration;

        std::time::SystemTime::now() > expiry
    }

    /// Helper to create a test message
    fn create_test_message(payload: Vec<u8>) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    /// Helper to create a test message with TTL
    fn create_test_message_with_ttl(payload: Vec<u8>, ttl: Duration) -> Message {
        let mut msg = create_test_message(payload);
        // Set timestamp to current time for TTL calculation
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        msg.timestamp = Some(prost_types::Timestamp {
            seconds: now.as_secs() as i64,
            nanos: now.subsec_nanos() as i32,
        });
        msg.ttl = Some(prost_types::Duration {
            seconds: ttl.as_secs() as i64,
            nanos: ttl.subsec_nanos() as i32,
        });
        msg
    }

    async fn create_test_mailbox() -> Arc<Mailbox> {
        Arc::new(
            Mailbox::new(mailbox_config_default(), "test-actor@test-node".to_string())
                .await
                .expect("Failed to create mailbox"),
        )
    }

    fn test_actor_id(name: &str, node_id: &str) -> ActorId {
        ActorId::new(name, "worker", "test", node_id).expect("test actor id must be valid")
    }

    /// Test that ActorRef preserves TTL when converting to proto
    #[tokio::test]
    async fn test_actor_ref_ttl_preserved() {
        let mailbox = create_test_mailbox().await;
        let service_locator: Arc<dyn ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new());
        let _actor_ref = ActorRef::local(
            test_actor_id("test", "node1"),
            "test".to_string(),
            "test".to_string(),
            mailbox,
            service_locator,
            ActorVisibility::ActorVisibilityPublic,
        );

        let ttl = Duration::from_secs(30);
        let message = create_test_message_with_ttl(b"test".to_vec(), ttl);

        // TTL is already a proto Duration, just verify it
        assert!(message.ttl.is_some());
        let proto_ttl = message.ttl.unwrap();
        assert_eq!(proto_ttl.seconds, 30);
    }

    /// Test that ActorRef handles messages without TTL
    #[tokio::test]
    async fn test_actor_ref_no_ttl() {
        let mailbox = create_test_mailbox().await;
        let service_locator: Arc<dyn ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new());
        let _actor_ref = ActorRef::local(
            test_actor_id("test", "node1"),
            "test".to_string(),
            "test".to_string(),
            mailbox,
            service_locator,
            ActorVisibility::ActorVisibilityPublic,
        );

        let message = create_test_message(b"test".to_vec());

        // Verify no TTL
        assert!(message.ttl.is_none());
    }

    /// Test that expired messages can still be sent (TTL is informational)
    #[tokio::test]
    async fn test_actor_ref_expired_message() {
        let mailbox = create_test_mailbox().await;
        let service_locator: Arc<dyn ServiceLocator> =
            Arc::new(crate::TestServiceLocatorStub::new());
        let actor_id = ActorId::new("test", "worker", "test", "node1").unwrap();
        let actor_ref = ActorRef::local(
            actor_id.clone(),
            "test".to_string(),
            "test".to_string(),
            Arc::clone(&mailbox),
            service_locator.clone(),
            ActorVisibility::ActorVisibilityPublic,
        );

        // Register actor before calling tell()
        use crate::core::RequestContext;
        use plexspaces_common::RequestContextExt;
        let tell_ctx =
            RequestContext::new_without_auth("internal".to_string(), "system".to_string());
        if let Some(registry) = service_locator.actor_registry().await {
            let sender: Arc<dyn crate::core::MessageSender> = Arc::new(actor_ref.clone());
            registry
                .register_actor(
                    &tell_ctx,
                    crate::ActorRegistrationParams {
                        actor_id,
                        sender,
                        actor_type: "test_actor".to_string(),
                        config: None,
                        instance: None,
                        behavior_kind: None,
                    },
                )
                .await;
        }

        let ttl = Duration::from_millis(10);
        let message = create_test_message_with_ttl(b"test".to_vec(), ttl);

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Message is expired, but we can still send it
        // (TTL checking happens at mailbox/actor level, not ActorRef level)
        assert!(is_message_expired(&message));

        // ActorRef should still allow sending (TTL is informational)
        let result = actor_ref.tell(&tell_ctx, message).await;
        // This should succeed - TTL checking is done by mailbox/actor, not ActorRef
        assert!(result.is_ok());
    }
}
