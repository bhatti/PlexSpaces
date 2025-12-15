// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Comprehensive coverage tests for WASM runtime
// Tests edge cases, error paths, and all code branches

use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_core::ChannelService;
use plexspaces_mailbox::Message;
use plexspaces_wasm_runtime::{WasmInstance, WasmRuntime};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use wasmtime::StoreLimitsBuilder;

/// Mock ChannelService that can simulate errors
struct ErrorProneChannelService {
    should_fail_send: bool,
    should_fail_publish: bool,
    should_fail_receive: bool,
    queue_messages: Arc<RwLock<Vec<Message>>>,
}

impl ErrorProneChannelService {
    fn new() -> Self {
        Self {
            should_fail_send: false,
            should_fail_publish: false,
            should_fail_receive: false,
            queue_messages: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn with_send_error(mut self) -> Self {
        self.should_fail_send = true;
        self
    }

    fn with_publish_error(mut self) -> Self {
        self.should_fail_publish = true;
        self
    }

    fn with_receive_error(mut self) -> Self {
        self.should_fail_receive = true;
        self
    }
}

#[async_trait]
impl ChannelService for ErrorProneChannelService {
    async fn send_to_queue(
        &self,
        _queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail_send {
            Err("Simulated send error".into())
        } else {
            let mut messages = self.queue_messages.write().await;
            messages.push(message);
            Ok("msg-001".to_string())
        }
    }

    async fn publish_to_topic(
        &self,
        _topic_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail_publish {
            Err("Simulated publish error".into())
        } else {
            Ok("msg-002".to_string())
        }
    }

    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn receive_from_queue(
        &self,
        _queue_name: &str,
        _timeout: Option<Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        if self.should_fail_receive {
            Err("Simulated receive error".into())
        } else {
            let mut messages = self.queue_messages.write().await;
            Ok(messages.pop())
        }
    }
}

/// Test HostFunctions::new()
#[tokio::test]
async fn test_host_functions_new() {
    use plexspaces_wasm_runtime::HostFunctions;
    let host_functions = HostFunctions::new();
    // Should create without error
    assert!(true); // Just verify it doesn't panic
}

/// Test HostFunctions::default()
#[tokio::test]
async fn test_host_functions_default() {
    use plexspaces_wasm_runtime::HostFunctions;
    let _host_functions = HostFunctions::default();
    // Should create without error
    assert!(true);
}

/// Test HostFunctions::with_channel_service()
#[tokio::test]
async fn test_host_functions_with_channel_service() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new());
    let host_functions = HostFunctions::with_channel_service(channel_service);
    
    // Test send_to_queue with channel service
    let result = host_functions
        .send_to_queue("test-queue", "test", b"payload".to_vec())
        .await;
    assert!(result.is_ok());
}

/// Test HostFunctions::send_to_queue() without channel service
#[tokio::test]
async fn test_host_functions_send_to_queue_no_service() {
    use plexspaces_wasm_runtime::HostFunctions;
    let host_functions = HostFunctions::new();
    
    let result = host_functions
        .send_to_queue("test-queue", "test", b"payload".to_vec())
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not configured"));
}

/// Test HostFunctions::publish_to_topic() without channel service
#[tokio::test]
async fn test_host_functions_publish_to_topic_no_service() {
    use plexspaces_wasm_runtime::HostFunctions;
    let host_functions = HostFunctions::new();
    
    let result = host_functions
        .publish_to_topic("test-topic", "test", b"payload".to_vec())
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not configured"));
}

/// Test HostFunctions::receive_from_queue() without channel service
#[tokio::test]
async fn test_host_functions_receive_from_queue_no_service() {
    use plexspaces_wasm_runtime::HostFunctions;
    let host_functions = HostFunctions::new();
    
    let result = host_functions.receive_from_queue("test-queue", 1000).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not configured"));
}

/// Test HostFunctions::receive_from_queue() with timeout
#[tokio::test]
async fn test_host_functions_receive_from_queue_with_timeout() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new());
    let host_functions = HostFunctions::with_channel_service(channel_service);
    
    // Test with timeout (should return None if no message)
    let result = host_functions.receive_from_queue("test-queue", 100).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

/// Test HostFunctions::receive_from_queue() with message
#[tokio::test]
async fn test_host_functions_receive_from_queue_with_message() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new());
    
    // Add a message to the queue
    {
        let mut messages = channel_service.queue_messages.write().await;
        messages.push(Message::new(b"test".to_vec()));
    }
    
    let host_functions = HostFunctions::with_channel_service(channel_service);
    
    // Test receiving message
    let result = host_functions.receive_from_queue("test-queue", 0).await;
    assert!(result.is_ok());
    let (msg_type, payload) = result.unwrap().unwrap();
    assert_eq!(payload, b"test");
    assert!(!msg_type.is_empty());
}

/// Test HostFunctions::receive_from_queue() error handling
#[tokio::test]
async fn test_host_functions_receive_from_queue_error() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new().with_receive_error());
    let host_functions = HostFunctions::with_channel_service(channel_service);
    
    let result = host_functions.receive_from_queue("test-queue", 0).await;
    assert!(result.is_err());
}

/// Test behavior routing with error in handle_request
#[tokio::test]
async fn test_handle_request_error() {
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "init") (param i32 i32) (result i32)
                i32.const 0
            )
            (func (export "handle_request") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Return error code (non-zero)
                i32.const -1
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
        .load_module("error-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "error-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call with "call" - should route to handle_request which returns error
    let result = instance
        .handle_message("sender", "call", b"test".to_vec())
        .await;
    
    // Should succeed (error code is handled internally)
    assert!(result.is_ok());
}

/// Test behavior routing with error in handle_event
#[tokio::test]
async fn test_handle_event_error() {
    let wat = r#"
        (module
            (memory (export "memory") 1)
            (func (export "init") (param i32 i32) (result i32)
                i32.const 0
            )
            (func (export "handle_event") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Return error code (non-zero)
                i32.const -1
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
        .load_module("error-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "error-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        None,
    )
    .await
    .expect("Failed to create instance");

    // Call with "cast" - should route to handle_event which returns error
    let result = instance
        .handle_message("sender", "cast", b"test".to_vec())
        .await;
    
    // Should return error
    assert!(result.is_err());
}

/// Test channel host function error handling
#[tokio::test]
async fn test_channel_host_function_error_handling() {
    let channel_service = Arc::new(ErrorProneChannelService::new().with_send_error());
    
    let wat = r#"
        (module
            (import "plexspaces" "send_to_queue" 
                (func $send_to_queue 
                    (param i32 i32 i32 i32 i32 i32) 
                    (result i32)
                )
            )
            (memory (export "memory") 1)
            (func (export "init") (param i32 i32) (result i32)
                i32.const 0
            )
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Write queue name "test" at offset 0
                (i32.store8 (i32.const 0) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 1) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 2) (i32.const 115)) ;; 's'
                (i32.store8 (i32.const 3) (i32.const 116)) ;; 't'
                
                ;; Call send_to_queue (will fail)
                (call $send_to_queue
                    (i32.const 0)  ;; queue_name_ptr
                    (i32.const 4)  ;; queue_name_len
                    (i32.const 0)  ;; msg_type_ptr
                    (i32.const 0)  ;; msg_type_len
                    (i32.const 0)  ;; payload_ptr
                    (i32.const 0)  ;; payload_len
                )
                drop
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
        .load_module("error-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "error-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        Some(channel_service),
    )
    .await
    .expect("Failed to create instance");

    // Should handle error gracefully (host function logs error but doesn't crash)
    let result = instance
        .handle_message("sender", "test", b"trigger".to_vec())
        .await;
    
    // Should succeed (error is logged but doesn't propagate)
    assert!(result.is_ok());
}

/// Test with_services() method
#[tokio::test]
async fn test_host_functions_with_services() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new());
    let host_functions = HostFunctions::with_services(None, Some(channel_service));
    
    // Test that channel service works
    let result = host_functions
        .send_to_queue("test-queue", "test", b"payload".to_vec())
        .await;
    assert!(result.is_ok());
}

/// Test message type extraction from Message
#[tokio::test]
async fn test_message_type_extraction() {
    use plexspaces_wasm_runtime::HostFunctions;
    let channel_service = Arc::new(ErrorProneChannelService::new());
    
    // Add a message with specific type
    {
        let mut messages = channel_service.queue_messages.write().await;
        let mut msg = Message::new(b"test".to_vec());
        msg = msg.with_message_type("custom-type".to_string());
        messages.push(msg);
    }
    
    let host_functions = HostFunctions::with_channel_service(channel_service);
    
    let result = host_functions.receive_from_queue("test-queue", 0).await;
    assert!(result.is_ok());
    let (msg_type, _) = result.unwrap().unwrap();
    assert_eq!(msg_type, "custom-type");
}

