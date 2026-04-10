//! Canonical ActorId Routing Example
//!
//! Demonstrates location-transparent actor messaging using canonical ActorId strings.
//! This example shows how to:
//! 1. Create actors on different nodes
//! 2. Send messages using canonical routing identities
//! 3. Use ActorService for location-transparent messaging
//!
//! ## Usage
//! ```bash
//! cargo run --example actor_at_node
//! ```

use async_trait::async_trait;
use plexspaces_core::{Actor, ActorContext, BehaviorError, BehaviorType, Message};
use plexspaces_node::{Node, NodeBuilder};

/// Helper to create a proto Message with payload
fn create_message(payload: Vec<u8>) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

// ============================================================================
// Simple Counter Actor
// ============================================================================

struct Counter {
    count: i32,
}

#[async_trait]
impl Actor for Counter {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        let node_id = ctx.node_id.clone();
        println!(
            "[actor@{}] Received: {} (count: {})",
            node_id, payload, self.count
        );

        match payload.as_ref() {
            "increment" => {
                self.count += 1;
                println!("[actor@{}] Count incremented to {}", node_id, self.count);
            }
            "get" => {
                println!("[actor@{}] Current count: {}", node_id, self.count);
            }
            _ => {
                println!("[actor@{}] Unknown command: {}", node_id, payload);
            }
        }

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// ============================================================================
// Main Example
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Actor@Node Syntax Example                                     ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Create Node 1
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Creating Node 1...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let node1 = NodeBuilder::new("node1")
        .with_listen_addr("127.0.0.1:8000".to_string())
        .build()
        .await;

    // Spawn counter actor on node1
    use plexspaces_actor::ActorBuilder;
    println!("\nSpawning counter on node1...");
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    )
    .with_internal(true)
    .with_admin(true);
    let counter1_ref = ActorBuilder::new(Box::new(Counter { count: 0 }))
        .with_name("counter")
        .spawn(&ctx, node1.service_locator().clone())
        .await?;

    println!("✅ counter spawned with canonical ID: {}", counter1_ref.id());

    // Create a minimal ActorContext for sending messages
    // In a real actor, you would get this from handle_message() parameter
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "default".to_string(),
        "default".to_string(),
        service_locator,
        None,
    );

    // Send messages using canonical structured actor IDs
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Sending messages using canonical structured actor IDs...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Method 1: Using ActorService from ActorContext (location-transparent)
    println!("\n1. Sending via ActorService (location-transparent):");
    let msg1 = create_message(b"increment".to_vec());
    let actor_service = ctx
        .get_actor_service()
        .await
        .ok_or("ActorService not available")?;
    actor_service
        .send(&counter1_ref.id(), msg1)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;
    println!("   ✅ Sent 'increment' to {} via ActorService.send()", counter1_ref.id());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Method 2: Using ActorService from ActorContext (location-transparent)
    // This demonstrates how actors can send messages using canonical actor IDs
    println!("\n2. Sending via ActorService (location-transparent, canonical ActorId):");
    // For this example, we'll show the pattern:
    println!("   ℹ️  In an actor's handle_message, you would use:");
    println!("      ctx.actor_service.send(target_ref.id(), msg).await?;");
    println!("   ℹ️  This works for both local and remote actors!");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Method 3: Another message via ActorService
    println!("\n3. Sending 'get' command via ActorService:");
    let msg3 = create_message(b"get".to_vec());
    let actor_service = ctx
        .get_actor_service()
        .await
        .ok_or("ActorService not available")?;
    actor_service.send(&counter1_ref.id(), msg3).await?;
    println!("   ✅ Sent 'get' to {} via ActorService.send()", counter1_ref.id());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Method 4: Increment again
    println!("\n4. Sending another 'increment' command:");
    let msg4 = create_message(b"increment".to_vec());
    actor_service
        .send(&counter1_ref.id(), msg4)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;
    println!("   ✅ Sent 'increment' to {} via ActorService.send()", counter1_ref.id());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Method 5: Final get
    println!("\n5. Sending final 'get' command:");
    let msg5 = create_message(b"get".to_vec());
    actor_service
        .send(&counter1_ref.id(), msg5)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;
    println!("   ✅ Sent 'get' to {} via ActorService.send()", counter1_ref.id());

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example Complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Key Takeaways:");
    println!("  • Actors are created with NodeBuilder and ActorBuilder (unified API)");
    println!("  • Actor IDs use canonical structured form name//actor_type::namespace@node_id");
    println!("  • Client code should usually provide only the actor name; the builder/factory adds type, namespace, and node");
    println!("  • ActorService.send() works for local and remote actors with the same ActorRef");
    println!("  • ActorService handles local/remote routing automatically");
    println!("  • Same API works for local and remote actors (location transparency)");
    println!();
    println!("Example Usage in Actor:");
    println!("  async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {{");
    println!("      // Get ActorRef for target actor");
    println!("      let target_ref = ctx.self_ref().expect(\"actor ref available\");");
    println!("      // Send message (works for local and remote)");
    println!("      target_ref.tell(ctx, msg).await?;");
    println!("      // Or use ActorService directly");
    println!("      ctx.actor_service.send(target_ref.id(), msg).await?;");
    println!("      Ok(())");
    println!("  }}");

    Ok(())
}
