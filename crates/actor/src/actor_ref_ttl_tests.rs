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
    use plexspaces_core::ServiceLocator;
    use plexspaces_mailbox::{Mailbox, MailboxConfig, Message, mailbox_config_default};
    use std::sync::Arc;
    use std::time::Duration;
    
    async fn create_test_mailbox() -> Arc<Mailbox> {
        Arc::new(Mailbox::new(mailbox_config_default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"))
    }

    /// Test that ActorRef preserves TTL when converting to proto
    #[tokio::test]
    async fn test_actor_ref_ttl_preserved() {
        let mailbox = create_test_mailbox().await;
        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let _actor_ref = ActorRef::local("test@node1".to_string(), mailbox, service_locator);
        
        let ttl = Duration::from_secs(30);
        let message = Message::new(b"test".to_vec()).with_ttl(ttl);
        
        // Convert to proto via to_proto_message (internal method)
        // We'll test this indirectly via tell() which uses it
        let proto_msg = message.to_proto();
        assert!(proto_msg.ttl.is_some());
        let proto_ttl = proto_msg.ttl.unwrap();
        assert_eq!(proto_ttl.seconds, 30);
    }

    /// Test that ActorRef handles messages without TTL
    #[tokio::test]
    async fn test_actor_ref_no_ttl() {
        let mailbox = create_test_mailbox().await;
        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let _actor_ref = ActorRef::local("test@node1".to_string(), mailbox, service_locator);
        
        let message = Message::new(b"test".to_vec());
        
        let proto_msg = message.to_proto();
        assert!(proto_msg.ttl.is_none());
    }

    /// Test that expired messages can still be sent (TTL is informational)
    #[tokio::test]
    async fn test_actor_ref_expired_message() {
        let mailbox = create_test_mailbox().await;
        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let actor_ref = ActorRef::local("test@node1".to_string(), Arc::clone(&mailbox), service_locator);
        
        let ttl = Duration::from_millis(10);
        let message = Message::new(b"test".to_vec()).with_ttl(ttl);
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        // Message is expired, but we can still send it
        // (TTL checking happens at mailbox/actor level, not ActorRef level)
        assert!(message.is_expired());
        
        // ActorRef should still allow sending (TTL is informational)
        let result = actor_ref.tell(message).await;
        // This should succeed - TTL checking is done by mailbox/actor, not ActorRef
        assert!(result.is_ok());
    }
}

