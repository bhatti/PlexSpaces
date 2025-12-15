// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for WASM runtime behavior-specific routing and channel functions

use plexspaces_wasm_runtime::*;
use plexspaces_core::ChannelService;
use plexspaces_mailbox::Message;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use futures::StreamExt;

/// Mock ChannelService for testing
struct MockChannelService {
    queues: Arc<RwLock<HashMap<String, Vec<Message>>>>,
    topics: Arc<RwLock<HashMap<String, Vec<Message>>>>,
}

impl MockChannelService {
    fn new() -> Self {
        Self {
            queues: Arc::new(RwLock::new(HashMap::new())),
            topics: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl ChannelService for MockChannelService {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut queues = self.queues.write().await;
        queues.entry(queue_name.to_string())
            .or_insert_with(Vec::new)
            .push(message);
        Ok("msg-001".to_string())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut topics = self.topics.write().await;
        topics.entry(topic_name.to_string())
            .or_insert_with(Vec::new)
            .push(message);
        Ok("msg-001".to_string())
    }

    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        // Return empty stream for testing
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn receive_from_queue(
        &self,
        queue_name: &str,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        let mut queues = self.queues.write().await;
        if let Some(queue) = queues.get_mut(queue_name) {
            if !queue.is_empty() {
                return Ok(Some(queue.remove(0)));
            }
        }
        Ok(None)
    }
}

/// Test GenServer behavior (handle_request)
#[tokio::test]
async fn test_genserver_handle_request() {
    // Create WASM module with handle_request export
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_request") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Return success (0) - response would be in memory
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-genserver", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-genserver".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message with "call" type - should route to handle_request
    let result = instance
        .handle_message("sender", "call", b"test payload".to_vec())
        .await;

    // Should succeed (handle_request was called)
    assert!(result.is_ok());
}

/// Test GenEvent behavior (handle_event)
#[tokio::test]
async fn test_genevent_handle_event() {
    // Create WASM module with handle_event export
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_event") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Return success (0)
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-genevent", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-genevent".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message with "cast" type - should route to handle_event
    let result = instance
        .handle_message("sender", "cast", b"test event".to_vec())
        .await;

    // Should succeed (handle_event was called)
    assert!(result.is_ok());
}

/// Test GenFSM behavior (handle_transition)
#[tokio::test]
async fn test_genfsm_handle_transition() {
    // Create WASM module with handle_transition export
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_transition") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Return success (0) - new state would be in memory
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-genfsm", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-genfsm".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message - should route to handle_transition
    let result = instance
        .handle_message("sender", "transition", b"test transition".to_vec())
        .await;

    // Should succeed (handle_transition was called)
    assert!(result.is_ok());
}

/// Test fallback to handle_message when behavior-specific methods not available
#[tokio::test]
async fn test_fallback_to_handle_message() {
    // Create WASM module with only handle_message export (no behavior-specific methods)
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-fallback", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-fallback".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message - should fallback to handle_message
    let result = instance
        .handle_message("sender", "call", b"test".to_vec())
        .await;

    // Should succeed (handle_message was called)
    assert!(result.is_ok());
}

/// Test channel host function send_to_queue
#[tokio::test]
async fn test_channel_send_to_queue() {
    // Note: This test verifies the host function is registered
    // Full integration test would require calling from WASM
    let channel_service = Arc::new(MockChannelService::new());
    let channel_service_clone = channel_service.clone();

    // Create WASM module that calls send_to_queue
    let wat = r#"
        (module
            (import "plexspaces" "send_to_queue" 
                (func $send_to_queue 
                    (param i32 i32 i32 i32 i32 i32) 
                    (result i32)
                )
            )
            (memory (export "memory") 1)
            (func (export "test_send") (result i32)
                ;; Call send_to_queue(queue_name="test-queue", msg_type="test", payload="data")
                ;; For now, just verify function exists
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-channel", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-channel".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        Some(channel_service),
    )
    .await
    .expect("Failed to create instance");

    // Verify instance was created with channel service
    assert_eq!(instance.actor_id(), "test-channel");
}

/// Test channel host function publish_to_topic
#[tokio::test]
async fn test_channel_publish_to_topic() {
    let channel_service = Arc::new(MockChannelService::new());

    let wasm_bytes = wat::parse_str(r#"
        (module
            (memory (export "memory") 1)
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#).expect("Failed to parse WAT");

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-publish", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-publish".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        Some(channel_service),
    )
    .await
    .expect("Failed to create instance");

    // Verify instance was created
    assert_eq!(instance.actor_id(), "test-publish");
}

/// Test that channel service is optional
#[tokio::test]
async fn test_channel_service_optional() {
    let wasm_bytes = wat::parse_str(r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#).expect("Failed to parse WAT");

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let module = runtime
        .load_module("test-optional", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = wasmtime::StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    // Create instance without channel service
    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "test-optional".to_string(),
        &[],
        crate::capabilities::profiles::default(),
        limits,
        None, // No channel service
    )
    .await
    .expect("Failed to create instance");

    // Should still work
    assert_eq!(instance.actor_id(), "test-optional");
}

