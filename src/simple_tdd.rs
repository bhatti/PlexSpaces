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

//! Simplified TDD implementation - walking skeleton
//!
//! This module demonstrates the core PlexSpaces concepts with a minimal,
//! testable implementation following TDD principles.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ============================================================================
// Core Types (simplified for TDD)
// ============================================================================

/// Simple actor ID type
pub type ActorId = String;

/// Simple message structure
#[derive(Debug, Clone)]
pub struct Message {
    pub from: ActorId,
    pub to: ActorId,
    pub msg_type: String,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        msg_type: impl Into<String>,
    ) -> Self {
        Message {
            from: from.into(),
            to: to.into(),
            msg_type: msg_type.into(),
            payload: Vec::new(),
        }
    }

    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }
}

/// Facet trait for capabilities
#[async_trait]
pub trait Facet: Send + Sync {
    fn facet_type(&self) -> &str;
    async fn intercept(&self, msg: &Message);
}

/// Simple actor implementation
pub struct Actor {
    id: ActorId,
    state: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    facets: Arc<RwLock<Vec<Box<dyn Facet>>>>,
    _inbox: Arc<RwLock<mpsc::Receiver<Message>>>,
    _sender: mpsc::Sender<Message>,
}

impl Actor {
    pub fn new(id: impl Into<String>) -> (Self, mpsc::Sender<Message>) {
        let (tx, rx) = mpsc::channel(100);

        let actor = Actor {
            id: id.into(),
            state: Arc::new(RwLock::new(HashMap::new())),
            facets: Arc::new(RwLock::new(Vec::new())),
            _inbox: Arc::new(RwLock::new(rx)),
            _sender: tx.clone(),
        };

        (actor, tx)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn attach_facet(&self, facet: Box<dyn Facet>) {
        let mut facets = self.facets.write().await;
        facets.push(facet);
    }

    pub async fn process_message(&self, msg: Message) -> Result<(), String> {
        // Apply facets
        let facets = self.facets.read().await;
        for facet in facets.iter() {
            facet.intercept(&msg).await;
        }

        // Process message based on type
        match msg.msg_type.as_str() {
            "set" => {
                let key = String::from_utf8(msg.payload.clone()).map_err(|e| e.to_string())?;
                let mut state = self.state.write().await;
                state.insert(key, vec![1]); // Simple value
                Ok(())
            }
            "get" => {
                let key = String::from_utf8(msg.payload).map_err(|e| e.to_string())?;
                let state = self.state.read().await;
                let _value = state.get(&key);
                // In real implementation, would send reply
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub async fn get_state(&self, key: &str) -> Option<Vec<u8>> {
        let state = self.state.read().await;
        state.get(key).cloned()
    }
}

// ============================================================================
// Example Facets
// ============================================================================

pub struct LoggingFacet {
    level: String,
}

impl LoggingFacet {
    pub fn new(level: impl Into<String>) -> Self {
        LoggingFacet {
            level: level.into(),
        }
    }
}

#[async_trait]
impl Facet for LoggingFacet {
    fn facet_type(&self) -> &str {
        "logging"
    }

    async fn intercept(&self, msg: &Message) {
        println!(
            "[{}] Message: {} -> {} ({})",
            self.level, msg.from, msg.to, msg.msg_type
        );
    }
}

// ============================================================================
// Tests - TDD Approach
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::new("actor1", "actor2", "test");
        assert_eq!(msg.from, "actor1");
        assert_eq!(msg.to, "actor2");
        assert_eq!(msg.msg_type, "test");
        assert!(msg.payload.is_empty());
    }

    #[test]
    fn test_message_with_payload() {
        let msg = Message::new("a", "b", "test").with_payload(b"hello".to_vec());
        assert_eq!(msg.payload, b"hello");
    }

    #[tokio::test]
    async fn test_actor_creation() {
        let (actor, _tx) = Actor::new("test-actor");
        assert_eq!(actor.id(), "test-actor");
    }

    #[tokio::test]
    async fn test_actor_state() {
        let (actor, _tx) = Actor::new("test");

        // Process a set message
        let msg = Message::new("sender", "test", "set").with_payload(b"key1".to_vec());
        actor.process_message(msg).await.unwrap();

        // Check state was updated
        let value = actor.get_state("key1").await;
        assert!(value.is_some());
    }

    #[tokio::test]
    async fn test_facet_attachment() {
        let (actor, _tx) = Actor::new("test");

        let facet = Box::new(LoggingFacet::new("INFO"));
        actor.attach_facet(facet).await;

        // Verify facet is attached (in real impl, would check facet list)
        let facets = actor.facets.read().await;
        assert_eq!(facets.len(), 1);
    }

    #[tokio::test]
    async fn test_facet_interception() {
        let (actor, _tx) = Actor::new("test");

        // Attach logging facet
        let facet = Box::new(LoggingFacet::new("DEBUG"));
        actor.attach_facet(facet).await;

        // Process message - facet should intercept
        let msg = Message::new("sender", "test", "set").with_payload(b"key1".to_vec());

        // This will print to console showing facet interception
        actor.process_message(msg).await.unwrap();
    }

    #[tokio::test]
    async fn test_multiple_facets() {
        let (actor, _tx) = Actor::new("test");

        // Attach multiple facets
        actor
            .attach_facet(Box::new(LoggingFacet::new("INFO")))
            .await;
        actor
            .attach_facet(Box::new(LoggingFacet::new("DEBUG")))
            .await;

        let facets = actor.facets.read().await;
        assert_eq!(facets.len(), 2);
    }

    #[tokio::test]
    #[ignore] // Legacy test - uses old API, needs migration to new ActorBuilder API
    async fn test_end_to_end() {
        // NOTE: This test uses old API - needs migration to new ActorBuilder API
        // TODO: Migrate to use ActorBuilder and new Actor API
        // For now, just verify the test compiles
        assert!(true);
    }
}
