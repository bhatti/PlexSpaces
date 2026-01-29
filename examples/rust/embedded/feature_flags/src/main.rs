// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Feature Flags Example (Config Updates via Process Groups)
//
// Demonstrates distributed configuration updates:
// - Central feature flag store
// - Services subscribe for flag changes
// - Instant propagation across all subscribers
//
// Use Case: Feature toggles, A/B testing, gradual rollouts

use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::{ActorId, RequestContext};
use std::collections::HashMap;
use std::sync::Arc;

// =============================================================================
// Feature Flag Store (simplified)
// =============================================================================

struct FeatureFlagStore {
    flags: HashMap<String, bool>,
    registry: ProcessGroupRegistry,
    tenant_id: String,
    namespace: String,
}

impl FeatureFlagStore {
    fn new(registry: ProcessGroupRegistry, tenant_id: &str, namespace: &str) -> Self {
        Self {
            flags: HashMap::new(),
            registry,
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    fn get(&self, flag: &str) -> bool {
        *self.flags.get(flag).unwrap_or(&false)
    }

    async fn set(&mut self, flag: &str, value: bool) -> Result<Vec<ActorId>, Box<dyn std::error::Error>> {
        let old_value = self.flags.insert(flag.to_string(), value);
        
        // Only broadcast if value changed
        if old_value != Some(value) {
            let ctx = RequestContext::new_without_auth(
                self.tenant_id.clone(),
                self.namespace.clone(),
            );
            
            // Broadcast flag change to all subscribers
            let message = format!("{}={}", flag, value).into_bytes();
            let recipients = self.registry
                .publish_to_group(&ctx, "feature-flags", None, message)
                .await?;
            
            return Ok(recipients);
        }
        
        Ok(vec![])
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           Feature Flags Example (Config Updates)               ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Instant feature toggle propagation across services");
    println!();

    // Create backend
    let kv_store = Arc::new(InMemoryKVStore::new());
    let registry = ProcessGroupRegistry::new("config-server", kv_store);

    let tenant_id = "acme-corp";
    let namespace = "config";
    
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // =========================================================================
    // Step 1: Create feature-flags group
    // =========================================================================
    println!("Step 1: Create feature flags group");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    registry.create_group(&ctx, "feature-flags").await?;
    println!("  Created 'feature-flags' group");
    println!();

    // =========================================================================
    // Step 2: Services subscribe for flag updates
    // =========================================================================
    println!("Step 2: Services subscribe for flag updates");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let services = vec![
        ActorId::from("api-gateway@node-1"),
        ActorId::from("user-service@node-2"),
        ActorId::from("billing-service@node-2"),
        ActorId::from("notification-service@node-3"),
    ];
    
    for service in &services {
        registry.join_group(&ctx, "feature-flags", service, vec![]).await?;
        println!("  {} subscribed", service);
    }
    println!();

    // Initialize flag store
    let mut flags = FeatureFlagStore::new(registry.clone(), tenant_id, namespace);

    // =========================================================================
    // Step 3: Admin enables "dark_mode" feature
    // =========================================================================
    println!("Step 3: Admin enables 'dark_mode' feature");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let recipients = flags.set("dark_mode", true).await?;
    
    println!("  Flag: dark_mode = true");
    println!("  Broadcasted to {} services:", recipients.len());
    for r in &recipients {
        println!("    -> {}", r);
    }
    println!();

    // =========================================================================
    // Step 4: Services check the flag
    // =========================================================================
    println!("Step 4: Services check the flag");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    println!("  dark_mode = {}", flags.get("dark_mode"));
    println!("  new_checkout = {} (default)", flags.get("new_checkout"));
    println!();

    // =========================================================================
    // Step 5: Enable gradual rollout (new_checkout)
    // =========================================================================
    println!("Step 5: Enable 'new_checkout' for A/B test");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let recipients = flags.set("new_checkout", true).await?;
    
    println!("  Flag: new_checkout = true");
    println!("  Broadcasted to {} services:", recipients.len());
    for r in &recipients {
        println!("    -> {}", r);
    }
    println!();

    // =========================================================================
    // Step 6: Disable feature (rollback)
    // =========================================================================
    println!("Step 6: Disable 'new_checkout' (rollback)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let recipients = flags.set("new_checkout", false).await?;
    
    println!("  Flag: new_checkout = false (rolled back)");
    println!("  Broadcasted to {} services:", recipients.len());
    for r in &recipients {
        println!("    -> {}", r);
    }
    println!();

    // =========================================================================
    // Step 7: No broadcast when value unchanged
    // =========================================================================
    println!("Step 7: No broadcast when value unchanged");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let recipients = flags.set("dark_mode", true).await?;
    
    println!("  Flag: dark_mode = true (same value)");
    println!("  Recipients: {} (no broadcast needed)", recipients.len());
    println!();

    // =========================================================================
    // Step 8: Service leaves (unsubscribes)
    // =========================================================================
    println!("Step 8: Service unsubscribes");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let billing = ActorId::from("billing-service@node-2");
    registry.leave_group(&ctx, "feature-flags", &billing).await?;
    println!("  billing-service unsubscribed");
    
    let recipients = flags.set("dark_mode", false).await?;
    println!("  Flag: dark_mode = false");
    println!("  Broadcasted to {} services (billing excluded):", recipients.len());
    for r in &recipients {
        println!("    -> {}", r);
    }
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Feature Flags Example Complete");
    println!();
    println!("Key Concepts:");
    println!("  - Config as pub/sub: Services subscribe to config group");
    println!("  - Instant propagation: Changes broadcast immediately");
    println!("  - Selective updates: Only changed values trigger broadcast");
    println!();
    println!("PlexSpaces APIs Used:");
    println!("  - ProcessGroupRegistry: Manages subscriber groups");
    println!("  - publish_to_group(): Broadcasts config changes");
    println!("  - join_group() / leave_group(): Subscribe/unsubscribe");
    println!();
    println!("Use Cases:");
    println!("  - Feature flags / toggles");
    println!("  - A/B testing configuration");
    println!("  - Gradual rollouts (canary deploys)");
    println!("  - Kill switches");
    println!("  - Rate limit updates");
    println!();

    Ok(())
}
