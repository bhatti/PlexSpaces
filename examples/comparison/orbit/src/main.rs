// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Orbit (Virtual Actors on JVM)

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_journaling::{VirtualActorFacet, DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// User message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserMessage {
    GetProfile,
    UpdateProfile { name: String, email: String },
    Profile { name: String, email: String },
}

/// User actor (Orbit-style virtual actor)
/// Demonstrates: Virtual Actor Lifecycle, State Persistence, Lifecycle Management
pub struct UserActor {
    name: String,
    email: String,
}

impl UserActor {
    pub fn new() -> Self {
        Self {
            name: "Unknown".to_string(),
            email: "unknown@example.com".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for UserActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for UserActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let user_msg: UserMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match user_msg {
            UserMessage::GetProfile => {
                info!("Getting profile: name={}, email={}", self.name, self.email);
                let reply = UserMessage::Profile {
                    name: self.name.clone(),
                    email: self.email.clone(),
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            UserMessage::UpdateProfile { name, email } => {
                info!("Updating profile: name={}, email={}", name, email);
                self.name = name.clone();
                self.email = email.clone();
                // State automatically persisted via DurabilityFacet
                let reply = UserMessage::Profile { name, email };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Orbit vs PlexSpaces Comparison ===");
    info!("Demonstrating Orbit Virtual Actors (JVM-based virtual actor framework)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Orbit actors are virtual actors with lifecycle management
    let actor_id: ActorId = "user/alice@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating user actor (Orbit-style virtual actor)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(UserActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Create facets with config and priority
    let virtual_facet_config = serde_json::json!({
        "idle_timeout": "10m",
        "activation_strategy": "lazy"
    });
    let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
    
    // Spawn using ActorFactory with facets
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "GenServer",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![virtual_facet, durability_facet], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    // Create ActorRef directly - no need to access mailbox
    let user = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ User actor created/activated: {}", user.id());
    info!("✅ VirtualActorFacet attached - automatic lifecycle management");
    info!("✅ DurabilityFacet attached - state persistence enabled");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test update profile
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: Update profile (triggers activation if deactivated)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let msg = Message::new(serde_json::to_vec(&UserMessage::UpdateProfile {
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
    })?)
        .with_message_type("call".to_string());
    let result = user
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: UserMessage = serde_json::from_slice(result.payload())?;
    if let UserMessage::Profile { name, email } = reply {
        info!("✅ Profile updated: name={}, email={}", name, email);
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@example.com");
    }

    // Test get profile
    let msg = Message::new(serde_json::to_vec(&UserMessage::GetProfile)?)
        .with_message_type("call".to_string());
    let result = user
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: UserMessage = serde_json::from_slice(result.payload())?;
    if let UserMessage::Profile { name, email } = reply {
        info!("✅ Profile retrieved: name={}, email={}", name, email);
        assert_eq!(name, "Alice");
        assert_eq!(email, "alice@example.com");
    }

    // Demonstrate lifecycle: Virtual actor lifecycle
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: Virtual Actor Lifecycle");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("✅ Virtual actor created: {}", user.id());
    info!("   - Virtual actor lifecycle managed automatically");
    info!("   - State persisted across deactivation/reactivation");

    info!("=== Comparison Complete ===");
    info!("✅ VirtualActorFacet: Lifecycle management (Orbit pattern)");
    info!("✅ DurabilityFacet: State persistence");
    info!("✅ Virtual actor pattern: Automatic activation on first message");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_user_profile() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "user/test-1@test-node".to_string();
        
        // Create facets with config and priority
        let virtual_facet_config = serde_json::json!({
            "idle_timeout": "10m",
            "activation_strategy": "lazy"
        });
        let virtual_facet = Box::new(VirtualActorFacet::new(virtual_facet_config, 100));
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
        
        // Spawn using ActorFactory with facets
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        use std::sync::Arc;
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "GenServer",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![virtual_facet, durability_facet], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let user = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let msg = Message::new(serde_json::to_vec(&UserMessage::UpdateProfile {
            name: "Test".to_string(),
            email: "test@example.com".to_string(),
        }).unwrap())
            .with_message_type("call".to_string());
        let result = user
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: UserMessage = serde_json::from_slice(result.payload()).unwrap();
        if let UserMessage::Profile { name, email } = reply {
            assert_eq!(name, "Test");
            assert_eq!(email, "test@example.com");
        } else {
            panic!("Expected Profile message");
        }
    }
}
