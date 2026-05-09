// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WasmRuntimeTrait — allows ServiceLocator to return WASM runtime without
//! depending on the `wasm-runtime` crate.

use async_trait::async_trait;
use plexspaces_common::KeyValueStore;
use std::sync::Arc;

use crate::blob_service::BlobServiceTrait;
use crate::channel_service::ChannelService;
use crate::journal_storage::JournalStorage;
use crate::object_registry::ObjectRegistry;
use crate::outbound_http::OutboundHttpClient;
use crate::tuplespace_provider::TupleSpaceProvider;

/// Allows ServiceLocator to return a WASM runtime without depending on `plexspaces-wasm-runtime`.
#[async_trait]
pub trait WasmRuntimeTrait: Send + Sync {
    async fn module_count(&self) -> usize;
    async fn clear_cache(&self);
    async fn load_module(
        &self,
        name: &str,
        version: &str,
        bytes: &[u8],
    ) -> Result<Arc<dyn std::any::Any + Send + Sync>, Box<dyn std::error::Error + Send + Sync>>;
    async fn get_module(
        &self,
        hash: &str,
    ) -> Option<Arc<dyn std::any::Any + Send + Sync>>;
    async fn resolve_module(
        &self,
        module_ref: &str,
    ) -> Option<Arc<dyn std::any::Any + Send + Sync>>;
    async fn contains_module(&self, hash: &str) -> bool;
    async fn list_modules(&self) -> Vec<(String, String, String)>;
    async fn evict_module(&self, hash: &str) -> bool;
    #[allow(clippy::too_many_arguments)]
    async fn instantiate(
        &self,
        module: Arc<dyn std::any::Any + Send + Sync>,
        actor_id: String,
        initial_state: &[u8],
        config: Arc<dyn std::any::Any + Send + Sync>,
        channel_service: Option<Arc<dyn ChannelService>>,
        message_sender: Option<Arc<dyn std::any::Any + Send + Sync>>,
        tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
        keyvalue_store: Option<Arc<dyn KeyValueStore>>,
        process_group_registry: Option<Arc<dyn std::any::Any + Send + Sync>>,
        lock_manager: Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>>,
        object_registry: Option<Arc<dyn ObjectRegistry>>,
        journal_storage: Option<Arc<dyn JournalStorage>>,
        blob_service: Option<Arc<dyn BlobServiceTrait>>,
        outbound_http_client: Option<Arc<dyn OutboundHttpClient>>,
    ) -> Result<Arc<dyn std::any::Any + Send + Sync>, Box<dyn std::error::Error + Send + Sync>>;
    fn as_any(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync>;
}
