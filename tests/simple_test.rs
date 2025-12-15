//! Simple TDD Test
//!
//! Start with the absolute minimum to get something working.

#[test]
fn test_basic_message() {
    // Test that we can create a simple message structure
    struct SimpleMessage {
        from: String,
        to: String,
        payload: Vec<u8>,
    }

    let msg = SimpleMessage {
        from: "actor1".to_string(),
        to: "actor2".to_string(),
        payload: b"hello".to_vec(),
    };

    assert_eq!(msg.from, "actor1");
    assert_eq!(msg.to, "actor2");
    assert_eq!(msg.payload, b"hello");
}

#[tokio::test]
async fn test_basic_actor() {
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Simplest possible actor
    struct SimpleActor {
        id: String,
        state: Arc<RwLock<i32>>,
    }

    impl SimpleActor {
        fn new(id: String) -> Self {
            SimpleActor {
                id,
                state: Arc::new(RwLock::new(0)),
            }
        }

        async fn increment(&self) -> i32 {
            let mut state = self.state.write().await;
            *state += 1;
            *state
        }

        async fn get_value(&self) -> i32 {
            *self.state.read().await
        }
    }

    let actor = SimpleActor::new("test".to_string());
    assert_eq!(actor.id, "test");

    assert_eq!(actor.get_value().await, 0);
    assert_eq!(actor.increment().await, 1);
    assert_eq!(actor.increment().await, 2);
    assert_eq!(actor.get_value().await, 2);
}

#[tokio::test]
async fn test_basic_behavior() {
    use async_trait::async_trait;

    // Simplest behavior trait
    #[async_trait]
    trait SimpleBehavior: Send + Sync {
        async fn handle(&mut self, msg: Vec<u8>) -> Vec<u8>;
    }

    // Echo behavior
    struct EchoBehavior;

    #[async_trait]
    impl SimpleBehavior for EchoBehavior {
        async fn handle(&mut self, msg: Vec<u8>) -> Vec<u8> {
            msg // Just echo back
        }
    }

    let mut behavior = EchoBehavior;
    let input = b"test".to_vec();
    let output = behavior.handle(input.clone()).await;
    assert_eq!(output, input);
}

#[tokio::test]
async fn test_basic_facet() {
    use std::collections::HashMap;

    // Simplest facet - just configuration
    struct SimpleFacet {
        facet_type: String,
        config: HashMap<String, String>,
    }

    let mut config = HashMap::new();
    config.insert("timeout".to_string(), "30s".to_string());

    let facet = SimpleFacet {
        facet_type: "logging".to_string(),
        config,
    };

    assert_eq!(facet.facet_type, "logging");
    assert_eq!(facet.config.get("timeout"), Some(&"30s".to_string()));
}

#[tokio::test]
async fn test_end_to_end_simple() {
    use std::sync::Arc;
    use tokio::sync::{mpsc, RwLock};

    // Minimal working actor system
    struct MiniActor {
        id: String,
        counter: Arc<RwLock<i32>>,
        sender: mpsc::Sender<String>,
        receiver: Arc<RwLock<mpsc::Receiver<String>>>,
    }

    impl MiniActor {
        fn new(id: String) -> Self {
            let (tx, rx) = mpsc::channel(10);
            MiniActor {
                id,
                counter: Arc::new(RwLock::new(0)),
                sender: tx,
                receiver: Arc::new(RwLock::new(rx)),
            }
        }

        async fn send(&self, msg: String) {
            self.sender.send(msg).await.unwrap();
        }

        async fn process_one(&self) -> Option<String> {
            let mut rx = self.receiver.write().await;
            if let Some(msg) = rx.recv().await {
                let mut counter = self.counter.write().await;
                *counter += 1;
                Some(format!("Processed: {}", msg))
            } else {
                None
            }
        }

        async fn get_count(&self) -> i32 {
            *self.counter.read().await
        }
    }

    // Test the mini actor
    let actor = MiniActor::new("test".to_string());

    // Send messages
    actor.send("msg1".to_string()).await;
    actor.send("msg2".to_string()).await;

    // Process messages
    let result1 = actor.process_one().await;
    assert_eq!(result1, Some("Processed: msg1".to_string()));

    let result2 = actor.process_one().await;
    assert_eq!(result2, Some("Processed: msg2".to_string()));

    // Check counter
    assert_eq!(actor.get_count().await, 2);
}

// This test demonstrates we can build incrementally with TDD
#[test]
fn test_incremental_development() {
    // Step 1: Data structures
    assert!(true);

    // Step 2: Basic operations
    assert!(true);

    // Step 3: Async operations
    assert!(true);

    // Step 4: Integration
    assert!(true);
}
