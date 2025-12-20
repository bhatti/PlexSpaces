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

//! Dynamic Facet System for Runtime Behavior Composition
//!
//! Facets allow actors to gain new capabilities at runtime without inheritance or recompilation.
//! This is inspired by the facet pattern where secondary objects (facets) can be dynamically
//! attached to primary objects (actors) to extend their behavior.

// event_emitter and capabilities are declared in lib.rs

use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A facet that can be dynamically attached to actors
#[async_trait]
pub trait Facet: Send + Sync + Any {
    /// Unique identifier for this facet type
    fn facet_type(&self) -> &str;
    
    /// Get reference to self as Any (for downcasting)
    fn as_any(&self) -> &dyn Any;
    
    /// Get mutable reference to self as Any (for downcasting)
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Called when facet is attached to an actor
    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError>;

    /// Called when facet is detached from an actor
    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError>;

    /// Intercept a method call before it reaches the actor
    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        Ok(InterceptResult::Continue)
    }

    /// Intercept a method call after the actor processes it
    async fn after_method(
        &self,
        _method: &str,
        _args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        Ok(InterceptResult::Continue)
    }

    /// Handle errors from the actor
    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        Ok(ErrorHandling::Propagate)
    }

    /// Get current facet state (for persistence/migration)
    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(Value::Null)
    }

    /// Restore facet state
    fn set_state(&mut self, _state: Value) -> Result<(), FacetError> {
        Ok(())
    }

    /// Get facet configuration (immutable)
    fn get_config(&self) -> Value;

    /// Get facet priority
    fn get_priority(&self) -> i32;
}

/// Result of intercepting a method
#[derive(Debug)]
pub enum InterceptResult {
    /// Continue to next facet or actor
    Continue,
    /// Replace the arguments
    ReplaceArgs(Vec<u8>),
    /// Replace the result
    ReplaceResult(Vec<u8>),
    /// Stop processing and return early
    ShortCircuit(Vec<u8>),
}

/// How to handle errors
#[derive(Debug)]
pub enum ErrorHandling {
    /// Propagate the error
    Propagate,
    /// Retry the operation with maximum attempts
    Retry {
        /// Maximum number of retry attempts
        max_attempts: u32,
    },
    /// Replace with default value
    Default(Vec<u8>),
    /// Transform the error
    Transform(String),
}

/// Error type for facet operations
#[derive(Debug, thiserror::Error)]
pub enum FacetError {
    /// Facet with given type not found
    #[error("Facet not found: {0}")]
    NotFound(String),

    /// Facet of this type already attached
    #[error("Facet already attached: {0}")]
    AlreadyAttached(String),

    /// Invalid facet configuration provided
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Failed to attach facet to actor
    #[error("Attachment failed: {0}")]
    AttachmentFailed(String),

    /// Failed to detach facet from actor
    #[error("Detachment failed: {0}")]
    DetachmentFailed(String),

    /// Message interception processing failed
    #[error("Interception failed: {0}")]
    InterceptionFailed(String),
}

/// Container for facets attached to an actor
pub struct FacetContainer {
    facets: Vec<Arc<RwLock<Box<dyn Facet>>>>,
    metadata: HashMap<String, FacetMetadata>,
}

/// Metadata about a facet attached to an actor
#[derive(Clone, Debug)]
pub struct FacetMetadata {
    pub facet_type: String,
    pub priority: i32,
    pub attached_at: std::time::Instant,
    pub config: Value,
}

impl Default for FacetContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl FacetContainer {
    /// Creates a new empty facet container
    pub fn new() -> Self {
        FacetContainer {
            facets: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Attach a facet (config and priority are extracted from facet)
    pub async fn attach(
        &mut self,
        mut facet: Box<dyn Facet>,
        actor_id: &str,
    ) -> Result<String, FacetError> {
        let span = tracing::span!(tracing::Level::DEBUG, "facet.attach", facet_type = %facet.facet_type(), actor_id = %actor_id);
        let _enter = span.enter();
        
        let facet_type = facet.facet_type().to_string();
        metrics::counter!("plexspaces_facet_attach_attempts_total", "facet_type" => facet_type.clone()).increment(1);
        let start = std::time::Instant::now();

        // Check if already attached
        if self.metadata.contains_key(&facet_type) {
            metrics::counter!("plexspaces_facet_attach_errors_total", "facet_type" => facet_type.clone(), "error" => "already_attached").increment(1);
            tracing::warn!(facet_type = %facet_type, "Facet already attached");
            return Err(FacetError::AlreadyAttached(facet_type));
        }

        // Extract config and priority from facet
        let config = facet.get_config();
        let priority = facet.get_priority();

        // Create locked facet
        facet.on_attach(actor_id, config.clone()).await?;

        let facet_arc = Arc::new(RwLock::new(facet));

        // Insert based on priority (higher priority first)
        let insert_pos = self
            .facets
            .iter()
            .position(|f| {
                // We need to check priority, but facets are locked
                // For now, insert at end and sort later if needed
                false
            })
            .unwrap_or(self.facets.len());

        self.facets.insert(insert_pos, facet_arc);

        // Store metadata
        self.metadata.insert(
            facet_type.clone(),
            FacetMetadata {
                facet_type: facet_type.clone(),
                priority: priority,
                attached_at: std::time::Instant::now(),
                config: config,
            },
        );

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_attach_duration_seconds", "facet_type" => facet_type.clone()).record(duration.as_secs_f64());
        metrics::counter!("plexspaces_facet_attached_total", "facet_type" => facet_type.clone()).increment(1);
        metrics::gauge!("plexspaces_facet_active_total", "facet_type" => facet_type.clone()).increment(1.0);
        tracing::info!(facet_type = %facet_type, actor_id = %actor_id, priority = priority, "Facet attached");

        Ok(facet_type)
    }

    /// Detach a facet
    pub async fn detach(&mut self, facet_type: &str, actor_id: &str) -> Result<(), FacetError> {
        let span = tracing::span!(tracing::Level::DEBUG, "facet.detach", facet_type = %facet_type, actor_id = %actor_id);
        let _enter = span.enter();
        
        metrics::counter!("plexspaces_facet_detach_attempts_total", "facet_type" => facet_type.to_string()).increment(1);
        let start = std::time::Instant::now();

        // Find and remove facet
        if self.metadata.remove(facet_type).is_none() {
            metrics::counter!("plexspaces_facet_detach_errors_total", "facet_type" => facet_type.to_string(), "error" => "not_found").increment(1);
            tracing::warn!(facet_type = %facet_type, "Facet not found for detach");
            return Err(FacetError::NotFound(facet_type.to_string()));
        }

        // Find facet in list
        let pos = self
            .facets
            .iter()
            .position(|f| f.blocking_read().facet_type() == facet_type);

        if let Some(pos) = pos {
            let facet = self.facets.remove(pos);
            let mut facet = facet.write().await;
            facet.on_detach(actor_id).await?;
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_detach_duration_seconds", "facet_type" => facet_type.to_string()).record(duration.as_secs_f64());
        metrics::counter!("plexspaces_facet_detached_total", "facet_type" => facet_type.to_string()).increment(1);
        metrics::gauge!("plexspaces_facet_active_total", "facet_type" => facet_type.to_string()).decrement(1.0);
        tracing::info!(facet_type = %facet_type, actor_id = %actor_id, "Facet detached");

        Ok(())
    }

    /// Execute before interceptors
    pub async fn intercept_before(&self, method: &str, args: &[u8]) -> Result<Vec<u8>, FacetError> {
        let span = tracing::span!(tracing::Level::TRACE, "facet.intercept_before", method = %method);
        let _enter = span.enter();
        
        metrics::counter!("plexspaces_facet_intercept_before_total", "method" => method.to_string()).increment(1);
        let start = std::time::Instant::now();
        
        let mut current_args = args.to_vec();

        for facet in &self.facets {
            let facet = facet.read().await;
            match facet.before_method(method, &current_args).await? {
                InterceptResult::Continue => {}
                InterceptResult::ReplaceArgs(new_args) => {
                    current_args = new_args;
                    tracing::trace!(facet_type = %facet.facet_type(), "Facet replaced args");
                }
                InterceptResult::ShortCircuit(result) => {
                    let duration = start.elapsed();
                    metrics::histogram!("plexspaces_facet_intercept_before_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_facet_intercept_shortcircuit_total", "method" => method.to_string(), "facet_type" => facet.facet_type().to_string()).increment(1);
                    tracing::debug!(facet_type = %facet.facet_type(), "Facet short-circuited");
                    return Ok(result);
                }
                _ => {}
            }
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_intercept_before_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());
        
        Ok(current_args)
    }

    /// Execute after interceptors
    pub async fn intercept_after(
        &self,
        method: &str,
        args: &[u8],
        result: &[u8],
    ) -> Result<Vec<u8>, FacetError> {
        let span = tracing::span!(tracing::Level::TRACE, "facet.intercept_after", method = %method);
        let _enter = span.enter();
        
        metrics::counter!("plexspaces_facet_intercept_after_total", "method" => method.to_string()).increment(1);
        let start = std::time::Instant::now();
        
        let mut current_result = result.to_vec();

        // Run in reverse order for after interceptors
        for facet in self.facets.iter().rev() {
            let facet = facet.read().await;
            match facet.after_method(method, args, &current_result).await? {
                InterceptResult::Continue => {}
                InterceptResult::ReplaceResult(new_result) => {
                    current_result = new_result;
                    tracing::trace!(facet_type = %facet.facet_type(), "Facet replaced result");
                }
                _ => {}
            }
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_intercept_after_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());
        
        Ok(current_result)
    }

    /// List attached facets
    pub fn list_facets(&self) -> Vec<String> {
        self.metadata.keys().cloned().collect()
    }
    
    /// Get a facet by type (for FacetService - Option B)
    ///
    /// ## Arguments
    /// * `facet_type` - Facet type identifier
    ///
    /// ## Returns
    /// Arc to the facet if found, None otherwise
    pub fn get_facet(&self, facet_type: &str) -> Option<Arc<RwLock<Box<dyn Facet>>>> {
        // Find facet by type
        for facet_arc in &self.facets {
            // Try to read facet type (non-blocking check)
            // Note: This is a bit inefficient, but necessary for type matching
            if let Ok(facet_guard) = facet_arc.try_read() {
                if facet_guard.facet_type() == facet_type {
                    return Some(facet_arc.clone());
                }
            }
        }
        None
    }
}

/// Registry for available facet implementations
pub struct FacetRegistry {
    factories: HashMap<String, Arc<dyn FacetFactory>>,
}

/// Factory for creating facet instances
#[async_trait]
pub trait FacetFactory: Send + Sync {
    /// Create a new facet instance
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError>;

    /// Get facet metadata
    fn metadata(&self) -> FacetMetadata;
}

impl Default for FacetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FacetRegistry {
    /// Creates a new empty facet registry
    pub fn new() -> Self {
        FacetRegistry {
            factories: HashMap::new(),
        }
    }

    /// Register a facet factory
    pub fn register(&mut self, facet_type: String, factory: Arc<dyn FacetFactory>) {
        self.factories.insert(facet_type, factory);
    }

    /// Create a facet instance
    pub async fn create_facet(
        &self,
        facet_type: &str,
        config: Value,
    ) -> Result<Box<dyn Facet>, FacetError> {
        let factory = self
            .factories
            .get(facet_type)
            .ok_or_else(|| FacetError::NotFound(facet_type.to_string()))?;

        factory.create(config).await
    }

    /// List available facet types
    pub fn list_types(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }
}

// Example facets demonstrating the pattern

/// Logging facet - logs all method calls
pub struct LoggingFacet {
    config: Value,
    priority: i32,
    level: String,
}

/// Default priority for LoggingFacet
pub const LOGGING_FACET_DEFAULT_PRIORITY: i32 = 900;

impl LoggingFacet {
    /// Create a new logging facet
    pub fn new(config: Value, priority: i32) -> Self {
        let level = config
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("INFO")
            .to_string();
        LoggingFacet {
            config,
            priority,
            level,
        }
    }
}

#[async_trait]
impl Facet for LoggingFacet {
    fn facet_type(&self) -> &str {
        "logging"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        println!("Logging facet attached to actor {}", actor_id);
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        println!("Logging facet detached from actor {}", actor_id);
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        println!(
            "[{}] Calling method: {} with {} bytes",
            self.level,
            method,
            args.len()
        );
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        println!(
            "[{}] Method {} returned {} bytes",
            self.level,
            method,
            _result.len()
        );
        Ok(InterceptResult::Continue)
    }
    
    fn get_config(&self) -> Value {
        self.config.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Caching facet - caches method results
pub struct CachingFacet {
    config: Value,
    priority: i32,
    cache: HashMap<String, Vec<u8>>,
    ttl: std::time::Duration,
}

/// Default priority for CachingFacet
pub const CACHING_FACET_DEFAULT_PRIORITY: i32 = 40;

impl CachingFacet {
    /// Create a new caching facet
    pub fn new(config: Value, priority: i32) -> Self {
        let ttl = config
            .get("ttl_seconds")
            .and_then(|v| v.as_u64())
            .map(|s| std::time::Duration::from_secs(s))
            .unwrap_or(std::time::Duration::from_secs(300));
        CachingFacet {
            config,
            priority,
            cache: HashMap::new(),
            ttl,
        }
    }
}

#[async_trait]
impl Facet for CachingFacet {
    fn facet_type(&self) -> &str {
        "caching"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        self.cache.clear();
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Create cache key
        let key = format!("{}:{}", method, hex::encode(args));

        // Check cache
        if let Some(cached) = self.cache.get(&key) {
            println!("Cache hit for {}", method);
            return Ok(InterceptResult::ShortCircuit(cached.clone()));
        }

        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Cache the result
        let _key = format!("{}:{}", method, hex::encode(args));
        // In real implementation, we'd need mutable self or interior mutability
        // self.cache.insert(key, result.to_vec());

        Ok(InterceptResult::Continue)
    }
    
    fn get_config(&self) -> Value {
        self.config.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Metrics facet - tracks method performance
pub struct MetricsFacet {
    config: Value,
    priority: i32,
    metrics: HashMap<String, MethodMetrics>,
}

/// Default priority for MetricsFacet
pub const METRICS_FACET_DEFAULT_PRIORITY: i32 = 800;

impl MetricsFacet {
    /// Create a new metrics facet
    pub fn new(config: Value, priority: i32) -> Self {
        MetricsFacet {
            config,
            priority,
            metrics: HashMap::new(),
        }
    }
}

#[derive(Default)]
struct MethodMetrics {
    count: u64,
    total_time_ms: u64,
    errors: u64,
}

#[async_trait]
impl Facet for MetricsFacet {
    fn facet_type(&self) -> &str {
        "metrics"
    }
    
    fn as_any(&self) -> &dyn Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Log final metrics
        for (method, metrics) in &self.metrics {
            let avg_time = if metrics.count > 0 {
                metrics.total_time_ms / metrics.count
            } else {
                0
            };
            println!(
                "Method {}: {} calls, avg {}ms, {} errors",
                method, metrics.count, avg_time, metrics.errors
            );
        }
        Ok(())
    }

    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Start timing (would need to store start time somewhere)
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Update metrics (would need mutable self)
        println!("Recording metrics for {}", method);
        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        // Increment error count
        println!("Error in method {}", method);
        Ok(ErrorHandling::Propagate)
    }
    
    fn get_config(&self) -> Value {
        self.config.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_facet_container() {
        let mut container = FacetContainer::new();

        // Create and attach a logging facet
        let config = serde_json::json!({
            "level": "DEBUG"
        });
        let facet = Box::new(LoggingFacet::new(config.clone(), 10));

        let facet_id = container
            .attach(facet, "test_actor")
            .await
            .unwrap();

        assert_eq!(facet_id, "logging");
        assert_eq!(container.list_facets(), vec!["logging"]);

        // Test interception
        let args = b"test args";
        let result = container
            .intercept_before("test_method", args)
            .await
            .unwrap();
        assert_eq!(result, args);
    }

    #[tokio::test]
    async fn test_facet_registry() {
        let mut registry = FacetRegistry::new();

        // Register a factory (would need to implement FacetFactory)
        // registry.register("logging", Arc::new(LoggingFacetFactory));

        let types = registry.list_types();
        assert_eq!(types.len(), 0); // No factories registered in this test
    }
}
