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

//! Test stub implementation of ServiceLocator for actor crate
//!
//! ## Purpose
//! Provides a minimal stub implementation of ServiceLocator for testing purposes.
//! This allows actor crate to create ActorContext without depending on the full services crate.
//!
//! ## Design
//! - Minimal implementation that returns None for all service lookups
//! - Used only for creating ActorContext in tests and Actor::new()
//! - Node will replace this with full ServiceLocatorImpl when spawning actors

use std::sync::Arc;
use async_trait::async_trait;
use plexspaces_core::{
    ServiceLocator,
    ActorRegistry, VirtualActorManager, ReplyWaiterRegistry, Service,
    ActorService, ChannelService, TupleSpaceProvider, ObjectRegistry,
    NodeMetricsAccessor, JournalStorage, BehaviorRegistry,
    GrpcConnectionManager, RequestContext, ProcessGroupService,
};
use plexspaces_core::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};

/// Minimal stub implementation of ServiceLocator for testing
///
/// ## Purpose
/// Provides a stub that returns None for all service lookups.
/// Used only for creating ActorContext in Actor::new() and tests.
/// Node will replace this with full ServiceLocatorImpl when spawning actors.
#[derive(Clone)]
pub struct TestServiceLocatorStub;

impl TestServiceLocatorStub {
    /// Create a new test stub
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ServiceLocator for TestServiceLocatorStub {
    async fn register_service<T: Service + 'static>(&self, _service: Arc<T>)
    where Self: Sized {
        // No-op for stub
    }

    async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
    where Self: Sized {
        None
    }

    async fn register_service_by_name<T: Service + 'static>(&self, _name: &str, _service: Arc<T>)
    where Self: Sized {
        // No-op for stub
    }

    async fn get_service_by_name<T: Service + 'static>(&self, _name: &str) -> Option<Arc<T>>
    where Self: Sized {
        None
    }

    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>> {
        None
    }

    async fn register_actor_registry(&self, _registry: Arc<ActorRegistry>) {
        // No-op for stub
    }

    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        None
    }

    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
        None
    }

    // Note: ActorFactory methods are not part of ServiceLocator trait (to avoid circular dependency)
    // Test stubs don't need to implement them

    async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> {
        None
    }

    async fn register_actor_service(&self, _service: Arc<dyn ActorService>) {
        // No-op for stub
    }

    async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        None
    }

    async fn register_channel_service(&self, _service: Arc<dyn ChannelService>) {
        // No-op for stub
    }

    async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        None
    }

    async fn register_tuplespace_provider(&self, _provider: Arc<dyn TupleSpaceProvider>) {
        // No-op for stub
    }

    async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
        None
    }

    async fn register_object_registry(&self, _registry: Arc<dyn ObjectRegistry>) {
        // No-op for stub
    }

    async fn get_journal_storage(&self) -> Option<Arc<dyn JournalStorage + Send + Sync>> {
        None
    }

    async fn register_journal_storage(&self, _storage: Arc<dyn JournalStorage + Send + Sync>) {
        // No-op for stub
    }

    async fn get_lock_manager(&self) -> Option<Arc<dyn plexspaces_core::LockManager + Send + Sync>> {
        None
    }

    async fn register_lock_manager(&self, _service: Arc<dyn plexspaces_core::LockManager + Send + Sync>) {
        // No-op for stub
    }

    async fn get_node_metrics_accessor(&self) -> Option<Arc<dyn NodeMetricsAccessor + Send + Sync>> {
        None
    }

    async fn register_node_metrics_accessor(&self, _accessor: Arc<dyn NodeMetricsAccessor + Send + Sync>) {
        // No-op for stub
    }

    async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>> {
        None
    }

    async fn register_facet_manager(&self, _service: Arc<FacetManagerServiceWrapper>) {
        // No-op for stub
    }

    async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>> {
        None
    }

    async fn register_facet_registry(&self, _service: Arc<FacetRegistryServiceWrapper>) {
        // No-op for stub
    }

    async fn get_actor_factory(&self) -> Option<Arc<dyn plexspaces_core::ActorFactory>> {
        None
    }

    async fn register_actor_factory(&self, _factory: Arc<dyn plexspaces_core::ActorFactory>) {
        // No-op for stub
    }

    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        None
    }

    async fn register_node_config(&self, _config: plexspaces_proto::node::v1::NodeConfig) {
        // No-op for stub
    }

    async fn get_node_connection_info(&self) -> Option<Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>> {
        None
    }

    async fn register_node_connection_info(&self, _accessor: Arc<dyn plexspaces_core::NodeConnectionInfo + Send + Sync>) {
        // No-op for stub
    }

    async fn initialize_services(
        &self,
        _node_id: Option<String>,
        _node_config: Option<plexspaces_proto::node::v1::NodeConfig>,
        _release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    ) {
        // No-op for stub
    }

    fn is_shutdown_requested(&self) -> bool {
        false
    }

    fn request_shutdown(&self) {
        // No-op for stub
    }

    async fn application_manager(&self) -> Option<Arc<dyn plexspaces_core::ApplicationManager>> {
        None
    }

    async fn register_application_manager(&self, _manager: Arc<dyn plexspaces_core::ApplicationManager>) {
        // No-op for stub
    }

    async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>> {
        None
    }

    async fn register_behavior_registry(&self, _registry: Arc<BehaviorRegistry>) {
        // No-op for stub
    }

    async fn request_context_for_system_operations(&self) -> RequestContext {
        RequestContext::new_without_auth("default".to_string(), "default".to_string())
    }
    
    async fn request_context_for_system_operations_with_namespace(&self, namespace: String) -> RequestContext {
        RequestContext::new_without_auth("default".to_string(), namespace)
    }

    async fn get_grpc_connection_manager(&self) -> Option<Arc<GrpcConnectionManager>> {
        None
    }

    async fn register_grpc_connection_manager(&self, _manager: Arc<GrpcConnectionManager>) {
        // No-op for stub
    }

    async fn get_actor_service_client(
        &self,
        _node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
        Err("TestServiceLocatorStub: get_actor_service_client not implemented".into())
    }

    async fn get_wasm_runtime(&self) -> Option<Arc<dyn plexspaces_core::WasmRuntimeTrait>> {
        None
    }

    async fn register_wasm_runtime(&self, _runtime: Arc<dyn plexspaces_core::WasmRuntimeTrait>) {
        // No-op for stub
    }

    async fn get_process_group_service(&self) -> Option<Arc<dyn ProcessGroupService>> {
        None
    }

    async fn register_process_group_service(&self, _service: Arc<dyn ProcessGroupService>) {
        // No-op for stub
    }

    async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig> {
        None
    }

    async fn register_security_config(&self, _config: plexspaces_proto::node::v1::SecurityConfig) {
        // No-op for stub
    }

    async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig> {
        None
    }

    async fn register_runtime_config(&self, _config: plexspaces_proto::node::v1::RuntimeConfig) {
        // No-op for stub
    }

    async fn is_auth_disabled(&self) -> bool {
        false // Auth enabled by default for security
    }

    async fn get_blob_service(&self) -> Option<Arc<dyn plexspaces_core::BlobServiceTrait>> {
        None
    }

    async fn register_blob_service(&self, _service: Arc<dyn plexspaces_core::BlobServiceTrait>) {
        // No-op for stub
    }

    async fn get_node_registry(&self) -> Option<Arc<dyn plexspaces_core::NodeRegistryTrait>> {
        None
    }

    async fn register_node_registry(&self, _registry: Arc<dyn plexspaces_core::NodeRegistryTrait>) {
        // No-op for stub
    }
}

#[async_trait]

impl Default for TestServiceLocatorStub {
    fn default() -> Self {
        Self::new()
    }
}
