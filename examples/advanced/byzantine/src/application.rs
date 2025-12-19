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

//! Byzantine Generals Application
//!
//! Simple application wrapper that uses ByzantineAlgorithm

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use std::sync::Arc;

use crate::byzantine::ByzantineAlgorithm;
use crate::config::ByzantineConfig;

/// Byzantine Generals Application
pub struct ByzantineApplication {
    /// Configuration
    config: ByzantineConfig,
}

impl ByzantineApplication {
    /// Create new Byzantine Application from config
    pub fn from_config(config: ByzantineConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Create new Byzantine Application
    pub fn new(general_count: usize, byzantine_count: usize) -> Self {
        let config = ByzantineConfig {
            general_count,
            fault_count: byzantine_count,
            tuplespace_backend: "memory".to_string(),
            redis_url: None,
            postgres_url: None,
        };
        Self { config }
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
        
        let service_locator = node.service_locator()
            .ok_or_else(|| ApplicationError::StartupFailed("ServiceLocator not available from node".to_string()))?;
        
        // Wait for services to be registered
        for _ in 0..10 {
            if service_locator.get_service::<plexspaces_core::ActorRegistry>().await.is_some() {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        
        // Register ActorFactory
        use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
        use plexspaces_actor::ActorFactory;
        let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
        service_locator.register_service(actor_factory_impl.clone()).await;
        
        // Register BehaviorFactory
        use plexspaces_core::BehaviorRegistry;
        use crate::register_byzantine_behaviors;
        use plexspaces::journal::MemoryJournal;
        use plexspaces::tuplespace::TupleSpace;
        let mut behavior_registry = BehaviorRegistry::new();
        let journal = Arc::new(MemoryJournal::new());
        let tuplespace = Arc::new(TupleSpace::with_tenant_namespace("internal", "system"));
        register_byzantine_behaviors(&mut behavior_registry, journal, tuplespace).await;
        service_locator.register_service(Arc::new(behavior_registry)).await;
        
        // Register ActorService
        use plexspaces_node::service_wrappers::ActorServiceWrapper;
        use plexspaces_actor_service::ActorServiceImpl;
        let actor_service_impl = Arc::new(ActorServiceImpl::new(
            service_locator.clone(),
            node.id().to_string(),
        ));
        let actor_service_wrapper = Arc::new(ActorServiceWrapper::new(actor_service_impl));
        service_locator.register_actor_service(actor_service_wrapper).await;
        
        // Create actor context
        let ctx = plexspaces_core::ActorContext::new(
            node.id().to_string(),
            "internal".to_string(),
            "system".to_string(),
            service_locator.clone(),
            None,
        );
        let ctx_arc = Arc::new(ctx);
        
        // Spawn generals
        let source_id = 0;
        let num_rounds = 1;
        
        for general_id in 0..self.config.general_count {
            let actor_id = format!("general{}@{}", general_id, node.id());
            let initial_state = serde_json::json!({
                "id": general_id,
                "source_id": source_id,
                "num_rounds": num_rounds
            });
            
            let request_ctx = plexspaces_core::RequestContext::internal();
            actor_factory_impl.spawn_actor(
                &request_ctx,
                &actor_id,
                "ByzantineGeneral",
                serde_json::to_vec(&initial_state).unwrap(),
                None,
                std::collections::HashMap::new(),
            ).await
                .map_err(|e| ApplicationError::StartupFailed(format!("Failed to spawn general {}: {}", general_id, e)))?;
        }
        
        // Run the algorithm
        let algorithm = ByzantineAlgorithm::new(self.config.general_count, source_id, num_rounds)
            .map_err(|e| ApplicationError::StartupFailed(e))?;
        
        let (duration, total_messages, results) = algorithm.run(&ctx_arc).await
            .map_err(|e| ApplicationError::StartupFailed(e))?;
        
        // Print results
        println!("\nResults:");
        println!("Time: {}ms", duration);
        println!("Messages: {}", total_messages);
        
        for result in &results {
            let prefix = if result.is_source { "Source " } else { "" };
            if result.is_faulty {
                println!("{}Process {} is faulty", prefix, result.id);
            } else {
                let decision_str = match result.decision {
                    crate::byzantine::Value::Zero => "zero",
                    crate::byzantine::Value::One => "one",
                    crate::byzantine::Value::Retreat => "retreat",
                };
                println!("{}Process {} decides on value {}", prefix, result.id, decision_str);
            }
        }
        
        println!("✅ Byzantine Generals Application started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        println!("🛑 Stopping Byzantine Generals Application");
        println!("✅ Byzantine Generals Application stopped");
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
            general_count: 3,
            fault_count: 0,
            tuplespace_backend: "memory".to_string(),
            redis_url: None,
            postgres_url: None,
        };
        assert!(ByzantineApplication::from_config(config).is_err());
    }
}
