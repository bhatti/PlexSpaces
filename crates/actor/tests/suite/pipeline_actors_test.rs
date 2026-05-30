// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unit tests for pipeline actor logic
// Tests core functionality: actors processing messages, channels, mailboxes
// Not scalability tests - just logic validation

use plexspaces_actor::ActorBuilder;
use plexspaces_actor::{
    Actor, ActorContext, ActorId, BehaviorError, BehaviorType, Message, RequestContext,
    RequestContextExt, ServiceLocator,
};
use plexspaces_actor::behavior::GenServer;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

fn actor_id_from_message_receiver(receiver_id: &str) -> ActorId {
    ActorId::from_canonical(receiver_id).expect("request receiver_id should be canonical")
}

/// Helper to create a proto Message
fn create_message(payload: Vec<u8>) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

/// Helper to create a proto Message with message type
fn create_request_message(payload: Vec<u8>, receiver: &str) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        receiver_id: receiver.to_string(),
        payload,
        ..Default::default()
    }
}

fn test_actor_id(name: &str, node_id: &str, namespace: &str) -> ActorId {
    ActorId::new(
        name.to_string(),
        "gen_server".to_string(),
        namespace.to_string(),
        node_id.to_string(),
    )
    .expect("test actor id should be valid")
}

/// Helper to set up a test node
async fn setup_test_node(node_id: &str) -> (Arc<plexspaces_node::Node>, Arc<dyn ServiceLocator>) {
    let node = Arc::new(
        NodeBuilder::new(node_id)
            .with_in_memory_backends()
            .build()
            .await,
    );
    let service_locator = node.service_locator();
    (node, service_locator)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum PipelineMessage {
    Ingest { events: Vec<String> },
    Process { events: Vec<String> },
    Processed { events: Vec<String> },
    SendToDestination { events: Vec<String> },
    SendToDestinationResponse { events_sent: u64 },
}

/// Input Worker Actor - receives events and passes them through
struct InputWorkerActor {
    events_processed: u64,
}

impl InputWorkerActor {
    fn new() -> Self {
        Self {
            events_processed: 0,
        }
    }
}

#[async_trait::async_trait]
impl Actor for InputWorkerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for InputWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Ingest { events } => {
                self.events_processed += events.len() as u64;

                // Reply with processed events
                if !msg.sender_id.is_empty() {
                    let sender_id = &msg.sender_id;
                    let reply = PipelineMessage::Processed {
                        events: events.clone(),
                    };
                    let reply_msg = create_message(serde_json::to_vec(&reply).map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to serialize: {}", e))
                    })?);
                    ctx.send_reply(
                        if msg.correlation_id.is_empty() {
                            None
                        } else {
                            Some(msg.correlation_id.as_str())
                        },
                        sender_id,
                        actor_id_from_message_receiver(&msg.receiver_id),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(format!(
                    "InputWorkerActor received unexpected message"
                )));
            }
        }

        Ok(())
    }
}

/// Processor Worker Actor - processes events
struct ProcessorWorkerActor {
    events_processed: u64,
}

impl ProcessorWorkerActor {
    fn new() -> Self {
        Self {
            events_processed: 0,
        }
    }
}

#[async_trait::async_trait]
impl Actor for ProcessorWorkerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for ProcessorWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Process { events } => {
                // Simple processing: filter out empty events
                let processed: Vec<String> = events.into_iter().filter(|e| !e.is_empty()).collect();

                self.events_processed += processed.len() as u64;

                // Reply with processed events
                if !msg.sender_id.is_empty() {
                    let sender_id = &msg.sender_id;
                    let reply = PipelineMessage::Processed { events: processed };
                    let reply_msg = create_message(serde_json::to_vec(&reply).map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to serialize: {}", e))
                    })?);
                    ctx.send_reply(
                        if msg.correlation_id.is_empty() {
                            None
                        } else {
                            Some(msg.correlation_id.as_str())
                        },
                        sender_id,
                        actor_id_from_message_receiver(&msg.receiver_id),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(format!(
                    "ProcessorWorkerActor received unexpected message"
                )));
            }
        }

        Ok(())
    }
}

/// Output Worker Actor - sends events to destination
struct OutputWorkerActor {
    events_sent: u64,
}

impl OutputWorkerActor {
    fn new() -> Self {
        Self { events_sent: 0 }
    }
}

#[async_trait::async_trait]
impl Actor for OutputWorkerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for OutputWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let pipeline_msg: PipelineMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::SendToDestination { events } => {
                self.events_sent += events.len() as u64;

                // Reply with success
                if !msg.sender_id.is_empty() {
                    let sender_id = &msg.sender_id;
                    let reply = PipelineMessage::SendToDestinationResponse {
                        events_sent: events.len() as u64,
                    };
                    let reply_msg = create_message(serde_json::to_vec(&reply).map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to serialize: {}", e))
                    })?);
                    ctx.send_reply(
                        if msg.correlation_id.is_empty() {
                            None
                        } else {
                            Some(msg.correlation_id.as_str())
                        },
                        sender_id,
                        actor_id_from_message_receiver(&msg.receiver_id),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(format!(
                    "OutputWorkerActor received unexpected message"
                )));
            }
        }

        Ok(())
    }
}

/// Test that input actor processes messages correctly
#[tokio::test]
async fn test_input_actor_processes_messages() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());

    // Create input actor
    let input_actor = InputWorkerActor::new();
    let input_actor_id = test_actor_id("input", "test-node", ctx.namespace());
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_name(input_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send ingest message
    let ingest_msg = PipelineMessage::Ingest {
        events: vec!["event1".to_string(), "event2".to_string()],
    };
    let request = create_request_message(
        serde_json::to_vec(&ingest_msg).unwrap(),
        input_ref.id().as_str(),
    );

    // Ask for response
    let response = input_ref
        .ask(&ctx, request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response
    let result: PipelineMessage = serde_json::from_slice(&response.payload).unwrap();
    match result {
        PipelineMessage::Processed { events } => {
            assert_eq!(events.len(), 2);
            assert_eq!(events[0], "event1");
            assert_eq!(events[1], "event2");
        }
        _ => panic!("Expected Processed message"),
    }
}

/// Test that processor actor filters events correctly
#[tokio::test]
async fn test_processor_actor_filters_events() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());

    // Create processor actor
    let processor_actor = ProcessorWorkerActor::new();
    let processor_actor_id = test_actor_id("processor", "test-node", ctx.namespace());
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_name(processor_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send process message with empty events (should be filtered)
    let process_msg = PipelineMessage::Process {
        events: vec!["event1".to_string(), "".to_string(), "event2".to_string()],
    };
    let request = create_request_message(
        serde_json::to_vec(&process_msg).unwrap(),
        processor_ref.id().as_str(),
    );

    // Ask for response
    let response = processor_ref
        .ask(&ctx, request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response (empty events should be filtered)
    let result: PipelineMessage = serde_json::from_slice(&response.payload).unwrap();
    match result {
        PipelineMessage::Processed { events } => {
            assert_eq!(events.len(), 2);
            assert_eq!(events[0], "event1");
            assert_eq!(events[1], "event2");
        }
        _ => panic!("Expected Processed message"),
    }
}

/// Test that output actor sends events correctly
#[tokio::test]
async fn test_output_actor_sends_events() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());

    // Create output actor
    let output_actor = OutputWorkerActor::new();
    let output_actor_id = test_actor_id("output", "test-node", ctx.namespace());
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_name(output_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send destination message
    let dest_msg = PipelineMessage::SendToDestination {
        events: vec![
            "event1".to_string(),
            "event2".to_string(),
            "event3".to_string(),
        ],
    };
    let request = create_request_message(
        serde_json::to_vec(&dest_msg).unwrap(),
        output_ref.id().as_str(),
    );

    // Ask for response
    let response = output_ref
        .ask(&ctx, request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response
    let result: PipelineMessage = serde_json::from_slice(&response.payload).unwrap();
    match result {
        PipelineMessage::SendToDestinationResponse { events_sent } => {
            assert_eq!(events_sent, 3);
        }
        _ => panic!("Expected SendToDestinationResponse message"),
    }
}

/// Test full pipeline: input -> processor -> output
#[tokio::test]
async fn test_full_pipeline() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());

    // Create all actors
    let input_actor = InputWorkerActor::new();
    let input_actor_id = test_actor_id("input", "test-node", ctx.namespace());
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_name(input_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let processor_actor = ProcessorWorkerActor::new();
    let processor_actor_id = test_actor_id("processor", "test-node", ctx.namespace());
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_name(processor_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let output_actor = OutputWorkerActor::new();
    let output_actor_id = test_actor_id("output", "test-node", ctx.namespace());
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_name(output_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Stage 1: Input
    let ingest_msg = PipelineMessage::Ingest {
        events: vec!["event1".to_string(), "event2".to_string()],
    };
    let input_request = create_request_message(
        serde_json::to_vec(&ingest_msg).unwrap(),
        input_ref.id().as_str(),
    );

    let input_response = input_ref
        .ask(&ctx, input_request, Duration::from_secs(5))
        .await
        .unwrap();

    let input_result: PipelineMessage = serde_json::from_slice(&input_response.payload).unwrap();
    let processed_events = match input_result {
        PipelineMessage::Processed { events } => events,
        _ => panic!("Expected Processed message from input"),
    };

    // Stage 2: Processor
    let process_msg = PipelineMessage::Process {
        events: processed_events,
    };
    let processor_request = create_request_message(
        serde_json::to_vec(&process_msg).unwrap(),
        processor_ref.id().as_str(),
    );

    let processor_response = processor_ref
        .ask(&ctx, processor_request, Duration::from_secs(5))
        .await
        .unwrap();

    let processor_result: PipelineMessage =
        serde_json::from_slice(&processor_response.payload).unwrap();
    let final_events = match processor_result {
        PipelineMessage::Processed { events } => events,
        _ => panic!("Expected Processed message from processor"),
    };

    // Stage 3: Output
    let output_msg = PipelineMessage::SendToDestination {
        events: final_events,
    };
    let output_request = create_request_message(
        serde_json::to_vec(&output_msg).unwrap(),
        output_ref.id().as_str(),
    );

    let output_response = output_ref
        .ask(&ctx, output_request, Duration::from_secs(5))
        .await
        .unwrap();

    let output_result: PipelineMessage = serde_json::from_slice(&output_response.payload).unwrap();
    match output_result {
        PipelineMessage::SendToDestinationResponse { events_sent } => {
            assert_eq!(events_sent, 2);
        }
        _ => panic!("Expected SendToDestinationResponse message"),
    }
}

/// Test concurrent pipeline processing
#[tokio::test]
async fn test_concurrent_pipeline_processing() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());

    // Create actors
    let input_actor = InputWorkerActor::new();
    let input_actor_id = test_actor_id("input", "test-node", ctx.namespace());
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_name(input_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let processor_actor = ProcessorWorkerActor::new();
    let processor_actor_id = test_actor_id("processor", "test-node", ctx.namespace());
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_name(processor_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let output_actor = OutputWorkerActor::new();
    let output_actor_id = test_actor_id("output", "test-node", ctx.namespace());
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_name(output_actor_id.name().to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Spawn multiple concurrent requests
    let mut tasks = Vec::new();
    for i in 0..5 {
        let input_ref_clone = input_ref.clone();
        let processor_ref_clone = processor_ref.clone();
        let output_ref_clone = output_ref.clone();
        let pipe_ctx = ctx.clone();
        let task = tokio::spawn(async move {
            // Stage 1: Input
            let ingest_msg = PipelineMessage::Ingest {
                events: vec![format!("event-{}-1", i), format!("event-{}-2", i)],
            };
            let input_request = Message {
                id: ulid::Ulid::new().to_string(),
                message_type: "call".to_string(),
                receiver_id: input_ref_clone.id().to_string(),
                payload: serde_json::to_vec(&ingest_msg).unwrap(),
                ..Default::default()
            };

            let input_response = input_ref_clone
                .ask(&pipe_ctx, input_request, Duration::from_secs(5))
                .await
                .map_err(|e| format!("Input actor ask failed: {}", e))?;
            let input_result: PipelineMessage = serde_json::from_slice(&input_response.payload)
                .map_err(|e| format!("Failed to deserialize input response: {}", e))?;
            let processed_events = match input_result {
                PipelineMessage::Processed { events } => events,
                _ => return Err("Expected Processed message".to_string()),
            };

            // Stage 2: Processor
            let process_msg = PipelineMessage::Process {
                events: processed_events,
            };
            let processor_request = Message {
                id: ulid::Ulid::new().to_string(),
                message_type: "call".to_string(),
                receiver_id: processor_ref_clone.id().to_string(),
                payload: serde_json::to_vec(&process_msg).unwrap(),
                ..Default::default()
            };

            let processor_response = processor_ref_clone
                .ask(&pipe_ctx, processor_request, Duration::from_secs(5))
                .await
                .map_err(|e| format!("Processor actor ask failed: {}", e))?;
            let processor_result: PipelineMessage =
                serde_json::from_slice(&processor_response.payload)
                    .map_err(|e| format!("Failed to deserialize processor response: {}", e))?;
            let final_events = match processor_result {
                PipelineMessage::Processed { events } => events,
                _ => return Err("Expected Processed message".to_string()),
            };

            // Stage 3: Output
            let output_msg = PipelineMessage::SendToDestination {
                events: final_events,
            };
            let output_request = Message {
                id: ulid::Ulid::new().to_string(),
                message_type: "call".to_string(),
                receiver_id: output_ref_clone.id().to_string(),
                payload: serde_json::to_vec(&output_msg).unwrap(),
                ..Default::default()
            };

            let output_response = output_ref_clone
                .ask(&pipe_ctx, output_request, Duration::from_secs(5))
                .await
                .map_err(|e| format!("Output actor ask failed: {}", e))?;
            let output_result: PipelineMessage =
                serde_json::from_slice(&output_response.payload)
                    .map_err(|e| format!("Failed to deserialize output response: {}", e))?;
            match output_result {
                PipelineMessage::SendToDestinationResponse { events_sent } => {
                    if events_sent != 2 {
                        return Err(format!("Expected 2 events, got {}", events_sent));
                    }
                }
                _ => return Err("Expected SendToDestinationResponse".to_string()),
            }

            Ok::<(), String>(())
        });
        tasks.push(task);
    }

    // Wait for all tasks
    for task in tasks {
        let result = timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.is_ok(),
            "Pipeline should process successfully: {:?}",
            result
        );
    }
}
