// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Process Group Capability Facet
//!
//! ## Purpose
//! Provides distributed pub/sub capabilities to actors as a runtime-attachable facet.
//! This follows the same pattern as KeyValueFacet and LockFacet - actors send messages
//! with specific message types, and the facet intercepts and handles them using the
//! real ProcessGroupRegistry backend.
//!
//! ## Design Pattern
//! - **Message Interception**: Facet intercepts messages with types like `"create_group"`, `"join_group"`, etc.
//! - **Short-Circuit Handling**: Facet handles the operation and returns result without calling the actor
//! - **Works for Rust and WASM**: Both Rust and WASM actors send messages, facet handles them uniformly
//! - **Uses Real Backend**: Wraps ProcessGroupRegistry from ServiceLocator (based on node config)
//!
//! ## Message Types
//! - `"create_group"`: Create a new process group
//! - `"join_group"`: Join a process group (with optional topics)
//! - `"leave_group"`: Leave a process group
//! - `"get_members"`: Get all members (cluster-wide)
//! - `"get_local_members"`: Get local members only
//! - `"list_groups"`: List all groups
//! - `"publish_to_group"`: Publish message to group members

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::{Facet, FacetError, InterceptResult};
use plexspaces_common::{RequestContext, RequestContextExt};
use tracing::{debug, error, instrument, warn};

/// Default priority for ProcessGroupFacet
pub const PROCESS_GROUP_FACET_DEFAULT_PRIORITY: i32 = 30;

/// Configuration for process group facet
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessGroupConfig {
    /// Default topics if not specified in join
    pub default_topics: Option<Vec<String>>,
}

/// Trait for process group registry implementations (to avoid circular dependency)
#[async_trait]
pub trait ProcessGroupRegistry: Send + Sync {
    /// Create a new process group
    async fn create_group(&self, ctx: &RequestContext, group_name: &str) -> Result<(), String>;

    /// Join a process group
    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), String>;

    /// Leave a process group
    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), String>;

    /// Get all members of a group (cluster-wide)
    async fn get_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String>;

    /// Get local members of a group (this node only)
    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String>;

    /// List all groups
    async fn list_groups(&self, ctx: &RequestContext) -> Result<Vec<String>, String>;

    /// Publish message to group members
    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Vec<u8>,
    ) -> Result<Vec<String>, String>; // Returns recipient actor IDs
}

/// Process group capability facet
///
/// ## Purpose
/// Provides distributed pub/sub to actors via message interception.
/// Actors send messages with process group operation types, facet handles them using ProcessGroupRegistry.
pub struct ProcessGroupFacet {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    /// Facet priority (immutable)
    priority: i32,
    /// Process group registry implementation
    process_group_registry: Arc<dyn ProcessGroupRegistry>,
    /// Configuration (parsed from config_value)
    config: ProcessGroupConfig,
}

impl ProcessGroupFacet {
    /// Create a new process group facet
    ///
    /// ## Arguments
    /// * `process_group_registry` - Process group registry backend (implements ProcessGroupRegistry trait)
    /// * `config` - Facet configuration JSON
    /// * `priority` - Facet priority
    pub fn new(
        process_group_registry: Arc<dyn ProcessGroupRegistry>,
        config: Value,
        priority: i32,
    ) -> Self {
        let config_clone = config.clone();
        let pg_config = serde_json::from_value::<ProcessGroupConfig>(config_clone)
            .unwrap_or_else(|_| ProcessGroupConfig::default());

        ProcessGroupFacet {
            config_value: config,
            priority,
            process_group_registry,
            config: pg_config,
        }
    }

    /// Handle process group operations with observability
    #[instrument(skip(self, args), fields(operation = method))]
    async fn handle_process_group_operation(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, FacetError> {
        let start = Instant::now();
        // Create request context - process_groups facet doesn't have access to ServiceLocator or actor context
        // TODO: Extract tenant/namespace from actor context when available (similar to RegistryFacet)
        // For now, use empty strings (process groups will work but without tenant/namespace isolation)
        let ctx = RequestContext::new_without_auth(String::new(), String::new());

        let result = match method {
            "create_group" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "create_group").increment(1);

                let group_name: String = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "create_group", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                match self
                    .process_group_registry
                    .create_group(&ctx, &group_name)
                    .await
                {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "create_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "create_group").increment(1);
                        debug!(group_name = %group_name, duration_ms = duration.as_millis(), "Process group created");

                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "create_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "create_group", "error" => "creation_failed").increment(1);
                        error!(group_name = %group_name, error = %e, duration_ms = duration.as_millis(), "Failed to create process group");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "join_group" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "join_group").increment(1);
                #[derive(Deserialize)]
                struct JoinArgs {
                    group_name: String,
                    actor_id: String,
                    topics: Option<Vec<String>>,
                }

                let args: JoinArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let topics = args
                    .topics
                    .or_else(|| self.config.default_topics.clone())
                    .unwrap_or_default();

                match self
                    .process_group_registry
                    .join_group(&ctx, &args.group_name, &args.actor_id, topics)
                    .await
                {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "join_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "join_group").increment(1);
                        debug!(group_name = %args.group_name, actor_id = %args.actor_id, duration_ms = duration.as_millis(), "Actor joined process group");

                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "join_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "join_group", "error" => "join_failed").increment(1);
                        error!(group_name = %args.group_name, actor_id = %args.actor_id, error = %e, duration_ms = duration.as_millis(), "Failed to join process group");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "leave_group" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "leave_group").increment(1);
                #[derive(Deserialize)]
                struct LeaveArgs {
                    group_name: String,
                    actor_id: String,
                }

                let args: LeaveArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                match self
                    .process_group_registry
                    .leave_group(&ctx, &args.group_name, &args.actor_id)
                    .await
                {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "leave_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "leave_group").increment(1);
                        debug!(group_name = %args.group_name, actor_id = %args.actor_id, duration_ms = duration.as_millis(), "Actor left process group");

                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "leave_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "leave_group", "error" => "leave_failed").increment(1);
                        error!(group_name = %args.group_name, actor_id = %args.actor_id, error = %e, duration_ms = duration.as_millis(), "Failed to leave process group");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "get_members" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "get_members").increment(1);

                let group_name: String = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "get_members", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                match self
                    .process_group_registry
                    .get_members(&ctx, &group_name)
                    .await
                {
                    Ok(members) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "get_members").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "get_members").increment(1);
                        metrics::gauge!("plexspaces_facet_process_groups_member_count", "group" => group_name.clone()).set(members.len() as f64);
                        debug!(group_name = %group_name, member_count = members.len(), duration_ms = duration.as_millis(), "Retrieved process group members");

                        serde_json::to_vec(&json!({"members": members}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "get_members").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "get_members", "error" => "get_failed").increment(1);
                        error!(group_name = %group_name, error = %e, duration_ms = duration.as_millis(), "Failed to get process group members");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "get_local_members" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "get_local_members").increment(1);

                let group_name: String = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "get_local_members", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                match self
                    .process_group_registry
                    .get_local_members(&ctx, &group_name)
                    .await
                {
                    Ok(members) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "get_local_members").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "get_local_members").increment(1);
                        debug!(group_name = %group_name, member_count = members.len(), duration_ms = duration.as_millis(), "Retrieved local process group members");

                        serde_json::to_vec(&json!({"members": members}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "get_local_members").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "get_local_members", "error" => "get_failed").increment(1);
                        error!(group_name = %group_name, error = %e, duration_ms = duration.as_millis(), "Failed to get local process group members");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "list_groups" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "list_groups").increment(1);

                match self.process_group_registry.list_groups(&ctx).await {
                    Ok(groups) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "list_groups").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "list_groups").increment(1);
                        metrics::gauge!("plexspaces_facet_process_groups_total_count")
                            .set(groups.len() as f64);
                        debug!(
                            group_count = groups.len(),
                            duration_ms = duration.as_millis(),
                            "Listed process groups"
                        );

                        serde_json::to_vec(&json!({"groups": groups}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "list_groups").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "list_groups", "error" => "list_failed").increment(1);
                        error!(error = %e, duration_ms = duration.as_millis(), "Failed to list process groups");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "publish_to_group" => {
                metrics::counter!("plexspaces_facet_process_groups_operations_total", "operation" => "publish_to_group").increment(1);
                #[derive(Deserialize)]
                struct PublishArgs {
                    group_name: String,
                    topic: Option<String>,
                    message: String, // Base64 or JSON string
                }

                let args: PublishArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                // Convert message string to bytes (assume it's JSON for now)
                let message_bytes = args.message.as_bytes().to_vec();

                match self
                    .process_group_registry
                    .publish_to_group(&ctx, &args.group_name, args.topic.as_deref(), message_bytes)
                    .await
                {
                    Ok(recipients) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "publish_to_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_operations_success_total", "operation" => "publish_to_group").increment(1);
                        metrics::counter!("plexspaces_facet_process_groups_messages_published_total", "group" => args.group_name.clone()).increment(1);
                        metrics::gauge!("plexspaces_facet_process_groups_publish_recipient_count", "group" => args.group_name.clone()).set(recipients.len() as f64);
                        debug!(group_name = %args.group_name, recipient_count = recipients.len(), topic = ?args.topic, duration_ms = duration.as_millis(), "Message published to process group");

                        serde_json::to_vec(&json!({
                            "status": "ok",
                            "recipient_count": recipients.len(),
                            "recipients": recipients
                        }))
                        .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_process_groups_operation_duration_seconds", "operation" => "publish_to_group").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_process_groups_errors_total", "operation" => "publish_to_group", "error" => "publish_failed").increment(1);
                        error!(group_name = %args.group_name, error = %e, duration_ms = duration.as_millis(), "Failed to publish to process group");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            _ => {
                warn!(method = %method, "Unknown process group operation method");
                Ok(vec![])
            }
        };

        result
    }
}

#[async_trait]
impl Facet for ProcessGroupFacet {
    fn facet_type(&self) -> &str {
        "process_group"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_process_groups_attached_total").increment(1);
        debug!(actor_id = %actor_id, "Process group capability attached to actor");
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_process_groups_detached_total").increment(1);
        debug!(actor_id = %actor_id, "Process group capability detached from actor");
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        // Intercept process group operations
        if method == "create_group"
            || method == "join_group"
            || method == "leave_group"
            || method == "get_members"
            || method == "get_local_members"
            || method == "list_groups"
            || method == "publish_to_group"
        {
            let result = self.handle_process_group_operation(method, args).await?;
            return Ok(InterceptResult::ShortCircuit(result));
        }
        Ok(InterceptResult::Continue)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(serde_json::json!({}))
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // In-memory process group registry for testing
    struct TestProcessGroupRegistry {
        groups: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<String>>>>,
        memberships: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<String>>>>,
    }

    impl TestProcessGroupRegistry {
        fn new() -> Self {
            Self {
                groups: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
                memberships: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl ProcessGroupRegistry for TestProcessGroupRegistry {
        async fn create_group(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<(), String> {
            let mut groups = self.groups.write().await;
            if groups.contains_key(group_name) {
                return Err("Group already exists".to_string());
            }
            groups.insert(group_name.to_string(), vec![]);
            Ok(())
        }

        async fn join_group(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
            actor_id: &str,
            _topics: Vec<String>,
        ) -> Result<(), String> {
            let mut groups = self.groups.write().await;
            let members = groups
                .get_mut(group_name)
                .ok_or_else(|| "Group not found".to_string())?;
            if !members.contains(&actor_id.to_string()) {
                members.push(actor_id.to_string());
            }
            Ok(())
        }

        async fn leave_group(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
            actor_id: &str,
        ) -> Result<(), String> {
            let mut groups = self.groups.write().await;
            let members = groups
                .get_mut(group_name)
                .ok_or_else(|| "Group not found".to_string())?;
            members.retain(|id| id != actor_id);
            Ok(())
        }

        async fn get_members(
            &self,
            _ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, String> {
            let groups = self.groups.read().await;
            Ok(groups.get(group_name).cloned().unwrap_or_default())
        }

        async fn get_local_members(
            &self,
            ctx: &RequestContext,
            group_name: &str,
        ) -> Result<Vec<String>, String> {
            // For test, same as get_members
            self.get_members(ctx, group_name).await
        }

        async fn list_groups(&self, _ctx: &RequestContext) -> Result<Vec<String>, String> {
            let groups = self.groups.read().await;
            Ok(groups.keys().cloned().collect())
        }

        async fn publish_to_group(
            &self,
            ctx: &RequestContext,
            group_name: &str,
            _topic: Option<&str>,
            _message: Vec<u8>,
        ) -> Result<Vec<String>, String> {
            // Return members as recipients
            self.get_members(ctx, group_name).await
        }
    }

    #[tokio::test]
    async fn test_process_group_facet_create_join() {
        // ARRANGE
        let registry = Arc::new(TestProcessGroupRegistry::new());
        let mut facet = ProcessGroupFacet::new(registry, serde_json::json!({}), 50);

        // Attach to actor
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Create group
        let create_args = serde_json::json!("test-group-1");

        let result = facet
            .before_method(
                "create_group",
                serde_json::to_vec(&create_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should short-circuit with success
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Join group
        let join_args = serde_json::json!({
            "group_name": "test-group-1",
            "actor_id": "actor-1",
            "topics": ["topic-1"]
        });

        let result = facet
            .before_method(
                "join_group",
                serde_json::to_vec(&join_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should short-circuit with success
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Get members
        let get_members_args = serde_json::json!("test-group-1");

        let result = facet
            .before_method(
                "get_members",
                serde_json::to_vec(&get_members_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return members
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                let members = response["members"].as_array().unwrap();
                assert_eq!(members.len(), 1);
                assert_eq!(members[0], "actor-1");
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_process_group_facet_publish() {
        // ARRANGE
        let registry = Arc::new(TestProcessGroupRegistry::new());
        let mut facet = ProcessGroupFacet::new(registry, serde_json::json!({}), 50);

        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Create group and join
        facet
            .before_method(
                "create_group",
                serde_json::to_vec(&serde_json::json!("test-group-2"))
                    .unwrap()
                    .as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        facet
            .before_method(
                "join_group",
                serde_json::to_vec(&serde_json::json!({
                    "group_name": "test-group-2",
                    "actor_id": "actor-2",
                    "topics": []
                }))
                .unwrap()
                .as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ACT: Publish message
        let publish_args = serde_json::json!({
            "group_name": "test-group-2",
            "topic": null,
            "message": "Hello, group!"
        });

        let result = facet
            .before_method(
                "publish_to_group",
                serde_json::to_vec(&publish_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return recipients
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
                assert_eq!(response["recipient_count"], 1);
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_facet_type() {
        let registry = Arc::new(TestProcessGroupRegistry::new());
        let facet = ProcessGroupFacet::new(registry, serde_json::json!({}), 50);
        assert_eq!(facet.facet_type(), "process_group");
    }
}
