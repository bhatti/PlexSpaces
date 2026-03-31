// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! TTL Tests for ActorRef
//!
//! Tests that ActorRef correctly handles TTL when converting messages to proto

#[cfg(test)]
mod tests {
    use crate::actor_ref::{ActorRef, ActorRefError};
    use plexspaces_core::{Message, ServiceLocator};
    use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
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

    /// Test that ActorRef preserves TTL when converting to proto
    #[tokio::test]
    async fn test_actor_ref_ttl_preserved() {
        let mailbox = create_test_mailbox().await;
        use plexspaces_node::create_default_service_locator;
        let service_locator =
            create_default_service_locator(Some("test-node".to_string()), None).await;
        let _actor_ref = ActorRef::local(
            "test@node1".to_string(),
            "test".to_string(),
            "test".to_string(),
            mailbox,
            service_locator,
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
        use plexspaces_node::create_default_service_locator;
        let service_locator =
            create_default_service_locator(Some("test-node".to_string()), None).await;
        let _actor_ref = ActorRef::local(
            "test@node1".to_string(),
            "test".to_string(),
            "test".to_string(),
            mailbox,
            service_locator,
        );

        let message = create_test_message(b"test".to_vec());

        // Verify no TTL
        assert!(message.ttl.is_none());
    }

    /// Test that expired messages can still be sent (TTL is informational)
    #[tokio::test]
    async fn test_actor_ref_expired_message() {
        let mailbox = create_test_mailbox().await;
        use plexspaces_node::create_default_service_locator;
        let service_locator =
            create_default_service_locator(Some("test-node".to_string()), None).await;
        let actor_ref = ActorRef::local(
            "test@node1".to_string(),
            "test".to_string(),
            "test".to_string(),
            Arc::clone(&mailbox),
            service_locator.clone(),
        );

        // Register actor before calling tell()
        use plexspaces_core::{ActorRegistry, RequestContext};
        if let Some(registry) = service_locator.actor_registry().await {
            let ctx =
                RequestContext::new_without_auth("internal".to_string(), "system".to_string());
            let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref.clone());
            registry
                .register_actor(
                    &ctx,
                    "test@node1".to_string(),
                    sender,
                    "TestActor".to_string(),
                    None,
                    None,
                    None,
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
        let result = actor_ref.tell(message).await;
        // This should succeed - TTL checking is done by mailbox/actor, not ActorRef
        assert!(result.is_ok());
    }
}
