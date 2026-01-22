// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// ProcessGroupChannel - Channel implementation using ProcessGroupService
//
// This provides Kafka/NATS-like pub/sub semantics using Erlang pg/pg2-style
// process groups for distributed messaging without external dependencies.

#![cfg(feature = "process-group-backend")]

use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, trace, warn};

use crate::{Channel, ChannelError, ChannelResult};
use plexspaces_core::{ProcessGroupService, RequestContext, ServiceLocator};
use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, ChannelStats};
use plexspaces_proto::common::v1::Message;

const DEFAULT_BROADCAST_CAPACITY: usize = 1024;
const DEFAULT_RECEIVE_TIMEOUT_MS: u64 = 5000;

/// ProcessGroupChannel - Channel implementation using ProcessGroupService
///
/// ## Features
/// - Distributed pub/sub using Erlang pg/pg2-style process groups
/// - No external dependencies (Redis, Kafka, etc.)
/// - Multi-tenant support with namespace isolation
/// - Observability with metrics and tracing
///
/// ## Example
/// ```rust,ignore
/// use plexspaces_channel::ProcessGroupChannel;
///
/// let channel = ProcessGroupChannel::new(
///     service_locator,
///     "my-channel".to_string(),
///     "tenant-1".to_string(),
///     "default".to_string(),
///     config,
/// ).await?;
///
/// // Publish to all subscribers
/// channel.publish(message).await?;
/// ```
pub struct ProcessGroupChannel {
    service_locator: Arc<dyn ServiceLocator>,
    group_name: String,
    tenant_id: String,
    namespace: String,
    config: ChannelConfig,
    broadcast_tx: broadcast::Sender<Message>,
    pending_messages: RwLock<HashMap<String, Message>>,
    stats: RwLock<InternalStats>,
    closed: AtomicBool,
    total_sent: AtomicU64,
    total_received: AtomicU64,
    total_failed: AtomicU64,
    created_at: Instant,
    actor_id: String,
}

#[derive(Debug, Default)]
struct InternalStats {
    messages_sent: u64,
    messages_received: u64,
    messages_pending: u64,
    messages_failed: u64,
    total_latency_us: u64,
    latency_count: u64,
}

impl ProcessGroupChannel {
    /// Create a new ProcessGroupChannel
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for accessing ProcessGroupService
    /// * `group_name` - Name of the process group (channel name)
    /// * `tenant_id` - Tenant ID for multi-tenancy
    /// * `namespace` - Namespace for isolation
    /// * `config` - Channel configuration
    pub async fn new(
        service_locator: Arc<dyn ServiceLocator>,
        group_name: String,
        tenant_id: String,
        namespace: String,
        config: ChannelConfig,
    ) -> ChannelResult<Self> {
        let start = Instant::now();
        info!("Creating ProcessGroupChannel: group={}", group_name);

        let actor_id = format!("pg-channel-{}-{}", group_name, ulid::Ulid::new());
        let capacity = std::cmp::max(config.capacity as usize, DEFAULT_BROADCAST_CAPACITY);
        let (broadcast_tx, _) = broadcast::channel(capacity);

        let ctx = RequestContext::new_without_auth(tenant_id.clone(), namespace.clone());

        let pg_service = service_locator
            .get_process_group_service()
            .await
            .ok_or_else(|| {
                ChannelError::BackendError("ProcessGroupService not available".to_string())
            })?;

        // Create the process group (idempotent)
        match pg_service.create_group(&ctx, &group_name).await {
            Ok(()) => debug!("Created process group: {}", group_name),
            Err(e) => {
                let err_str = e.to_string();
                if !err_str.contains("already exists") {
                    warn!("Error creating process group {}: {}", group_name, e);
                }
            }
        }

        metrics::counter!("plexspaces_pg_channel_created_total").increment(1);
        info!("Created ProcessGroupChannel in {:?}", start.elapsed());

        Ok(ProcessGroupChannel {
            service_locator,
            group_name,
            tenant_id,
            namespace,
            config,
            broadcast_tx,
            pending_messages: RwLock::new(HashMap::new()),
            stats: RwLock::new(InternalStats::default()),
            closed: AtomicBool::new(false),
            total_sent: AtomicU64::new(0),
            total_received: AtomicU64::new(0),
            total_failed: AtomicU64::new(0),
            created_at: Instant::now(),
            actor_id,
        })
    }

    fn create_context(&self) -> RequestContext {
        RequestContext::new_without_auth(self.tenant_id.clone(), self.namespace.clone())
    }

    fn record_latency(&self, duration: Duration) {
        metrics::histogram!("plexspaces_pg_channel_latency_seconds")
            .record(duration.as_secs_f64());
    }
}

#[async_trait]
impl Channel for ProcessGroupChannel {
    async fn send(&self, message: Message) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.group_name.clone()));
        }

        let start = Instant::now();
        let message_id = if message.id.is_empty() {
            ulid::Ulid::new().to_string()
        } else {
            message.id.clone()
        };

        let ctx = self.create_context();
        let pg_service = self
            .service_locator
            .get_process_group_service()
            .await
            .ok_or_else(|| {
                ChannelError::BackendError("ProcessGroupService not available".to_string())
            })?;

        let topic = message.headers.get("topic").map(|s| s.as_str());

        pg_service
            .publish_to_group(&ctx, &self.group_name, topic, message.clone())
            .await
            .map_err(|e| {
                self.total_failed.fetch_add(1, Ordering::Relaxed);
                ChannelError::BackendError(format!("Failed to send: {}", e))
            })?;

        let _ = self.broadcast_tx.send(message.clone());

        {
            let mut pending = self.pending_messages.write().await;
            pending.insert(message_id.clone(), message);
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
            stats.messages_pending = pending.len() as u64;
        }

        self.total_sent.fetch_add(1, Ordering::Relaxed);
        self.record_latency(start.elapsed());
        metrics::counter!("plexspaces_pg_channel_send_total").increment(1);

        Ok(message_id)
    }

    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<Message>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.group_name.clone()));
        }

        let mut receiver = self.broadcast_tx.subscribe();
        let timeout = Duration::from_millis(DEFAULT_RECEIVE_TIMEOUT_MS);
        let mut messages = Vec::with_capacity(max_messages as usize);

        for _ in 0..max_messages {
            match tokio::time::timeout(timeout, receiver.recv()).await {
                Ok(Ok(msg)) => {
                    self.total_received.fetch_add(1, Ordering::Relaxed);
                    let mut stats = self.stats.write().await;
                    stats.messages_received += 1;
                    messages.push(msg);
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    warn!("Subscriber lagged, skipped {} messages", n);
                }
                Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => {
                    break;
                }
            }
        }

        metrics::counter!("plexspaces_pg_channel_receive_total").increment(messages.len() as u64);
        Ok(messages)
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<Message>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.group_name.clone()));
        }

        let mut receiver = self.broadcast_tx.subscribe();
        let mut messages = Vec::with_capacity(max_messages as usize);

        for _ in 0..max_messages {
            match receiver.try_recv() {
                Ok(msg) => {
                    self.total_received.fetch_add(1, Ordering::Relaxed);
                    messages.push(msg);
                }
                Err(_) => break,
            }
        }

        Ok(messages)
    }

    async fn subscribe(
        &self,
        _consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, Message>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.group_name.clone()));
        }

        let ctx = self.create_context();
        let pg_service = self
            .service_locator
            .get_process_group_service()
            .await
            .ok_or_else(|| {
                ChannelError::BackendError("ProcessGroupService not available".to_string())
            })?;

        pg_service
            .join_group(&ctx, &self.group_name, &self.actor_id, vec![])
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to join group: {}", e)))?;

        let mut receiver = self.broadcast_tx.subscribe();
        metrics::counter!("plexspaces_pg_channel_subscribe_total").increment(1);

        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(msg) => yield msg,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Subscriber lagged, skipped {} messages", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn publish(&self, message: Message) -> ChannelResult<u32> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.group_name.clone()));
        }

        let start = Instant::now();
        let ctx = self.create_context();

        let pg_service = self
            .service_locator
            .get_process_group_service()
            .await
            .ok_or_else(|| {
                ChannelError::BackendError("ProcessGroupService not available".to_string())
            })?;

        let topic = message.headers.get("topic").map(|s| s.as_str());

        let recipients = pg_service
            .publish_to_group(&ctx, &self.group_name, topic, message.clone())
            .await
            .map_err(|e| {
                self.total_failed.fetch_add(1, Ordering::Relaxed);
                ChannelError::BackendError(format!("Failed to publish: {}", e))
            })?;

        let _ = self.broadcast_tx.send(message);

        self.total_sent.fetch_add(1, Ordering::Relaxed);
        self.record_latency(start.elapsed());
        metrics::counter!("plexspaces_pg_channel_publish_total").increment(1);

        Ok(recipients)
    }

    async fn ack(&self, message_id: &str) -> ChannelResult<()> {
        let mut pending = self.pending_messages.write().await;
        if pending.remove(message_id).is_some() {
            let mut stats = self.stats.write().await;
            stats.messages_pending = pending.len() as u64;
            metrics::counter!("plexspaces_pg_channel_ack_total").increment(1);
            trace!("Acknowledged message: {}", message_id);
            Ok(())
        } else {
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
        let mut pending = self.pending_messages.write().await;
        if let Some(msg) = pending.get(message_id).cloned() {
            if requeue {
                drop(pending);
                let _ = self.publish(msg).await?;
            } else {
                pending.remove(message_id);
            }
            metrics::counter!("plexspaces_pg_channel_nack_total").increment(1);
            Ok(())
        } else {
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        let stats = self.stats.read().await;
        let avg_latency = if stats.latency_count > 0 {
            stats.total_latency_us / stats.latency_count
        } else {
            0
        };

        Ok(ChannelStats {
            name: self.group_name.clone(),
            backend: ChannelBackend::ChannelBackendProcessGroup as i32,
            messages_sent: stats.messages_sent,
            messages_received: stats.messages_received,
            messages_pending: stats.messages_pending,
            messages_failed: self.total_failed.load(Ordering::Relaxed),
            avg_latency_us: avg_latency,
            ..Default::default()
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        info!("Closing ProcessGroupChannel: {}", self.group_name);
        self.closed.store(true, Ordering::Relaxed);

        let ctx = self.create_context();
        if let Some(pg_service) = self.service_locator.get_process_group_service().await {
            let _ = pg_service
                .leave_group(&ctx, &self.group_name, &self.actor_id)
                .await;
        }

        metrics::counter!("plexspaces_pg_channel_closed_total").increment(1);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::actor_context::{ActorService, ChannelService, ObjectRegistry, ProcessGroupService, TupleSpaceProvider};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    struct TestServiceLocator {
        process_group_service: Arc<RwLock<Option<Arc<dyn ProcessGroupService>>>>,
    }

    impl TestServiceLocator {
        fn new() -> Self {
            TestServiceLocator {
                process_group_service: Arc::new(RwLock::new(None)),
            }
        }

        async fn with_process_group_service(self, svc: Arc<dyn ProcessGroupService>) -> Self {
            *self.process_group_service.write().await = Some(svc);
            self
        }
    }

    #[async_trait]
    impl ServiceLocator for TestServiceLocator {
        // Service registration by type (generic)
        async fn register_service<T: plexspaces_core::Service + 'static>(&self, _service: Arc<T>) where Self: Sized {}
        async fn get_service<T: plexspaces_core::Service + 'static>(&self) -> Option<Arc<T>> where Self: Sized { None }
        async fn register_service_by_name<T: plexspaces_core::Service + 'static>(&self, _name: &str, _service: Arc<T>) where Self: Sized {}
        async fn get_service_by_name<T: plexspaces_core::Service + 'static>(&self, _name: &str) -> Option<Arc<T>> where Self: Sized { None }
        
        // Registries
        async fn actor_registry(&self) -> Option<Arc<plexspaces_core::ActorRegistry>> { None }
        async fn register_actor_registry(&self, _registry: Arc<plexspaces_core::ActorRegistry>) {}
        async fn virtual_actor_manager(&self) -> Option<Arc<plexspaces_core::VirtualActorManager>> { None }
        async fn reply_waiter_registry(&self) -> Option<Arc<plexspaces_core::ReplyWaiterRegistry>> { None }
        async fn get_actor_factory(&self) -> Option<Arc<dyn std::any::Any + Send + Sync>> { None }
        
        // Core services
        async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> { None }
        async fn register_actor_service(&self, _: Arc<dyn ActorService>) {}
        async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> { None }
        async fn register_channel_service(&self, _: Arc<dyn ChannelService>) {}
        async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> { None }
        async fn register_tuplespace_provider(&self, _: Arc<dyn TupleSpaceProvider>) {}
        async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> { None }
        async fn register_object_registry(&self, _: Arc<dyn ObjectRegistry>) {}
        
        // Storage
        async fn get_journal_storage(&self) -> Option<Arc<dyn plexspaces_core::JournalStorage + Send + Sync>> { None }
        async fn register_journal_storage(&self, _: Arc<dyn plexspaces_core::JournalStorage + Send + Sync>) {}
        
        // Monitoring
        async fn get_node_metrics_accessor(&self) -> Option<Arc<dyn plexspaces_core::monitoring::NodeMetricsAccessor + Send + Sync>> { None }
        async fn register_node_metrics_accessor(&self, _: Arc<dyn plexspaces_core::monitoring::NodeMetricsAccessor + Send + Sync>) {}
        
        // Facets
        async fn get_facet_manager(&self) -> Option<Arc<plexspaces_core::facet_service_wrapper::FacetManagerServiceWrapper>> { None }
        async fn register_facet_manager(&self, _: Arc<plexspaces_core::facet_service_wrapper::FacetManagerServiceWrapper>) {}
        async fn get_facet_registry(&self) -> Option<Arc<plexspaces_core::facet_service_wrapper::FacetRegistryServiceWrapper>> { None }
        async fn register_facet_registry(&self, _: Arc<plexspaces_core::facet_service_wrapper::FacetRegistryServiceWrapper>) {}
        
        // Node config
        async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> { None }
        async fn register_node_config(&self, _: plexspaces_proto::node::v1::NodeConfig) {}
        async fn get_node_connection_info(&self) -> Option<Arc<dyn plexspaces_core::monitoring::NodeConnectionInfo + Send + Sync>> { None }
        async fn register_node_connection_info(&self, _: Arc<dyn plexspaces_core::monitoring::NodeConnectionInfo + Send + Sync>) {}
        
        // Shutdown
        fn is_shutdown_requested(&self) -> bool { false }
        fn request_shutdown(&self) {}
        
        // Application manager
        async fn application_manager(&self) -> Option<Arc<dyn plexspaces_core::ApplicationManager>> { None }
        async fn register_application_manager(&self, _: Arc<dyn plexspaces_core::ApplicationManager>) {}
        
        // Behavior registry
        async fn get_behavior_registry(&self) -> Option<Arc<plexspaces_core::behavior_factory::BehaviorRegistry>> { None }
        async fn register_behavior_registry(&self, _: Arc<plexspaces_core::behavior_factory::BehaviorRegistry>) {}
        
        // System context
        async fn request_context_for_system_operations(&self) -> plexspaces_core::RequestContext {
            plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string())
        }
        
        // gRPC connection manager
        async fn get_grpc_connection_manager(&self) -> Option<Arc<plexspaces_core::GrpcConnectionManager>> { None }
        async fn register_grpc_connection_manager(&self, _: Arc<plexspaces_core::GrpcConnectionManager>) {}
        async fn get_actor_service_client(&self, _node_id: &str) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        
        // WASM runtime
        async fn get_wasm_runtime(&self) -> Option<Arc<dyn plexspaces_core::WasmRuntimeTrait>> { None }
        async fn register_wasm_runtime(&self, _: Arc<dyn plexspaces_core::WasmRuntimeTrait>) {}
        
        // Process group service
        async fn get_process_group_service(&self) -> Option<Arc<dyn ProcessGroupService>> {
            self.process_group_service.read().await.clone()
        }
        async fn register_process_group_service(&self, service: Arc<dyn ProcessGroupService>) {
            *self.process_group_service.write().await = Some(service);
        }
    }

    struct MockProcessGroupService {
        groups: RwLock<HashMap<String, Vec<String>>>,
    }

    impl MockProcessGroupService {
        fn new() -> Self {
            MockProcessGroupService {
                groups: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl ProcessGroupService for MockProcessGroupService {
        async fn create_group(&self, _ctx: &RequestContext, group_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut groups = self.groups.write().await;
            groups.entry(group_name.to_string()).or_insert_with(Vec::new);
            Ok(())
        }
        async fn delete_group(&self, _ctx: &RequestContext, group_name: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut groups = self.groups.write().await;
            groups.remove(group_name);
            Ok(())
        }
        async fn join_group(&self, _ctx: &RequestContext, group_name: &str, actor_id: &str, _topics: Vec<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut groups = self.groups.write().await;
            let members = groups.entry(group_name.to_string()).or_insert_with(Vec::new);
            if !members.contains(&actor_id.to_string()) {
                members.push(actor_id.to_string());
            }
            Ok(())
        }
        async fn leave_group(&self, _ctx: &RequestContext, group_name: &str, actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let mut groups = self.groups.write().await;
            if let Some(members) = groups.get_mut(group_name) {
                members.retain(|m| m != actor_id);
            }
            Ok(())
        }
        async fn get_members(&self, _ctx: &RequestContext, group_name: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let groups = self.groups.read().await;
            Ok(groups.get(group_name).cloned().unwrap_or_default())
        }
        async fn get_local_members(&self, ctx: &RequestContext, group_name: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            self.get_members(ctx, group_name).await
        }
        async fn list_groups(&self, _ctx: &RequestContext) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            let groups = self.groups.read().await;
            Ok(groups.keys().cloned().collect())
        }
        async fn publish_to_group(&self, _ctx: &RequestContext, group_name: &str, _topic: Option<&str>, _message: Message) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
            let groups = self.groups.read().await;
            Ok(groups.get(group_name).map(|m| m.len()).unwrap_or(0) as u32)
        }
    }

    fn create_test_config(name: &str) -> ChannelConfig {
        ChannelConfig {
            name: name.to_string(),
            backend: ChannelBackend::ChannelBackendProcessGroup as i32,
            capacity: 100,
            ..Default::default()
        }
    }

    fn create_test_message(id: &str, payload: &str) -> Message {
        Message {
            id: id.to_string(),
            sender_id: "test-sender".to_string(),
            payload: payload.as_bytes().to_vec(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_channel_creation() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await;

        assert!(channel.is_ok());
        let channel = channel.unwrap();
        assert!(!channel.is_closed());
    }

    #[tokio::test]
    async fn test_send_message() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await
        .unwrap();

        let message = create_test_message("msg-1", "Hello");
        let result = channel.send(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_message() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await
        .unwrap();

        let message = create_test_message("msg-1", "Hello");
        let result = channel.publish(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ack_nack() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await
        .unwrap();

        let message = create_test_message("msg-ack", "Test");
        channel.send(message).await.unwrap();

        assert!(channel.ack("msg-ack").await.is_ok());
        assert!(channel.ack("non-existent").await.is_err());
    }

    #[tokio::test]
    async fn test_channel_stats() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await
        .unwrap();

        for i in 0..3 {
            let message = create_test_message(&format!("msg-{}", i), "Test");
            channel.send(message).await.unwrap();
        }

        let stats = channel.get_stats().await.unwrap();
        assert_eq!(stats.messages_sent, 3);
    }

    #[tokio::test]
    async fn test_channel_close() {
        let mock_pg = Arc::new(MockProcessGroupService::new());
        let service_locator = Arc::new(
            TestServiceLocator::new()
                .with_process_group_service(mock_pg.clone())
                .await,
        );

        let channel = ProcessGroupChannel::new(
            service_locator,
            "test-group".to_string(),
            "tenant-1".to_string(),
            "default".to_string(),
            create_test_config("test-channel"),
        )
        .await
        .unwrap();

        assert!(!channel.is_closed());
        channel.close().await.unwrap();
        assert!(channel.is_closed());

        let message = create_test_message("msg-after-close", "Fail");
        assert!(channel.send(message).await.is_err());
    }
}

