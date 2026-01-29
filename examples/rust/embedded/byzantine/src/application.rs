// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Byzantine Generals Application
//
// Demonstrates Application framework pattern with:
// - Application trait (like Erlang application)
// - BehaviorRegistry for custom behaviors
// - node.spawn() for actor creation

use async_trait::async_trait;
use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_core::{ActorId, BehaviorRegistry, RequestContext};
use std::sync::Arc;

use crate::config::ByzantineConfig;
use crate::general::General;

/// Byzantine Generals Application
pub struct ByzantineApplication {
    config: ByzantineConfig,
}

impl ByzantineApplication {
    /// Create from config
    pub fn from_config(config: ByzantineConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Create with defaults
    pub fn new(general_count: usize, byzantine_count: usize) -> Self {
        Self {
            config: ByzantineConfig {
                general_count,
                fault_count: byzantine_count,
                ..Default::default()
            },
        }
    }
}

#[async_trait]
impl Application for ByzantineApplication {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        println!("🚀 Starting Byzantine Generals Application");
        println!("   Generals: {}", self.config.general_count);
        println!("   Byzantine: {}", self.config.fault_count);

        // Get ServiceLocator from node
        let service_locator = node.service_locator()
            .ok_or_else(|| ApplicationError::StartupFailed(
                "ServiceLocator not available".to_string()
            ))?;

        // =====================================================================
        // Step 1: Register ByzantineGeneral behavior
        // =====================================================================
        // BehaviorRegistry maps actor_type -> behavior factory function
        // This is needed for spawn() to create the right behavior
        
        let mut behavior_registry = BehaviorRegistry::new();
        behavior_registry.register("ByzantineGeneral", |initial_state: &[u8]| {
            let state: serde_json::Value = serde_json::from_slice(initial_state)
                .map_err(|e| plexspaces_core::BehaviorFactoryError::InvalidArguments(
                    "ByzantineGeneral".to_string(),
                    format!("Invalid JSON: {}", e),
                ))?;
            
            let id = state["id"].as_u64().unwrap_or(0) as usize;
            let source_id = state["source_id"].as_u64().unwrap_or(0) as usize;
            let num_rounds = state["num_rounds"].as_u64().unwrap_or(1) as usize;
            
            Ok(Box::new(General::new(id, source_id, num_rounds)) as Box<dyn plexspaces_core::Actor>)
        }).await;
        
        // Use strongly-typed method (object-safe)
        service_locator.register_behavior_registry(Arc::new(behavior_registry)).await;

        // =====================================================================
        // Step 2: Spawn generals using ActorFactory
        // =====================================================================
        // Get ActorFactory from ServiceLocator and use spawn_actor()
        
        let ctx = RequestContext::new_without_auth(
            "byzantine".to_string(),
            "consensus".to_string(),
        );
        
        let source_id = 0usize;
        let num_rounds = 1usize;
        
        // Get ActorFactory from ServiceLocator
        let actor_factory = service_locator.get_actor_factory().await
            .ok_or_else(|| ApplicationError::StartupFailed(
                "ActorFactory not available".to_string()
            ))?;
        
        let mut general_ids = Vec::new();
        
        for i in 0..self.config.general_count {
            let actor_id = ActorId::from(format!("general-{}@{}", i, node.id()));
            let initial_state = serde_json::json!({
                "id": i,
                "source_id": source_id,
                "num_rounds": num_rounds,
            });
            
            // ActorFactory.spawn_actor() - creates actor with registered behavior
            let _actor = actor_factory.spawn_actor(
                &ctx,
                &actor_id,
                "ByzantineGeneral",
                serde_json::to_vec(&initial_state).unwrap(),
                None,
                std::collections::HashMap::new(),
                vec![],
            ).await
            .map_err(|e| ApplicationError::StartupFailed(
                format!("Failed to spawn general {}: {}", i, e)
            ))?;
            
            general_ids.push(actor_id);
            println!("  ✓ Spawned general-{}", i);
        }

        // =====================================================================
        // Step 3: Run consensus protocol
        // =====================================================================
        println!("\nRunning consensus...");
        
        // For this example, we demonstrate the pattern without full message passing
        // In a real implementation, generals would communicate via ActorRef::tell/ask
        
        // Simulate voting results
        let honest_count = self.config.general_count - self.config.fault_count;
        let quorum = (2 * self.config.general_count) / 3;
        
        println!("\nResults:");
        println!("  Honest generals: {}", honest_count);
        println!("  Byzantine (faulty): {}", self.config.fault_count);
        println!("  Quorum needed: {}", quorum);
        
        if honest_count >= quorum {
            println!("  ✅ Consensus REACHED");
        } else {
            println!("  ❌ Consensus FAILED (too many byzantine nodes)");
        }

        println!("\n✅ Byzantine Generals Application started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        println!("🛑 Stopping Byzantine Generals Application");
        println!("✅ Stopped");
        Ok(())
    }

    fn name(&self) -> &str {
        "byzantine-generals"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_application() {
        let app = ByzantineApplication::new(4, 1);
        assert_eq!(app.name(), "byzantine-generals");
    }

    #[test]
    fn test_invalid_config() {
        let config = ByzantineConfig {
            general_count: 3, // Invalid: needs at least 4
            fault_count: 0,
            ..Default::default()
        };
        assert!(ByzantineApplication::from_config(config).is_err());
    }
}
