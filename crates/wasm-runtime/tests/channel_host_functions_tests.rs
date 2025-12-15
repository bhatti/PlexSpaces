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

//! Integration tests for channel host functions
//!
//! ## Purpose
//! Tests that WASM actors can call channel host functions:
//! - send_to_queue
//! - publish_to_topic
//! - receive_from_queue (placeholder for now)

use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_core::ChannelService;
use plexspaces_mailbox::Message;
use plexspaces_wasm_runtime::{WasmInstance, WasmRuntime};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use wasmtime::StoreLimitsBuilder;

/// Mock ChannelService for testing
struct MockChannelService {
    queue_messages: Arc<RwLock<Vec<(String, Message)>>>,
    topic_messages: Arc<RwLock<Vec<(String, Message)>>>,
}

impl MockChannelService {
    fn new() -> Self {
        Self {
            queue_messages: Arc::new(RwLock::new(Vec::new())),
            topic_messages: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ChannelService for MockChannelService {
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = self.queue_messages.write().await;
        messages.push((queue_name.to_string(), message));
        Ok("msg-001".to_string())
    }

    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = self.topic_messages.write().await;
        messages.push((topic_name.to_string(), message));
        Ok("msg-002".to_string())
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
        queue_name: &str,
        timeout: Option<Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        let mut messages = self.queue_messages.write().await;
        if let Some(pos) = messages.iter().position(|(q, _)| q == queue_name) {
            let (_, message) = messages.remove(pos);
            Ok(Some(message))
        } else {
            // Simulate timeout
            if let Some(timeout) = timeout {
                tokio::time::sleep(timeout).await;
            }
            Ok(None)
        }
    }
}

/// Create a WASM module that calls send_to_queue host function
fn create_send_to_queue_module() -> Vec<u8> {
    wat::parse_str(
        r#"
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
                ;; Write queue name "test-queue" at offset 0
                (i32.store8 (i32.const 0) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 1) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 2) (i32.const 115)) ;; 's'
                (i32.store8 (i32.const 3) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 4) (i32.const 45)) ;; '-'
                (i32.store8 (i32.const 5) (i32.const 113)) ;; 'q'
                (i32.store8 (i32.const 6) (i32.const 117)) ;; 'u'
                (i32.store8 (i32.const 7) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 8) (i32.const 117)) ;; 'u'
                (i32.store8 (i32.const 9) (i32.const 101)) ;; 'e'
                
                ;; Write message type "test" at offset 10
                (i32.store8 (i32.const 10) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 11) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 12) (i32.const 115)) ;; 's'
                (i32.store8 (i32.const 13) (i32.const 116)) ;; 't'
                
                ;; Write payload "hello" at offset 14
                (i32.store8 (i32.const 14) (i32.const 104)) ;; 'h'
                (i32.store8 (i32.const 15) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 16) (i32.const 108)) ;; 'l'
                (i32.store8 (i32.const 17) (i32.const 108)) ;; 'l'
                (i32.store8 (i32.const 18) (i32.const 111)) ;; 'o'
                
                ;; Call send_to_queue(queue_name_ptr=0, queue_name_len=10, msg_type_ptr=10, msg_type_len=4, payload_ptr=14, payload_len=5)
                (call $send_to_queue
                    (i32.const 0)  ;; queue_name_ptr
                    (i32.const 10) ;; queue_name_len
                    (i32.const 10) ;; msg_type_ptr
                    (i32.const 4)  ;; msg_type_len
                    (i32.const 14) ;; payload_ptr
                    (i32.const 5)  ;; payload_len
                )
                drop
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#,
    )
    .expect("Failed to parse WAT")
}

/// Create a WASM module that calls publish_to_topic host function
fn create_publish_to_topic_module() -> Vec<u8> {
    wat::parse_str(
        r#"
        (module
            (import "plexspaces" "publish_to_topic" 
                (func $publish_to_topic 
                    (param i32 i32 i32 i32 i32 i32) 
                    (result i32)
                )
            )
            (memory (export "memory") 1)
            (func (export "init") (param i32 i32) (result i32)
                i32.const 0
            )
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                ;; Write topic name "test-topic" at offset 0
                (i32.store8 (i32.const 0) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 1) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 2) (i32.const 115)) ;; 's'
                (i32.store8 (i32.const 3) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 4) (i32.const 45)) ;; '-'
                (i32.store8 (i32.const 5) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 6) (i32.const 111)) ;; 'o'
                (i32.store8 (i32.const 7) (i32.const 112)) ;; 'p'
                (i32.store8 (i32.const 8) (i32.const 105)) ;; 'i'
                (i32.store8 (i32.const 9) (i32.const 99))  ;; 'c'
                
                ;; Write message type "event" at offset 10
                (i32.store8 (i32.const 10) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 11) (i32.const 118)) ;; 'v'
                (i32.store8 (i32.const 12) (i32.const 101)) ;; 'e'
                (i32.store8 (i32.const 13) (i32.const 110)) ;; 'n'
                (i32.store8 (i32.const 14) (i32.const 116)) ;; 't'
                
                ;; Write payload "data" at offset 15
                (i32.store8 (i32.const 15) (i32.const 100)) ;; 'd'
                (i32.store8 (i32.const 16) (i32.const 97))  ;; 'a'
                (i32.store8 (i32.const 17) (i32.const 116)) ;; 't'
                (i32.store8 (i32.const 18) (i32.const 97))  ;; 'a'
                
                ;; Call publish_to_topic(topic_name_ptr=0, topic_name_len=10, msg_type_ptr=10, msg_type_len=5, payload_ptr=15, payload_len=4)
                (call $publish_to_topic
                    (i32.const 0)  ;; topic_name_ptr
                    (i32.const 10) ;; topic_name_len
                    (i32.const 10) ;; msg_type_ptr
                    (i32.const 5)  ;; msg_type_len
                    (i32.const 15) ;; payload_ptr
                    (i32.const 4)  ;; payload_len
                )
                drop
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#,
    )
    .expect("Failed to parse WAT")
}

#[tokio::test]
async fn test_send_to_queue_host_function() {
    let channel_service = Arc::new(MockChannelService::new());
    let queue_messages = channel_service.queue_messages.clone();

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = create_send_to_queue_module();
    let module = runtime
        .load_module("queue-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "queue-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        Some(channel_service),
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message which will call send_to_queue
    instance
        .handle_message("sender", "test", b"trigger".to_vec())
        .await
        .expect("Failed to handle message");

    // Wait a bit for async task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify message was sent to queue
    let messages = queue_messages.read().await;
    assert_eq!(messages.len(), 1);
    let (queue_name, message) = &messages[0];
    assert_eq!(queue_name, "test-queue");
    assert_eq!(message.payload(), b"hello");
    assert_eq!(message.message_type_str(), "test");
}

#[tokio::test]
async fn test_publish_to_topic_host_function() {
    let channel_service = Arc::new(MockChannelService::new());
    let topic_messages = channel_service.topic_messages.clone();

    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = create_publish_to_topic_module();
    let module = runtime
        .load_module("topic-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "topic-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        Some(channel_service),
    )
    .await
    .expect("Failed to create instance");

    // Call handle_message which will call publish_to_topic
    instance
        .handle_message("sender", "test", b"trigger".to_vec())
        .await
        .expect("Failed to handle message");

    // Wait a bit for async task to complete
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify message was published to topic
    let messages = topic_messages.read().await;
    assert_eq!(messages.len(), 1);
    let (topic_name, message) = &messages[0];
    assert_eq!(topic_name, "test-topic");
    assert_eq!(message.payload(), b"data");
    assert_eq!(message.message_type_str(), "event");
}

#[tokio::test]
async fn test_send_to_queue_without_channel_service() {
    let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
    let wasm_bytes = create_send_to_queue_module();
    let module = runtime
        .load_module("queue-actor", "1.0.0", &wasm_bytes)
        .await
        .expect("Failed to load module");

    let limits = StoreLimitsBuilder::new()
        .memory_size(16 * 1024 * 1024)
        .build();

    let instance = WasmInstance::new(
        runtime.engine(),
        module,
        "queue-actor".to_string(),
        &[],
        plexspaces_wasm_runtime::capabilities::profiles::default(),
        limits,
        None, // No ChannelService
    )
    .await
    .expect("Failed to create instance");

    // Should still work (host function returns error code but doesn't crash)
    let result = instance
        .handle_message("sender", "test", b"trigger".to_vec())
        .await;

    // Should succeed (host function handles missing service gracefully)
    assert!(result.is_ok());
}

