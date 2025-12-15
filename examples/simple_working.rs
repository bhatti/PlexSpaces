//! Simple Working Example
//!
//! This demonstrates the simplified PlexSpaces architecture with just 3 concepts:
//! 1. Actor - Unit of computation
//! 2. Message - Unit of communication
//! 3. Facet - Unit of capability

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// ============================================================================
// CONCEPT 1: ACTOR
// ============================================================================

struct Actor {
    id: String,
    state: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    facets: Arc<RwLock<Vec<Box<dyn Facet>>>>,
    inbox: mpsc::Receiver<Message>,
    outbox: mpsc::Sender<Message>,
}

impl Actor {
    fn new(id: impl Into<String>) -> (Self, mpsc::Sender<Message>) {
        let (tx, rx) = mpsc::channel(100);
        let (outbox, _) = mpsc::channel(100);

        let actor = Actor {
            id: id.into(),
            state: Arc::new(RwLock::new(HashMap::new())),
            facets: Arc::new(RwLock::new(Vec::new())),
            inbox: rx,
            outbox,
        };

        (actor, tx)
    }

    async fn attach_facet(&self, facet: Box<dyn Facet>) {
        let mut facets = self.facets.write().await;
        println!(
            "Attaching facet '{}' to actor '{}'",
            facet.facet_type(),
            self.id
        );
        facets.push(facet);
    }

    async fn run(mut self) {
        println!("Actor '{}' started", self.id);

        while let Some(msg) = self.inbox.recv().await {
            println!("Actor '{}' received: {:?}", self.id, msg);

            // Apply facet interceptors
            let facets = self.facets.read().await;
            for facet in facets.iter() {
                facet.intercept(&msg).await;
            }

            // Process message
            match msg.msg_type.as_str() {
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
                }
                "get" => {
                    let state = self.state.read().await;
                    let count = state
                        .get("counter")
                        .and_then(|v| String::from_utf8(v.clone()).ok())
                        .unwrap_or_else(|| "0".to_string());
                    println!("Actor '{}' counter value: {}", self.id, count);
                }
                _ => {
                    println!("Actor '{}' unknown message type: {}", self.id, msg.msg_type);
                }
            }
        }

        println!("Actor '{}' stopped", self.id);
    }
}

// ============================================================================
// CONCEPT 2: MESSAGE
// ============================================================================

#[derive(Debug, Clone)]
struct Message {
    from: String,
    to: String,
    msg_type: String,
    payload: Vec<u8>,
}

impl Message {
    fn new(from: impl Into<String>, to: impl Into<String>, msg_type: impl Into<String>) -> Self {
        Message {
            from: from.into(),
            to: to.into(),
            msg_type: msg_type.into(),
            payload: Vec::new(),
        }
    }

    fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = payload;
        self
    }
}

// ============================================================================
// CONCEPT 3: FACET
// ============================================================================

#[async_trait]
trait Facet: Send + Sync {
    fn facet_type(&self) -> &str;
    async fn intercept(&self, msg: &Message);
}

// Example facet: Logging
struct LoggingFacet {
    level: String,
}

#[async_trait]
impl Facet for LoggingFacet {
    fn facet_type(&self) -> &str {
        "logging"
    }

    async fn intercept(&self, msg: &Message) {
        println!(
            "[{}] Message: {} -> {} (type: {})",
            self.level, msg.from, msg.to, msg.msg_type
        );
    }
}

// Example facet: Metrics
struct MetricsFacet {
    counter: Arc<RwLock<u64>>,
}

#[async_trait]
impl Facet for MetricsFacet {
    fn facet_type(&self) -> &str {
        "metrics"
    }

    async fn intercept(&self, _msg: &Message) {
        let mut counter = self.counter.write().await;
        *counter += 1;
        println!("[Metrics] Total messages processed: {}", *counter);
    }
}

// ============================================================================
// MAIN - Demonstrate the system
// ============================================================================

#[tokio::main]
async fn main() {
    println!("=== PlexSpaces Simple Example ===\n");

    // Create actors
    let (actor1, tx1) = Actor::new("counter-1");
    let (actor2, tx2) = Actor::new("counter-2");

    // Attach facets (capabilities)
    let actor1_ref = &actor1;
    actor1_ref
        .attach_facet(Box::new(LoggingFacet {
            level: "INFO".to_string(),
        }))
        .await;

    let actor2_ref = &actor2;
    actor2_ref
        .attach_facet(Box::new(LoggingFacet {
            level: "DEBUG".to_string(),
        }))
        .await;
    actor2_ref
        .attach_facet(Box::new(MetricsFacet {
            counter: Arc::new(RwLock::new(0)),
        }))
        .await;

    // Start actors
    let handle1 = tokio::spawn(actor1.run());
    let handle2 = tokio::spawn(actor2.run());

    // Send messages
    println!("\n--- Sending messages ---\n");

    tx1.send(Message::new("main", "counter-1", "increment"))
        .await
        .unwrap();
    tx1.send(Message::new("main", "counter-1", "increment"))
        .await
        .unwrap();
    tx1.send(Message::new("main", "counter-1", "get"))
        .await
        .unwrap();

    tx2.send(Message::new("main", "counter-2", "increment"))
        .await
        .unwrap();
    tx2.send(Message::new("main", "counter-2", "get"))
        .await
        .unwrap();

    // Wait a bit for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Shutdown
    println!("\n--- Shutting down ---\n");
    drop(tx1);
    drop(tx2);

    handle1.await.unwrap();
    handle2.await.unwrap();

    println!("\n=== Example Complete ===");
    println!("\nKey Takeaways:");
    println!("1. Just 3 concepts: Actor, Message, Facet");
    println!("2. Actors have state and process messages");
    println!("3. Facets add capabilities (logging, metrics, etc.)");
    println!("4. Everything is composable at runtime");
}

// ============================================================================
// TESTS - TDD Approach
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_actor_creation() {
        let (actor, _tx) = Actor::new("test");
        assert_eq!(actor.id, "test");
    }

    #[tokio::test]
    async fn test_message_creation() {
        let msg = Message::new("from", "to", "type").with_payload(b"data".to_vec());

        assert_eq!(msg.from, "from");
        assert_eq!(msg.to, "to");
        assert_eq!(msg.msg_type, "type");
        assert_eq!(msg.payload, b"data");
    }

    #[tokio::test]
    async fn test_facet_attachment() {
        let (actor, _tx) = Actor::new("test");

        let facet = Box::new(LoggingFacet {
            level: "TEST".to_string(),
        });

        actor.attach_facet(facet).await;

        let facets = actor.facets.read().await;
        assert_eq!(facets.len(), 1);
    }
}
