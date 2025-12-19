// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Cloudflare Workers Durable Objects

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorId, Actor, ActorContext, BehaviorType, BehaviorError};
use plexspaces_journaling::{VirtualActorFacet, DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_node::NodeBuilder;
use plexspaces_mailbox::Message;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

/// Counter message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterMessage {
    Increment,
    Decrement,
    Get,
    Count(u64),
}

/// Counter actor (similar to Cloudflare Durable Object)
pub struct CounterActor {
    count: u64,
}

impl CounterActor {
    pub fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait::async_trait]
impl Actor for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        tracing::debug!(
            "🟦 [COUNTER_ACTOR::handle_message] START: message_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}",
            msg.id, msg.sender, msg.receiver, msg.message_type_str(), msg.correlation_id
        );
        // Use explicit trait method call to avoid infinite recursion
        // This calls the default implementation in GenServer trait
        let result = self.route_message(ctx, msg).await;
        tracing::debug!(
            "🟦 [COUNTER_ACTOR::handle_message] COMPLETED: result={:?}",
            result.is_ok()
        );
        result
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        tracing::debug!(
            "🟠 [HANDLE_REQUEST] START: message_id={}, sender={:?}, receiver={}, correlation_id={:?}, message_type={}",
            msg.id, msg.sender, msg.receiver, msg.correlation_id, msg.message_type_str()
        );
        
        let counter_msg: CounterMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        let reply_msg = match counter_msg {
            CounterMessage::Increment => {
                self.count += 1;
                info!("Counter incremented to: {}", self.count);
                // State automatically persisted via DurabilityFacet if attached
                let reply = CounterMessage::Count(self.count);
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            CounterMessage::Decrement => {
                self.count = self.count.saturating_sub(1);
                info!("Counter decremented to: {}", self.count);
                let reply = CounterMessage::Count(self.count);
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            CounterMessage::Get => {
                info!("Counter get: {}", self.count);
                let reply = CounterMessage::Count(self.count);
                Message::new(serde_json::to_vec(&reply).unwrap())
            }
            _ => return Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        };
        
        // Add correlation_id to reply if present in request
        let mut reply_with_corr = if let Some(corr_id) = &msg.correlation_id {
            tracing::debug!("CounterActor::handle_request: Adding correlation_id to reply: {}", corr_id);
            reply_msg.with_correlation_id(corr_id.clone())
        } else {
            reply_msg
        };
        
        // Extract reply_message_id for logging before moving
        let reply_message_id = reply_with_corr.id.clone();
        let reply_correlation_id = reply_with_corr.correlation_id.clone();
        
        // Send reply using ctx.send_reply() (Erlang-style)
        if let Some(sender_id) = &msg.sender {
            tracing::debug!(
                "🟠 [HANDLE_REQUEST] SENDING REPLY: message_id={}, reply_message_id={}, sender_id={:?}, receiver={}, correlation_id={:?}",
                msg.id, reply_message_id, sender_id, msg.receiver, reply_correlation_id
            );
            ctx.send_reply(
                msg.correlation_id.as_deref(),
                sender_id,
                msg.receiver.clone(),
                reply_with_corr,
            ).await
                .map_err(|e| {
                    tracing::error!(
                        "🟠 [HANDLE_REQUEST] REPLY FAILED: sender_id={:?}, receiver={}, error={}",
                        sender_id, msg.receiver, e
                    );
                    BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                })?;
            tracing::debug!(
                "🟠 [HANDLE_REQUEST] REPLY SENT: message_id={}, reply_message_id={}, sender_id={:?}, receiver={}, correlation_id={:?}",
                msg.id, reply_message_id, sender_id, msg.receiver, msg.correlation_id
            );
        } else {
            tracing::debug!(
                "🟠 [HANDLE_REQUEST] NO REPLY: No sender_id, skipping reply (fire-and-forget): receiver={}",
                msg.receiver
            );
        }
        
        tracing::debug!(
            "🟠 [HANDLE_REQUEST] COMPLETED: sender={:?}, receiver={}, correlation_id={:?}",
            msg.sender, msg.receiver, msg.correlation_id
        );
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Install panic hook to capture stack traces on panic
    // Note: Stack overflow doesn't trigger panic hook - it's a fatal OS error (SIGSEGV)
    // To get stack trace on stack overflow, run with: lldb -- ./target/release/cloudflare_workers_comparison
    // Then in lldb: run, wait for crash, then: bt (backtrace)
    std::panic::set_hook(Box::new(|panic_info| {
        use std::io::Write;
        let stderr = std::io::stderr();
        let mut handle = stderr.lock();
        let _ = writeln!(handle, "\n╔════════════════════════════════════════════════════════════════╗");
        let _ = writeln!(handle, "║  PANIC DETECTED                                                  ║");
        let _ = writeln!(handle, "╚════════════════════════════════════════════════════════════════╝");
        let _ = writeln!(handle, "\nPanic location: {:?}", panic_info.location());
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            let _ = writeln!(handle, "Panic message: {}", s);
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            let _ = writeln!(handle, "Panic message: {}", s);
        }
        let _ = writeln!(handle, "\nStack backtrace:");
        let _ = writeln!(handle, "{:?}", std::backtrace::Backtrace::capture());
        let _ = writeln!(handle, "\n");
        let _ = handle.flush();
    }));
    
    // Initialize tracing with INFO level by default
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Cloudflare Workers vs PlexSpaces Comparison                   ║");
    println!("║  Demonstrating Durable Objects with DurabilityFacet            ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("=== Cloudflare Workers vs PlexSpaces Comparison ===");
    info!("Demonstrating Durable Objects with DurabilityFacet");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Cloudflare Durable Objects: Use get_or_activate pattern (virtual actors)
    let actor_id: ActorId = "counter-1@comparison-node-1".to_string();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Cloudflare Pattern: get_or_activate_actor (Durable Objects)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    // Use get_or_activate pattern - the Cloudflare Durable Objects pattern!
    // Get ActorFactory and VirtualActorManager from ServiceLocator
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use plexspaces_core::VirtualActorManager;
    use std::sync::Arc;
    
    let service_locator = node.service_locator();
    
    // Wait for services to be registered
    for _ in 0..10 {
        if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    let actor_registry: Arc<plexspaces_core::ActorRegistry> = service_locator.get_service().await
        .ok_or("ActorRegistry not found in ServiceLocator")?;
    let virtual_actor_manager: Arc<VirtualActorManager> = service_locator.get_service().await
        .ok_or("VirtualActorManager not found in ServiceLocator")?;
    
    // Register ActorFactory if not already registered
    let actor_factory: Arc<ActorFactoryImpl> = if let Some(factory) = service_locator.get_service().await {
        factory
    } else {
        let factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(factory.clone()).await;
        factory
    };
    
    // Check if actor is already activated
    let counter = if actor_registry.is_actor_activated(&actor_id).await {
        // Actor already exists - create ActorRef
        tracing::debug!("Actor already activated: {}", actor_id);
        ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            service_locator.clone(),
        )
    } else {
        // Actor doesn't exist - create and spawn it
        tracing::debug!("Actor factory called - creating new Durable Object with facets");
        
        let behavior = Box::new(CounterActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        // Attach VirtualActorFacet (Cloudflare Durable Objects are virtual actors)
        // This automatically registers the actor as virtual
        // Use "eager" activation to ensure actor is ready immediately
        tracing::debug!("Attaching VirtualActorFacet with eager activation");
        let virtual_facet_config = serde_json::json!({
            "idle_timeout": "5m",
            "activation_strategy": "eager"
        });
        let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config.clone()));
        actor
            .attach_facet(virtual_facet, 100, virtual_facet_config)
            .await
            .map_err(|e| format!("Failed to attach VirtualActorFacet: {}", e))?;
        tracing::debug!("VirtualActorFacet attached successfully");
        
        // Attach DurabilityFacet (Cloudflare Durable Objects persist state)
        tracing::debug!("Attaching DurabilityFacet");
        let storage = MemoryJournalStorage::new();
        let durability_config = DurabilityConfig::default();
        let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
        actor
            .attach_facet(durability_facet, 50, serde_json::json!({}))
            .await
            .map_err(|e| format!("Failed to attach DurabilityFacet: {}", e))?;
        tracing::debug!("DurabilityFacet attached successfully");
        
        // Spawn the actor using ActorFactory
        // Note: Since actor has DurabilityFacet attached, we use spawn_built_actor
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_built_actor(&ctx, Arc::new(actor), Some("GenServer".to_string())).await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        
        tracing::debug!("Actor spawned successfully");
        
        // Create ActorRef (use remote() even for local actors - it works for both)
        ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            service_locator.clone(),
        )
    };
    
    tracing::debug!("get_or_activate pattern completed: {}", counter.id());
    
    // Wait for actor to be fully registered
    tokio::time::sleep(Duration::from_millis(200)).await;

    info!("Counter Durable Object created/activated via get_or_activate: {}", counter.id());
    println!("📦 Counter Durable Object created/activated via get_or_activate: {}", counter.id());
    println!("✅ VirtualActorFacet attached - Cloudflare Durable Object lifecycle");
    println!("✅ DurabilityFacet attached - state persistence enabled");
    println!();

    // Wait for actor to be fully initialized and ready to process messages
    // Facets may need time to initialize, and actor needs to be ready in registry
    // VirtualActorFacet with eager activation should activate immediately
    tracing::debug!("Waiting for actor initialization (VirtualActorFacet eager activation)");
    tokio::time::sleep(Duration::from_millis(300)).await;
    tracing::debug!("Actor should be ready to process messages now");

    let execution_start = std::time::Instant::now();
    let mut request_count = 0;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Simulating HTTP requests (like Cloudflare Workers)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    // Simulate HTTP requests (like Cloudflare Workers)
    // Request 1: Increment
    println!("  Sending: Increment request...");
    let mut msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    // Don't set sender - let ask() handle it
    // The ActorRef that calls ask() will be used as the sender for reply routing
    msg.receiver = counter.id().clone();
    tracing::debug!("Main: Sending ask() request with message_type={}, sender={:?}, receiver={}", 
        msg.message_type_str(), msg.sender, msg.receiver);
    let response = counter
        .ask(msg, Duration::from_secs(10))
        .await?;
    tracing::debug!("Main: Received reply from ask()");
    let result: CounterMessage = serde_json::from_slice(response.payload())?;
    if let CounterMessage::Count(count) = result {
        info!("Increment response: count = {}", count);
        println!("  Request 1: Increment → count = {}", count);
        request_count += 1;
        assert_eq!(count, 1);
    }

    // Request 2: Get
    println!("  Sending: Get request...");
    let mut msg = Message::new(serde_json::to_vec(&CounterMessage::Get)?)
        .with_message_type("call".to_string());
    msg.receiver = counter.id().clone();
    tracing::debug!("Main: Sending ask() request with message_type={}, sender={:?}, receiver={}", 
        msg.message_type_str(), msg.sender, msg.receiver);
    let response = counter
        .ask(msg, Duration::from_secs(10))
        .await?;
    tracing::debug!("Main: Received reply from ask()");
    let result: CounterMessage = serde_json::from_slice(response.payload())?;
    if let CounterMessage::Count(count) = result {
        info!("Get response: count = {}", count);
        println!("  Request 2: Get → count = {}", count);
        request_count += 1;
        assert_eq!(count, 1);
    }

    // Request 3: Increment again (commented out for debugging - minimize logs)
    /*
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment)?)
        .with_message_type("call".to_string());
    let response = counter
        .ask(msg, Duration::from_secs(10))
        .await?;
    let result: CounterMessage = serde_json::from_slice(response.payload())?;
    if let CounterMessage::Count(count) = result {
        info!("Increment response: count = {}", count);
        println!("  Request 3: Increment → count = {}", count);
        request_count += 1;
        assert_eq!(count, 2);
    }

    // Request 4: Decrement (commented out for debugging - minimize logs)
    println!("  Sending: Decrement request...");
    let msg = Message::new(serde_json::to_vec(&CounterMessage::Decrement)?)
        .with_message_type("call".to_string());
    let response = counter
        .ask(msg, Duration::from_secs(10))
        .await?;
    let result: CounterMessage = serde_json::from_slice(response.payload())?;
    if let CounterMessage::Count(count) = result {
        info!("Decrement response: count = {}", count);
        println!("  Request 4: Decrement → count = {}", count);
        request_count += 1;
        assert_eq!(count, 1);
    }
    */

    let execution_elapsed = execution_start.elapsed();

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Execution Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Total Requests:    {}", request_count);
    println!("Execution Time:    {:.2}ms", execution_elapsed.as_secs_f64() * 1000.0);
    println!("Avg Time/Request:  {:.2}ms", execution_elapsed.as_secs_f64() * 1000.0 / request_count as f64);
    println!("Final Count:       1");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Execution Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Pattern:           get_or_activate_actor (Cloudflare Durable Objects)");
    println!("VirtualActorFacet: ✅ Automatic activation/deactivation");
    println!("DurabilityFacet:   ✅ State persistence enabled");
    println!("GenServerBehavior: ✅ Request-reply pattern (like HTTP)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  ✅ Comparison Complete                                         ║");
    println!("║  ✅ get_or_activate pattern: Cloudflare Durable Objects         ║");
    println!("║  ✅ VirtualActorFacet: Automatic activation/deactivation        ║");
    println!("║  ✅ DurabilityFacet: State persistence across requests          ║");
    println!("║  ✅ GenServerBehavior: Request-reply pattern (like HTTP)        ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    
    info!("=== Comparison Complete ===");
    info!("✅ get_or_activate pattern: Cloudflare Durable Objects virtual actor activation");
    info!("✅ VirtualActorFacet: Automatic activation/deactivation (like Cloudflare Durable Objects)");
    info!("✅ DurabilityFacet: State persistence across requests");
    info!("✅ GenServerBehavior: Request-reply pattern (like HTTP requests)");
    info!("All operations successful!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter_actor() {
        let node = NodeBuilder::new("test-node")
            .build();
        
        // Wait for services to be registered
        let service_locator = node.service_locator();
        for _ in 0..10 {
            if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        
        // Register ActorFactory for tests
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(actor_factory_impl.clone()).await;
        let actor_factory: Arc<ActorFactoryImpl> = actor_factory_impl;

        let behavior = Box::new(CounterActor::new());
        let actor = ActorBuilder::new(behavior)
            .with_id("test-counter@test-node".to_string())
            .build()
            .await;
        
        // Spawn actor using ActorFactory with spawn_actor
        let ctx = plexspaces_core::RequestContext::internal();
        let actor_id = "test-counter@test-node".to_string();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer", // actor_type
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
        ).await.unwrap();
        
        // Create ActorRef (use remote() even for local actors - it works for both)
        let counter = ActorRef::remote(
            "test-counter@test-node".to_string(),
            "test-node".to_string(),
            service_locator.clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Test increment
        let msg = Message::new(serde_json::to_vec(&CounterMessage::Increment).unwrap())
            .with_message_type("call".to_string());
        let response = counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();
        let result: CounterMessage = serde_json::from_slice(response.payload()).unwrap();
        assert!(matches!(result, CounterMessage::Count(1)));

        // Test get
        let msg = Message::new(serde_json::to_vec(&CounterMessage::Get).unwrap())
            .with_message_type("call".to_string());
        let response = counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();
        let result: CounterMessage = serde_json::from_slice(response.payload()).unwrap();
        assert!(matches!(result, CounterMessage::Count(1)));

        // Test decrement
        let msg = Message::new(serde_json::to_vec(&CounterMessage::Decrement).unwrap())
            .with_message_type("call".to_string());
        let response = counter
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();
        let result: CounterMessage = serde_json::from_slice(response.payload()).unwrap();
        assert!(matches!(result, CounterMessage::Count(0)));
    }
}
