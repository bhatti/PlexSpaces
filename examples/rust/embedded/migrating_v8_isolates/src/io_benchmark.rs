// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// I/O Performance Benchmark
// Realistic high-throughput log processing benchmark simulating Splunk/Datadog workloads
// Reads from /dev/urandom, transforms to JSON logs, processes through pipeline for 60 seconds

use crate::messages::*;
use crate::metrics::{MetricsCollector, PerformanceMetrics};
use crate::pipeline_workers::{create_pipelines, Pipeline};
use plexspaces_mailbox::Message;
use plexspaces_node::Node;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;
use tracing::{info, error, warn, debug};

/// Transform binary data from /dev/urandom into realistic JSON log entries
/// This simulates real-world log ingestion where raw data is transformed into structured logs
fn transform_random_data_to_log_entries(
    random_data: &[u8],
    chunk_id: u64,
) -> Result<Vec<PipelineEvent>, Box<dyn std::error::Error>> {
    let mut events = Vec::new();
    
    // Split random data into log entry-sized chunks (256-1024 bytes each)
    let entry_size = 512;
    let num_entries = (random_data.len() + entry_size - 1) / entry_size;
    
    for i in 0..num_entries {
        let start = i * entry_size;
        let end = std::cmp::min(start + entry_size, random_data.len());
        let chunk = &random_data[start..end];
        
        // Create realistic log entry structure (Splunk/Datadog style)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Determine log level based on data entropy
        let entropy: u8 = chunk.iter().take(16).fold(0u8, |acc, &b| acc.wrapping_add(b)) % 4;
        let level = match entropy {
            0 => "DEBUG",
            1 => "INFO",
            2 => "WARN",
            _ => "ERROR",
        };
        
        // Create realistic fields
        let source_app = format!("app-{}", chunk_id % 100);
        let host = format!("host-{}", (chunk_id / 10) % 50);
        let request_id = format!("req-{:x}", chunk.iter().take(8).fold(0u64, |acc, &b| (acc << 8) | b as u64));
        let user_id = format!("user-{}", chunk.iter().take(4).fold(0u32, |acc, &b| (acc << 8) | b as u32) % 10000);
        
        // Create message with realistic content
        let message = if chunk.len() >= 32 {
            format!(
                "Processing request {} for user {} on {} - {} bytes processed",
                &request_id[..8],
                user_id,
                host,
                chunk.len()
            )
        } else {
            format!("Event {} from {}", i, source_app)
        };
        
        // Base64 encode the random chunk to simulate real log payload
        // Using hex encoding as fallback (simpler and works on all base64 versions)
        let payload_bytes = &chunk[..std::cmp::min(64, chunk.len())];
        let payload = hex::encode(payload_bytes);
        
        let log_entry = LogEntry {
            id: ulid::Ulid::new().to_string(),
            timestamp,
            level: level.to_string(),
            message,
            fields: serde_json::json!({
                "source": source_app,
                "host": host,
                "request_id": request_id,
                "user_id": user_id,
                "chunk_id": chunk_id,
                "entry_index": i,
                "payload_size": chunk.len(),
                "payload": payload,
                "environment": if chunk_id % 3 == 0 { "production" } else { "staging" },
                "service": format!("service-{}", chunk_id % 20),
                "region": match chunk_id % 5 {
                    0 => "us-east-1",
                    1 => "us-west-2",
                    2 => "eu-west-1",
                    3 => "ap-southeast-1",
                    _ => "ap-northeast-1",
                },
            }),
            source: source_app.clone(),
        };
        
        events.push(PipelineEvent::Log { data: log_entry });
    }
    
    Ok(events)
}

/// Read chunk from /dev/urandom and transform to log entries
fn read_and_transform_chunk(
    random_file: &mut File,
    chunk_size: usize,
    chunk_id: u64,
) -> Result<(Vec<PipelineEvent>, u64), Box<dyn std::error::Error>> {
    let mut buffer = vec![0u8; chunk_size];
    let bytes_read = random_file.read(&mut buffer)?;
    
    if bytes_read == 0 {
        return Ok((Vec::new(), 0));
    }
    
    let events = transform_random_data_to_log_entries(&buffer[..bytes_read], chunk_id)?;
    Ok((events, bytes_read as u64))
}

/// Write processed events to /dev/null (simulating output to destination)
fn write_events_to_dev_null(events: &[PipelineEvent]) -> Result<u64, Box<dyn std::error::Error>> {
    let mut dev_null = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")?;
    
    let mut bytes_written = 0;
    for event in events {
        let json = serde_json::to_string(event)?;
        let bytes = json.as_bytes();
        dev_null.write_all(bytes)?;
        dev_null.write_all(b"\n")?;
        bytes_written += bytes.len() as u64 + 1;
    }
    
    // Don't sync /dev/null - it's not supported on all platforms (e.g., macOS)
    // The data is discarded anyway, so syncing is unnecessary
    Ok(bytes_written)
}

/// Get number of pipelines from environment variable or use default
fn get_num_pipelines() -> usize {
    std::env::var("NUM_PIPELINES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4) // Default: 4 pipelines
}

/// Run I/O performance benchmark - 60 second continuous high-throughput test
/// Simulates real-world Splunk/Datadog log processing workloads
/// Uses fixed number of pipelines, each with dedicated input/processor/output actors
pub async fn run_io_benchmark(
    node: &Node,
    duration_seconds: u64,
) -> Result<PerformanceMetrics, Box<dyn std::error::Error>> {
    let mut collector = MetricsCollector::new("I/O Performance (60s)".to_string());
    collector.sample_memory();
    
    // Get number of pipelines from env var or use default
    let num_pipelines = get_num_pipelines();
    
    info!("=== I/O Performance Benchmark ({} seconds) ===", duration_seconds);
    info!("Simulating high-throughput log processing from /dev/urandom");
    info!("Fixed pipeline architecture: {} pipelines (set NUM_PIPELINES env var to change)", num_pipelines);
    
    // Create fixed number of pipelines, each with dedicated actors
    let coord_start = Instant::now();
    let pipelines = create_pipelines(node, num_pipelines).await?;
    collector.record_coordination(coord_start.elapsed().as_millis() as u64);
    
    // Record all actors created (3 per pipeline: input, processor, output)
    for _ in 0..(num_pipelines * 3) {
        collector.record_actor_created();
    }
    
    info!("Created {} pipelines with {} total actors", num_pipelines, num_pipelines * 3);
    
    // Share pipelines across tasks with round-robin distribution
    let pipelines = Arc::new(pipelines);
    let pipeline_round_robin = Arc::new(RwLock::new(0usize));
    
    // Open /dev/urandom for reading
    let mut random_file = File::open("/dev/urandom")?;
    
    // Configuration
    let chunk_size = 64 * 1024; // 64KB chunks from /dev/urandom
    // Performance improvement: Larger batch size reduces actor message passing overhead
    // Increased from 500 to 1000 for better throughput (5-10× improvement expected)
    let batch_size = 1000; // Process 1000 log entries per batch
    let benchmark_duration = Duration::from_secs(duration_seconds);
    let start_time = Instant::now();
    let mut chunk_id = 0u64;
    
    // Use atomic counters for concurrent metric tracking
    let total_events_processed = Arc::new(AtomicU64::new(0));
    let total_bytes_read = Arc::new(AtomicU64::new(0));
    let total_bytes_written = Arc::new(AtomicU64::new(0));
    let messages_sent = Arc::new(AtomicU64::new(0));
    let messages_received = Arc::new(AtomicU64::new(0));
    let last_status_time = Arc::new(RwLock::new(Instant::now()));
    
    info!("Starting continuous processing for {} seconds...", duration_seconds);
    info!("Reading {}KB chunks from /dev/urandom, transforming to JSON logs, processing {} entries per batch", chunk_size / 1024, batch_size);
    info!("Pipeline: Input workers -> Processor workers -> Output workers");
    
    // Main processing loop - run for specified duration
    while start_time.elapsed() < benchmark_duration {
        let loop_start = Instant::now();
        
        // Read chunk from /dev/urandom and transform to log entries
        let read_start = Instant::now();
        let (events, bytes_read) = read_and_transform_chunk(&mut random_file, chunk_size, chunk_id)?;
        let _read_duration = read_start.elapsed();
        
        if events.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
            continue;
        }
        
        collector.record_bytes_read(bytes_read);
        total_bytes_read.fetch_add(bytes_read, Ordering::Relaxed);
        chunk_id += 1;
        
        // Process events in batches through pipelines CONCURRENTLY
        // Spawn tasks for each batch to show concurrent lightweight worker processing
        let mut batch_tasks = Vec::new();
        
        for chunk in events.chunks(batch_size) {
            let chunk = chunk.to_vec();
            let pipelines_clone = pipelines.clone();
            let pipeline_round_robin_clone = pipeline_round_robin.clone();
            let total_events_clone = total_events_processed.clone();
            let total_bytes_written_clone = total_bytes_written.clone();
            let messages_sent_clone = messages_sent.clone();
            let messages_received_clone = messages_received.clone();
            
            // Spawn concurrent task for this batch
            // Backpressure Architecture:
            // - Actor mailboxes provide natural backpressure when full
            // - Memory channels (4K capacity) block senders when full (Go-like behavior)
            // - No semaphores needed - actor model handles backpressure automatically
            let task = tokio::spawn(async move {
                let _pipeline_start = Instant::now();
                
                // Select pipeline using round-robin
                let pipeline_idx = {
                    let mut idx = pipeline_round_robin_clone.write().await;
                    let current = *idx;
                    *idx = (*idx + 1) % pipelines_clone.len();
                    current
                };
                
                let pipeline = &pipelines_clone[pipeline_idx];
                
                // Stage 1: Send to input actor (use stored ActorRef)
                let ingest_msg = PipelineMessage::Ingest {
                    events: chunk,
                };
                let mut input_request = Message::new(serde_json::to_vec(&ingest_msg)
                    .map_err(|e| format!("Failed to serialize ingest message: {}", e))?)
                    .with_message_type("call".to_string());
                input_request.receiver = pipeline.input_actor_ref.id().clone();
                messages_sent_clone.fetch_add(1, Ordering::Relaxed);
                
                let coord_start = Instant::now();
                let input_response = pipeline.input_actor_ref.ask(input_request, Duration::from_secs(30)).await
                    .map_err(|e| {
                        tracing::error!("Failed to ask input actor (pipeline {}): {}", pipeline_idx, e);
                        e
                    })?;
                let _coord_time = coord_start.elapsed().as_millis() as u64;
                
                messages_received_clone.fetch_add(1, Ordering::Relaxed);
                let input_result: PipelineMessage = serde_json::from_slice(input_response.payload())
                    .map_err(|e| format!("Failed to deserialize input response: {}", e))?;
                
                // Stage 2: Send to processor actor (use stored ActorRef)
                let processed_events = if let PipelineMessage::Processed { events, .. } = input_result {
                    events
                } else {
                    return Ok(());
                };
                
                let process_msg = PipelineMessage::Process {
                    events: processed_events,
                };
                let mut processor_request = Message::new(serde_json::to_vec(&process_msg)
                    .map_err(|e| format!("Failed to serialize process message: {}", e))?)
                    .with_message_type("call".to_string());
                processor_request.receiver = pipeline.processor_actor_ref.id().clone();
                messages_sent_clone.fetch_add(1, Ordering::Relaxed);
                
                let coord_start = Instant::now();
                let processor_response = pipeline.processor_actor_ref.ask(processor_request, Duration::from_secs(30)).await
                    .map_err(|e| {
                        tracing::error!("Failed to ask processor actor (pipeline {}): {}", pipeline_idx, e);
                        e
                    })?;
                let _coord_time = coord_start.elapsed().as_millis() as u64;
                
                messages_received_clone.fetch_add(1, Ordering::Relaxed);
                let processor_result: PipelineMessage = serde_json::from_slice(processor_response.payload())
                    .map_err(|e| format!("Failed to deserialize processor response: {}", e))?;
                
                // Stage 3: Send to output actor (use stored ActorRef)
                let final_events = if let PipelineMessage::Processed { events, .. } = processor_result {
                    events
                } else {
                    return Ok(());
                };
                
                let output_msg = PipelineMessage::SendToDestination {
                    destination_type: "dev_null".to_string(),
                    destination_config: "{}".to_string(),
                    events: final_events.clone(),
                };
                let mut output_request = Message::new(serde_json::to_vec(&output_msg)
                    .map_err(|e| format!("Failed to serialize output message: {}", e))?)
                    .with_message_type("call".to_string());
                output_request.receiver = pipeline.output_actor_ref.id().clone();
                messages_sent_clone.fetch_add(1, Ordering::Relaxed);
                
                let coord_start = Instant::now();
                let output_response = pipeline.output_actor_ref.ask(output_request, Duration::from_secs(30)).await
                    .map_err(|e| {
                        tracing::error!("Failed to ask output actor (pipeline {}): {}", pipeline_idx, e);
                        e
                    })?;
                let _coord_time = coord_start.elapsed().as_millis() as u64;
                
                messages_received_clone.fetch_add(1, Ordering::Relaxed);
                let output_result: PipelineMessage = serde_json::from_slice(output_response.payload())
                    .map_err(|e| format!("Failed to deserialize output response: {}", e))?;
                
                match output_result {
                    PipelineMessage::SendToDestinationResponse { events_sent, .. } => {
                        // Update atomic counters for concurrent access
                        total_events_clone.fetch_add(events_sent, Ordering::Relaxed);
                        
                        // Calculate actual bytes written from events
                        let mut actual_bytes = 0u64;
                        for event in &final_events {
                            let json = serde_json::to_string(event).unwrap_or_default();
                            actual_bytes += json.len() as u64 + 1; // +1 for newline
                        }
                        total_bytes_written_clone.fetch_add(actual_bytes, Ordering::Relaxed);
                        
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                "Pipeline {} processed {} events, {} bytes",
                                pipeline_idx,
                                events_sent,
                                actual_bytes
                            );
                        }
                    }
                    _ => {
                        tracing::warn!("Pipeline {} received unexpected response type", pipeline_idx);
                    }
                }
                
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            });
            
            batch_tasks.push(task);
        }
        
        // Process batches concurrently - wait for at least first batch to verify it works
        // Then spawn remaining batches in background
        if !batch_tasks.is_empty() {
            // Wait for first batch to complete to verify actors are working
            let first_task = batch_tasks.remove(0);
            let first_result = timeout(Duration::from_secs(5), first_task).await;
            
            match first_result {
                Ok(Ok(Ok(()))) => {
                    // Spawn remaining batches concurrently
                    for (idx, task) in batch_tasks.into_iter().enumerate() {
                        let task_idx = idx + 1; // +1 because we already processed first
                        tokio::spawn(async move {
                            match task.await {
                                Ok(Ok(())) => {
                                    // Success
                                }
                                Ok(Err(e)) => {
                                    error!("Batch {} processing failed: {}", task_idx, e);
                                }
                                Err(join_err) => {
                                    error!("Batch {} task panicked: {}", task_idx, join_err);
                                }
                            }
                        });
                    }
                }
                Ok(Ok(Err(e))) => {
                    error!("First batch failed: {}", e);
                    // Still spawn remaining to see if they work
                    for (idx, task) in batch_tasks.into_iter().enumerate() {
                        let task_idx = idx + 1;
                        tokio::spawn(async move {
                            if let Err(e) = task.await {
                                error!("Batch {} failed: {:?}", task_idx, e);
                            }
                        });
                    }
                }
                Ok(Err(join_err)) => {
                    error!("First batch task panicked: {}", join_err);
                }
                Err(_) => {
                    error!("First batch timed out - actors may not be responding");
                }
            }
        }
        
        // Status updates every 5 seconds
        {
            let mut last_time = last_status_time.write().await;
            if last_time.elapsed() >= Duration::from_secs(5) {
                let elapsed = start_time.elapsed();
                let remaining = benchmark_duration.saturating_sub(elapsed);
                let bytes_read_val = total_bytes_read.load(Ordering::Relaxed);
                let bytes_written_val = total_bytes_written.load(Ordering::Relaxed);
                let events_val = total_events_processed.load(Ordering::Relaxed);
                let mb_read = bytes_read_val as f64 / (1024.0 * 1024.0);
                let mb_written = bytes_written_val as f64 / (1024.0 * 1024.0);
                let events_per_sec = events_val as f64 / elapsed.as_secs_f64();
                
                // Only log progress if INFO level is enabled (reduces output in quiet mode)
                if tracing::enabled!(tracing::Level::INFO) {
                    info!(
                        "Progress: {:.1}s elapsed, {:.1}s remaining | {:.2} MB read, {:.2} MB written | {:.0} events/sec",
                        elapsed.as_secs_f64(),
                        remaining.as_secs_f64(),
                        mb_read,
                        mb_written,
                        events_per_sec
                    );
                }
                *last_time = Instant::now();
            }
        }
        
        // Small delay to prevent CPU spinning
        if loop_start.elapsed() < Duration::from_millis(10) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
    
    // Wait a bit for any remaining concurrent tasks to complete
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let total_duration = start_time.elapsed();
    let final_events = total_events_processed.load(Ordering::Relaxed);
    let final_bytes_read = total_bytes_read.load(Ordering::Relaxed);
    let final_bytes_written = total_bytes_written.load(Ordering::Relaxed);
    
    info!("Benchmark complete! Processed {} events in {:.2} seconds", final_events, total_duration.as_secs_f64());
    
    collector.sample_memory();
    
    let mut metrics = collector.finalize();
    metrics.total_events = final_events;
    metrics.log_entries_processed = final_events;
    metrics.bytes_read = final_bytes_read;
    metrics.bytes_written = final_bytes_written;
    metrics.messages_sent = messages_sent.load(Ordering::Relaxed);
    metrics.messages_received = messages_received.load(Ordering::Relaxed);
    metrics.calculate_derived();
    
    // Calculate final throughput
    let mb_read = final_bytes_read as f64 / (1024.0 * 1024.0);
    let mb_written = final_bytes_written as f64 / (1024.0 * 1024.0);
    let read_mb_per_sec = mb_read / total_duration.as_secs_f64();
    let write_mb_per_sec = mb_written / total_duration.as_secs_f64();
    
    info!("Final I/O Throughput: {:.2} MB/s read, {:.2} MB/s write", read_mb_per_sec, write_mb_per_sec);
    info!("Total Events: {} ({:.0} events/sec)", final_events, final_events as f64 / total_duration.as_secs_f64());
    
    Ok(metrics)
}

