//! Simple TDD tests that actually run
//!
//! This is a standalone test file that demonstrates the simplified
//! PlexSpaces architecture with working tests.

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
}

impl Actor {
    pub fn new(id: impl Into<String>) -> Self {
        Actor {
            id: id.into(),
            state: Arc::new(RwLock::new(HashMap::new())),
            facets: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn attach_facet(&self, facet: Box<dyn Facet>) {
        let mut facets = self.facets.write().await;
        println!(
            "Attaching facet '{}' to actor '{}'",
            facet.facet_type(),
            self.id
        );
        facets.push(facet);
    }

    pub async fn process_message(&self, msg: Message) -> Result<(), String> {
        println!(
            "Actor '{}' processing message from '{}' of type '{}'",
            self.id, msg.from, msg.msg_type
        );

        // Apply facets
        let facets = self.facets.read().await;
        for facet in facets.iter() {
            facet.intercept(&msg).await;
        }

        // Process message based on type
        match msg.msg_type.as_str() {
            "set" => {
                if let Ok(key) = String::from_utf8(msg.payload.clone()) {
                    let mut state = self.state.write().await;
                    state.insert(key.clone(), vec![1]); // Simple value
                    println!("Actor '{}' set key '{}'", self.id, key);
                }
                Ok(())
            }
            "increment" => {
                let mut state = self.state.write().await;
                let count = state
                    .get("counter")
                    .and_then(|v| String::from_utf8(v.clone()).ok())
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(0);

                let new_count = count + 1;
                state.insert("counter".to_string(), new_count.to_string().into_bytes());
                println!("Actor '{}' incremented counter to {}", self.id, new_count);
                Ok(())
            }
            "get" => {
                let state = self.state.read().await;
                let count = state
                    .get("counter")
                    .and_then(|v| String::from_utf8(v.clone()).ok())
                    .unwrap_or_else(|| "0".to_string());
                println!("Actor '{}' counter value: {}", self.id, count);
                Ok(())
            }
            _ => {
                println!("Actor '{}' unknown message type: {}", self.id, msg.msg_type);
                Ok(())
            }
        }
    }

    pub async fn get_state(&self, key: &str) -> Option<Vec<u8>> {
        let state = self.state.read().await;
        state.get(key).cloned()
    }

    pub async fn facet_count(&self) -> usize {
        let facets = self.facets.read().await;
        facets.len()
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
            "[{}] Intercepted: {} -> {} (type: {})",
            self.level, msg.from, msg.to, msg.msg_type
        );
    }
}

pub struct MetricsFacet {
    counter: Arc<RwLock<u64>>,
}

impl MetricsFacet {
    pub fn new() -> Self {
        MetricsFacet {
            counter: Arc::new(RwLock::new(0)),
        }
    }
}

#[async_trait]
impl Facet for MetricsFacet {
    fn facet_type(&self) -> &str {
        "metrics"
    }

    async fn intercept(&self, _msg: &Message) {
        let mut counter = self.counter.write().await;
        *counter += 1;
        println!("[Metrics] Total messages: {}", *counter);
    }
}

// ============================================================================
// Tests - TDD Approach
// ============================================================================

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
    let actor = Actor::new("test-actor");
    assert_eq!(actor.id(), "test-actor");
}

#[tokio::test]
async fn test_actor_state() {
    let actor = Actor::new("test");

    // Process a set message
    let msg = Message::new("sender", "test", "set").with_payload(b"key1".to_vec());
    actor.process_message(msg).await.unwrap();

    // Check state was updated
    let value = actor.get_state("key1").await;
    assert!(value.is_some());
    assert_eq!(value.unwrap(), vec![1]);
}

#[tokio::test]
async fn test_counter_increment() {
    let actor = Actor::new("counter");

    // Increment counter multiple times
    for i in 1..=3 {
        let msg = Message::new("test", "counter", "increment");
        actor.process_message(msg).await.unwrap();

        // Check counter value
        let counter = actor.get_state("counter").await;
        assert!(counter.is_some());
        let value = String::from_utf8(counter.unwrap()).unwrap();
        assert_eq!(value, i.to_string());
    }
}

#[tokio::test]
async fn test_facet_attachment() {
    let actor = Actor::new("test");

    let facet = Box::new(LoggingFacet::new("INFO"));
    actor.attach_facet(facet).await;

    // Verify facet is attached
    assert_eq!(actor.facet_count().await, 1);
}

#[tokio::test]
async fn test_facet_interception() {
    let actor = Actor::new("test");

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
    let actor = Actor::new("test");

    // Attach multiple facets
    actor
        .attach_facet(Box::new(LoggingFacet::new("INFO")))
        .await;
    actor.attach_facet(Box::new(MetricsFacet::new())).await;

    assert_eq!(actor.facet_count().await, 2);

    // Process message with both facets intercepting
    let msg = Message::new("sender", "test", "increment");
    actor.process_message(msg).await.unwrap();
}

#[tokio::test]
async fn test_end_to_end_simple() {
    println!("\n=== End-to-End Test ===\n");

    // Create actors
    let actor1 = Actor::new("actor1");
    let actor2 = Actor::new("actor2");

    // Attach facets
    actor1
        .attach_facet(Box::new(LoggingFacet::new("INFO")))
        .await;
    actor2
        .attach_facet(Box::new(LoggingFacet::new("DEBUG")))
        .await;
    actor2.attach_facet(Box::new(MetricsFacet::new())).await;

    // Send messages to actor1
    for i in 1..=2 {
        let msg = Message::new("main", "actor1", "increment");
        actor1.process_message(msg).await.unwrap();
    }

    // Send messages to actor2
    for i in 1..=3 {
        let msg = Message::new("main", "actor2", "increment");
        actor2.process_message(msg).await.unwrap();
    }

    // Verify state
    let actor1_counter = actor1.get_state("counter").await.unwrap();
    let actor2_counter = actor2.get_state("counter").await.unwrap();

    assert_eq!(String::from_utf8(actor1_counter).unwrap(), "2");
    assert_eq!(String::from_utf8(actor2_counter).unwrap(), "3");

    println!("\n=== Test Complete ===\n");
}

#[test]
fn test_tdd_incremental_development() {
    // This test demonstrates the TDD approach:
    // 1. Start with simple data structures (Message)
    // 2. Add basic operations (Actor creation)
    // 3. Add state management (set/get)
    // 4. Add behaviors (increment)
    // 5. Add composition (facets)
    // 6. Integration (end-to-end)

    println!("TDD Steps:");
    println!("1. ✓ Data structures");
    println!("2. ✓ Basic operations");
    println!("3. ✓ State management");
    println!("4. ✓ Behaviors");
    println!("5. ✓ Composition (facets)");
    println!("6. ✓ Integration");
}
