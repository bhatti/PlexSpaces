// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unit tests for pipeline actor logic
// Tests core functionality: actors processing messages, channels, mailboxes
// Not scalability tests - just logic validation

use plexspaces_actor::{ActorBuilder, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorError, BehaviorType, RequestContext, ServiceLocator};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Helper to set up a test node with ActorFactory registered
async fn setup_test_node(node_id: &str) -> (Arc<plexspaces_node::Node>, Arc<ServiceLocator>) {
    let node = Arc::new(NodeBuilder::new(node_id).build().await);
    let service_locator = node.service_locator();
    
    // Register ActorFactory (required for ActorBuilder::spawn)
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory_impl).await;
    
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
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Ingest { events } => {
                self.events_processed += events.len() as u64;

                // Reply with processed events
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::Processed {
                        events: events.clone(),
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("InputWorkerActor received unexpected message")
                ));
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
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::Process { events } => {
                // Simple processing: filter out empty events
                let processed: Vec<String> = events.into_iter()
                    .filter(|e| !e.is_empty())
                    .collect();

                self.events_processed += processed.len() as u64;

                // Reply with processed events
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::Processed {
                        events: processed,
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("ProcessorWorkerActor received unexpected message")
                ));
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
        Self {
            events_sent: 0,
        }
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
        let pipeline_msg: PipelineMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match pipeline_msg {
            PipelineMessage::SendToDestination { events } => {
                self.events_sent += events.len() as u64;

                // Reply with success
                if let Some(sender_id) = &msg.sender {
                    let reply = PipelineMessage::SendToDestinationResponse {
                        events_sent: events.len() as u64,
                    };
                    let reply_msg = Message::new(serde_json::to_vec(&reply)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize: {}", e)))?);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                return Err(BehaviorError::ProcessingError(
                    format!("OutputWorkerActor received unexpected message")
                ));
            }
        }

        Ok(())
    }
}

/// Test that input actor processes messages correctly
#[tokio::test]
async fn test_input_actor_processes_messages() {
    let (_node, service_locator) = setup_test_node("test-node").await;
    let ctx = RequestContext::internal();

    // Create input actor
    let input_actor = InputWorkerActor::new();
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_id("input@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send ingest message
    let ingest_msg = PipelineMessage::Ingest {
        events: vec!["event1".to_string(), "event2".to_string()],
    };
    let mut request = Message::new(serde_json::to_vec(&ingest_msg).unwrap())
        .with_message_type("call".to_string());
    request.receiver = input_ref.id().to_string();

    // Ask for response
    let response = input_ref.ask(request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response
    let result: PipelineMessage = serde_json::from_slice(response.payload()).unwrap();
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
    let ctx = RequestContext::internal();

    // Create processor actor
    let processor_actor = ProcessorWorkerActor::new();
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_id("processor@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send process message with empty events (should be filtered)
    let process_msg = PipelineMessage::Process {
        events: vec!["event1".to_string(), "".to_string(), "event2".to_string()],
    };
    let mut request = Message::new(serde_json::to_vec(&process_msg).unwrap())
        .with_message_type("call".to_string());
    request.receiver = processor_ref.id().to_string();

    // Ask for response
    let response = processor_ref.ask(request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response (empty events should be filtered)
    let result: PipelineMessage = serde_json::from_slice(response.payload()).unwrap();
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
    let ctx = RequestContext::internal();

    // Create output actor
    let output_actor = OutputWorkerActor::new();
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_id("output@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Send destination message
    let dest_msg = PipelineMessage::SendToDestination {
        events: vec!["event1".to_string(), "event2".to_string(), "event3".to_string()],
    };
    let mut request = Message::new(serde_json::to_vec(&dest_msg).unwrap())
        .with_message_type("call".to_string());
    request.receiver = output_ref.id().to_string();

    // Ask for response
    let response = output_ref.ask(request, Duration::from_secs(5))
        .await
        .unwrap();

    // Verify response
    let result: PipelineMessage = serde_json::from_slice(response.payload()).unwrap();
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
    let ctx = RequestContext::internal();

    // Create all actors
    let input_actor = InputWorkerActor::new();
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_id("input@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let processor_actor = ProcessorWorkerActor::new();
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_id("processor@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let output_actor = OutputWorkerActor::new();
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_id("output@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    // Stage 1: Input
    let ingest_msg = PipelineMessage::Ingest {
        events: vec!["event1".to_string(), "event2".to_string()],
    };
    let mut input_request = Message::new(serde_json::to_vec(&ingest_msg).unwrap())
        .with_message_type("call".to_string());
    input_request.receiver = input_ref.id().to_string();

    let input_response = input_ref.ask(input_request, Duration::from_secs(5))
        .await
        .unwrap();

    let input_result: PipelineMessage = serde_json::from_slice(input_response.payload()).unwrap();
    let processed_events = match input_result {
        PipelineMessage::Processed { events } => events,
        _ => panic!("Expected Processed message from input"),
    };

    // Stage 2: Processor
    let process_msg = PipelineMessage::Process {
        events: processed_events,
    };
    let mut processor_request = Message::new(serde_json::to_vec(&process_msg).unwrap())
        .with_message_type("call".to_string());
    processor_request.receiver = processor_ref.id().to_string();

    let processor_response = processor_ref.ask(processor_request, Duration::from_secs(5))
        .await
        .unwrap();

    let processor_result: PipelineMessage = serde_json::from_slice(processor_response.payload()).unwrap();
    let final_events = match processor_result {
        PipelineMessage::Processed { events } => events,
        _ => panic!("Expected Processed message from processor"),
    };

    // Stage 3: Output
    let output_msg = PipelineMessage::SendToDestination {
        events: final_events,
    };
    let mut output_request = Message::new(serde_json::to_vec(&output_msg).unwrap())
        .with_message_type("call".to_string());
    output_request.receiver = output_ref.id().to_string();

    let output_response = output_ref.ask(output_request, Duration::from_secs(5))
        .await
        .unwrap();

    let output_result: PipelineMessage = serde_json::from_slice(output_response.payload()).unwrap();
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
    let ctx = RequestContext::internal();

    // Create actors
    let input_actor = InputWorkerActor::new();
    let input_ref = ActorBuilder::new(Box::new(input_actor))
        .with_id("input@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let processor_actor = ProcessorWorkerActor::new();
    let processor_ref = ActorBuilder::new(Box::new(processor_actor))
        .with_id("processor@test-node".to_string())
        .with_namespace(ctx.namespace().to_string())
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();

    let output_actor = OutputWorkerActor::new();
    let output_ref = ActorBuilder::new(Box::new(output_actor))
        .with_id("output@test-node".to_string())
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
        let task = tokio::spawn(async move {
            // Stage 1: Input
            let ingest_msg = PipelineMessage::Ingest {
                events: vec![format!("event-{}-1", i), format!("event-{}-2", i)],
            };
            let mut input_request = Message::new(serde_json::to_vec(&ingest_msg).unwrap())
                .with_message_type("call".to_string());
            input_request.receiver = input_ref_clone.id().to_string();

            let input_response = input_ref_clone.ask(input_request, Duration::from_secs(5)).await
                .map_err(|e| format!("Input actor ask failed: {}", e))?;
            let input_result: PipelineMessage = serde_json::from_slice(input_response.payload())
                .map_err(|e| format!("Failed to deserialize input response: {}", e))?;
            let processed_events = match input_result {
                PipelineMessage::Processed { events } => events,
                _ => return Err("Expected Processed message".to_string()),
            };

            // Stage 2: Processor
            let process_msg = PipelineMessage::Process {
                events: processed_events,
            };
            let mut processor_request = Message::new(serde_json::to_vec(&process_msg).unwrap())
                .with_message_type("call".to_string());
            processor_request.receiver = processor_ref_clone.id().to_string();

            let processor_response = processor_ref_clone.ask(processor_request, Duration::from_secs(5)).await
                .map_err(|e| format!("Processor actor ask failed: {}", e))?;
            let processor_result: PipelineMessage = serde_json::from_slice(processor_response.payload())
                .map_err(|e| format!("Failed to deserialize processor response: {}", e))?;
            let final_events = match processor_result {
                PipelineMessage::Processed { events } => events,
                _ => return Err("Expected Processed message".to_string()),
            };

            // Stage 3: Output
            let output_msg = PipelineMessage::SendToDestination {
                events: final_events,
            };
            let mut output_request = Message::new(serde_json::to_vec(&output_msg).unwrap())
                .with_message_type("call".to_string());
            output_request.receiver = output_ref_clone.id().to_string();

            let output_response = output_ref_clone.ask(output_request, Duration::from_secs(5)).await
                .map_err(|e| format!("Output actor ask failed: {}", e))?;
            let output_result: PipelineMessage = serde_json::from_slice(output_response.payload())
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
        let result = timeout(Duration::from_secs(10), task).await.unwrap().unwrap();
        assert!(result.is_ok(), "Pipeline should process successfully: {:?}", result);
    }
}

