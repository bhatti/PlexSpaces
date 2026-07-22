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

use crate::core::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
use crate::core::{
    ActorRegistry, BehaviorRegistry, ChannelService, GrpcConnectionManager,
    MetricsPrometheusRenderer, ObjectRegistry, ReplyWaiterRegistry, ServiceLocator,
    TupleSpaceProvider, VirtualActorManager,
};
use async_trait::async_trait;
use plexspaces_common::RequestContextExt;
use std::sync::Arc;

/// Minimal stub implementation of ServiceLocator for testing
///
/// ## Purpose
/// Provides a stub that returns None for most service lookups, but includes
/// a real ReplyWaiterRegistry so reply-waiter tests work without the full services crate.
/// Used only for creating ActorContext in Actor::new() and tests.
/// Node will replace this with full ServiceLocatorImpl when spawning actors.
#[derive(Clone)]
pub struct TestServiceLocatorStub {
    reply_waiter_registry: Arc<ReplyWaiterRegistry>,
}

impl TestServiceLocatorStub {
    /// Create a new test stub
    pub fn new() -> Self {
        Self {
            reply_waiter_registry: Arc::new(ReplyWaiterRegistry::new()),
        }
    }
}

#[async_trait]
impl ServiceLocator for TestServiceLocatorStub {
    async fn actor_registry(&self) -> Option<Arc<ActorRegistry>> {
        None
    }

    async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
        None
    }

    async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
        Some(self.reply_waiter_registry.clone())
    }

    async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        None
    }

    async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        None
    }

    async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
        None
    }

    async fn get_metrics_prometheus_renderer(
        &self,
    ) -> Option<Arc<dyn MetricsPrometheusRenderer + Send + Sync>> {
        None
    }

    async fn get_metrics_service_access(
        &self,
    ) -> Option<Arc<dyn crate::core::MetricsServiceAccess + Send + Sync>> {
        None
    }

    async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>> {
        None
    }

    async fn facet_container_for_actor(
        &self,
        _actor_id: &str,
    ) -> Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>> {
        None
    }

    async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>> {
        None
    }

    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        None
    }

    async fn get_node_connection_info(
        &self,
    ) -> Option<Arc<dyn crate::core::NodeConnectionInfo + Send + Sync>> {
        None
    }

    async fn initialize_services(
        &self,
        _release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
    ) {
        // No-op for stub
    }

    fn request_shutdown(&self) {
        // No-op for stub
    }

    async fn application_manager(&self) -> Option<Arc<dyn crate::core::ApplicationManager>> {
        None
    }

    async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>> {
        None
    }

    async fn get_grpc_connection_manager(&self) -> Option<Arc<GrpcConnectionManager>> {
        None
    }

    async fn get_actor_service_client(
        &self,
        _node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
        Err("TestServiceLocatorStub: get_actor_service_client not implemented".into())
    }

    async fn get_application_service_client(
        &self,
        _node_id: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
        Err("TestServiceLocatorStub: get_application_service_client not implemented".into())
    }

    async fn get_wasm_runtime(&self) -> Option<Arc<dyn crate::core::WasmRuntimeTrait>> {
        None
    }

    async fn get_security_config(&self) -> Option<plexspaces_proto::node::v1::SecurityConfig> {
        None
    }

    async fn get_runtime_config(&self) -> Option<plexspaces_proto::node::v1::RuntimeConfig> {
        None
    }

    async fn is_auth_disabled(&self) -> bool {
        false // Auth enabled by default for security
    }

    async fn get_blob_service(&self) -> Option<Arc<dyn crate::core::BlobServiceTrait>> {
        None
    }

    async fn get_node_registry(&self) -> Option<Arc<dyn crate::core::NodeRegistryTrait>> {
        None
    }

    async fn get_actor_transport_client(
        &self,
    ) -> Option<Arc<dyn plexspaces_service_traits::ActorTransportClient>> {
        None
    }

    async fn get_node_transport_client(
        &self,
    ) -> Option<Arc<dyn plexspaces_service_traits::NodeTransportClient>> {
        None
    }

    async fn get_ws_registry(&self) -> Option<Arc<dyn crate::WsRegistryTrait>> {
        None
    }
}

#[async_trait]
impl plexspaces_service_traits::ServiceLocatorBase for TestServiceLocatorStub {
    async fn get_actor_service(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::ActorService>> {
        None
    }

    async fn get_journal_storage(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::JournalStorage + Send + Sync>> {
        None
    }

    async fn get_keyvalue_store(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_common::KeyValueStore>> {
        None
    }

    async fn get_lock_manager(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_locks::LockManager + Send + Sync>> {
        None
    }

    async fn get_actor_factory(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_service_traits::ActorFactory>> {
        None
    }

    fn is_shutdown_requested(&self) -> bool {
        false
    }

    async fn request_context_for_system_operations(&self) -> plexspaces_common::RequestContext {
        plexspaces_common::RequestContext::new_without_auth(
            "default".to_string(),
            "default".to_string(),
        )
    }

    async fn request_context_for_system_operations_with_namespace(
        &self,
        namespace: String,
    ) -> plexspaces_common::RequestContext {
        plexspaces_common::RequestContext::new_without_auth("default".to_string(), namespace)
    }
}

impl Default for TestServiceLocatorStub {
    fn default() -> Self {
        Self::new()
    }
}

// TestServiceLocatorStub is accessible outside the crate for integration tests
// The Default impl ensures it can be used as Arc::new(TestServiceLocatorStub::new())
