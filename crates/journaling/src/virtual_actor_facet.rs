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

//! Virtual Actor Facet (Orleans-inspired)
//!
//! ## Purpose
//! Provides automatic activation/deactivation for virtual actors. Virtual actors
//! are always addressable but not always in memory, enabling efficient resource
//! usage for large-scale actor systems.
//!
//! ## Architecture Context
//! Part of Phase 8.5: High Priority Missing Features. Implements Orleans-style
//! virtual actor lifecycle as an opt-in facet (not default).
//!
//! ## Design Decision
//! Virtual actor lifecycle is opt-in via facet to maintain simplicity:
//! - Regular actors: Explicit creation (simple, predictable)
//! - Virtual actors: Opt-in via facet (for microservices, user actors)
//!
//! ## How It Works
//! ```text
//! 1. Actor with VirtualActorFacet is created (virtual, not in memory)
//! 2. First message arrives → triggers activation
//! 3. Actor loads into memory, processes pending messages
//! 4. After idle_timeout → deactivates (removes from memory)
//! 5. Next message → reactivates
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::VirtualActorFacet;
//! use plexspaces_facet::Facet;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create virtual actor facet
//! let config = serde_json::json!({
//!     "idle_timeout": "5m",
//!     "activation_strategy": "lazy"
//! });
//!
//! let mut facet = VirtualActorFacet::new(config);
//! facet.on_attach("user-123", config).await?;
//!
//! // Actor is now virtual - can be messaged without explicit creation
//! // First message triggers activation
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Virtual Actor Facet for automatic activation/deactivation
///
/// ## Purpose
/// Implements Orleans-inspired virtual actor lifecycle. Actors with this facet
/// are always addressable but activated on-demand.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to lifecycle state.
pub struct VirtualActorFacet {
    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,

    /// Idle timeout before deactivation
    idle_timeout: Arc<RwLock<Duration>>,

    /// Activation strategy (lazy, eager, prewarm)
    activation_strategy: Arc<RwLock<ActivationStrategy>>,

    /// Last activation time
    last_activated: Arc<RwLock<Option<SystemTime>>>,

    /// Last access time
    last_accessed: Arc<RwLock<Option<SystemTime>>>,

    /// Activation count
    activation_count: Arc<RwLock<u32>>,

    /// Is currently activating (prevents duplicate activations)
    is_activating: Arc<RwLock<bool>>,
}

/// Activation strategy for virtual actors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationStrategy {
    /// Activate on first message (default)
    Lazy,
    /// Activate immediately on creation
    Eager,
    /// Pre-activate based on schedule
    Prewarm,
}

impl VirtualActorFacet {
    /// Create a new virtual actor facet
    ///
    /// ## Arguments
    /// * `config` - Facet configuration (idle_timeout, activation_strategy)
    ///
    /// ## Returns
    /// New VirtualActorFacet ready to attach to an actor
    pub fn new(config: Value) -> Self {
        let idle_timeout = config
            .get("idle_timeout")
            .and_then(|v| v.as_str())
            .and_then(|s| parse_duration(s))
            .unwrap_or(Duration::from_secs(300)); // Default: 5 minutes

        let activation_strategy = config
            .get("activation_strategy")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "eager" => ActivationStrategy::Eager,
                "prewarm" => ActivationStrategy::Prewarm,
                _ => ActivationStrategy::Lazy,
            })
            .unwrap_or(ActivationStrategy::Lazy);

        VirtualActorFacet {
            actor_id: Arc::new(RwLock::new(None)),
            idle_timeout: Arc::new(RwLock::new(idle_timeout)),
            activation_strategy: Arc::new(RwLock::new(activation_strategy)),
            last_activated: Arc::new(RwLock::new(None)),
            last_accessed: Arc::new(RwLock::new(None)),
            activation_count: Arc::new(RwLock::new(0)),
            is_activating: Arc::new(RwLock::new(false)),
        }
    }

    /// Get current lifecycle state
    pub async fn get_lifecycle_state(&self) -> VirtualActorLifecycleState {
        let last_activated = *self.last_activated.read().await;
        let last_accessed = *self.last_accessed.read().await;
        let activation_count = *self.activation_count.read().await;
        let is_activating = *self.is_activating.read().await;
        let idle_timeout = *self.idle_timeout.read().await;

        VirtualActorLifecycleState {
            last_activated,
            last_accessed,
            activation_count,
            is_activating,
            idle_timeout,
        }
    }

    /// Check if actor should be activated
    pub async fn should_activate(&self) -> bool {
        let is_activating = *self.is_activating.read().await;
        if is_activating {
            return false; // Already activating
        }

        let last_activated = *self.last_activated.read().await;
        last_activated.is_none() // Not yet activated
    }

    /// Check if actor should be deactivated (idle timeout exceeded)
    pub async fn should_deactivate(&self) -> bool {
        let last_accessed = *self.last_accessed.read().await;
        let idle_timeout = *self.idle_timeout.read().await;

        if let Some(last_access) = last_accessed {
            if let Ok(elapsed) = SystemTime::now().duration_since(last_access) {
                return elapsed >= idle_timeout;
            }
        }

        false
    }

    /// Mark actor as activated
    pub async fn mark_activated(&self) {
        let now = SystemTime::now();
        *self.last_activated.write().await = Some(now);
        *self.last_accessed.write().await = Some(now);
        *self.is_activating.write().await = false;
        *self.activation_count.write().await += 1;
    }

    /// Mark actor as accessed (updates last_accessed)
    pub async fn mark_accessed(&self) {
        *self.last_accessed.write().await = Some(SystemTime::now());
    }

    /// Mark actor as deactivated
    pub async fn mark_deactivated(&self) {
        *self.is_activating.write().await = false;
        // Keep last_activated and last_accessed for metrics
    }

    /// Start activation (set is_activating flag)
    pub async fn start_activation(&self) -> bool {
        let mut is_activating = self.is_activating.write().await;
        if *is_activating {
            return false; // Already activating
        }
        *is_activating = true;
        true
    }
    
    /// Get activation strategy
    pub async fn get_activation_strategy(&self) -> ActivationStrategy {
        *self.activation_strategy.read().await
    }
}

/// Lifecycle state for virtual actors
#[derive(Debug, Clone)]
pub struct VirtualActorLifecycleState {
    pub last_activated: Option<SystemTime>,
    pub last_accessed: Option<SystemTime>,
    pub activation_count: u32,
    pub is_activating: bool,
    pub idle_timeout: Duration,
}

#[async_trait]
impl Facet for VirtualActorFacet {
    fn facet_type(&self) -> &str {
        "virtual_actor"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError> {
        *self.actor_id.write().await = Some(actor_id.to_string());

        // Update config if provided
        if let Some(timeout_str) = config.get("idle_timeout").and_then(|v| v.as_str()) {
            if let Some(timeout) = parse_duration(timeout_str) {
                *self.idle_timeout.write().await = timeout;
            }
        }

        if let Some(strategy_str) = config.get("activation_strategy").and_then(|v| v.as_str()) {
            let strategy = match strategy_str {
                "eager" => ActivationStrategy::Eager,
                "prewarm" => ActivationStrategy::Prewarm,
                _ => ActivationStrategy::Lazy,
            };
            *self.activation_strategy.write().await = strategy;
        }

        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        *self.actor_id.write().await = None;
        Ok(())
    }

    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Mark as accessed (updates last_accessed timestamp)
        self.mark_accessed().await;

        // Check if activation needed (handled by Node, not here)
        // This facet just tracks lifecycle state

        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        _method: &str,
        _args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Mark as accessed after method completes
        self.mark_accessed().await;
        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        // Don't interfere with error handling
        Ok(ErrorHandling::Propagate)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        // Return lifecycle state as JSON
        // Note: This is a simplified version - full implementation would use tokio::runtime::Handle
        Ok(serde_json::json!({
            "facet_type": "virtual_actor",
            "note": "State access requires async context"
        }))
    }

    fn set_state(&mut self, _state: Value) -> Result<(), FacetError> {
        // Restore lifecycle state
        // Note: This is a simplified version - full implementation would use tokio::runtime::Handle
        Ok(())
    }
}

/// Parse duration string (e.g., "5m", "30s", "1h")
fn parse_duration(s: &str) -> Option<Duration> {
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = if s.ends_with('s') {
        (&s[..s.len() - 1], "s")
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], "m")
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], "h")
    } else {
        return None;
    };

    let num: u64 = num_str.parse().ok()?;

    match unit {
        "s" => Some(Duration::from_secs(num)),
        "m" => Some(Duration::from_secs(num * 60)),
        "h" => Some(Duration::from_secs(num * 3600)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_virtual_actor_facet_creation() {
        let config = serde_json::json!({
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        });

        let facet = VirtualActorFacet::new(config);
        assert_eq!(facet.facet_type(), "virtual_actor");

        let state = facet.get_lifecycle_state().await;
        assert_eq!(state.activation_count, 0);
        assert_eq!(state.is_activating, false);
        assert_eq!(state.idle_timeout, Duration::from_secs(300));
    }

    #[tokio::test]
    async fn test_virtual_actor_facet_attach() {
        let config = serde_json::json!({
            "idle_timeout": "10m",
            "activation_strategy": "eager"
        });

        let mut facet = VirtualActorFacet::new(serde_json::json!({}));
        facet.on_attach("actor-123", config).await.unwrap();

        let state = facet.get_lifecycle_state().await;
        assert_eq!(state.idle_timeout, Duration::from_secs(600));
    }

    #[tokio::test]
    async fn test_should_activate() {
        let facet = VirtualActorFacet::new(serde_json::json!({}));
        assert!(facet.should_activate().await);

        // After activation, should not activate again
        facet.mark_activated().await;
        assert!(!facet.should_activate().await);
    }

    #[tokio::test]
    async fn test_start_activation() {
        let facet = VirtualActorFacet::new(serde_json::json!({}));

        // First activation should succeed
        assert!(facet.start_activation().await);

        // Second activation should fail (already activating)
        assert!(!facet.start_activation().await);

        // After marking as activated, can activate again
        facet.mark_activated().await;
        assert!(facet.start_activation().await);
    }

    #[tokio::test]
    async fn test_should_deactivate() {
        let config = serde_json::json!({
            "idle_timeout": "1s"
        });

        let facet = VirtualActorFacet::new(config);
        facet.mark_activated().await;

        // Should not deactivate immediately
        assert!(!facet.should_deactivate().await);

        // Wait for idle timeout
        tokio::time::sleep(Duration::from_millis(1100)).await;

        // Should deactivate after idle timeout
        assert!(facet.should_deactivate().await);
    }

    #[tokio::test]
    async fn test_mark_accessed_resets_idle_timer() {
        let config = serde_json::json!({
            "idle_timeout": "1s"
        });

        let facet = VirtualActorFacet::new(config);
        facet.mark_activated().await;

        // Wait almost to timeout
        tokio::time::sleep(Duration::from_millis(900)).await;

        // Mark as accessed (resets timer)
        facet.mark_accessed().await;

        // Should not deactivate yet
        assert!(!facet.should_deactivate().await);

        // Wait for timeout again
        tokio::time::sleep(Duration::from_millis(1100)).await;

        // Now should deactivate
        assert!(facet.should_deactivate().await);
    }

    #[tokio::test]
    async fn test_activation_count() {
        let facet = VirtualActorFacet::new(serde_json::json!({}));

        let state = facet.get_lifecycle_state().await;
        assert_eq!(state.activation_count, 0);

        facet.mark_activated().await;
        let state = facet.get_lifecycle_state().await;
        assert_eq!(state.activation_count, 1);

        facet.mark_deactivated().await;
        facet.mark_activated().await;
        let state = facet.get_lifecycle_state().await;
        assert_eq!(state.activation_count, 2);
    }

    #[tokio::test]
    async fn test_parse_duration() {
        assert_eq!(parse_duration("5s"), Some(Duration::from_secs(5)));
        assert_eq!(parse_duration("10m"), Some(Duration::from_secs(600)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("invalid"), None);
        assert_eq!(parse_duration(""), None);
    }
}

