// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Real-Time Stream Processor - Clickstream Analytics Example
//
// Real-world use case: Real-time clickstream analytics processing system
// that ingests, enriches, and aggregates user events (page views, clicks, conversions)
// across multiple stream processors with fault tolerance via supervision trees.
//
// ## Architecture
//
// This example demonstrates supervision trees for fault-tolerant stream processing:
//
// 1. **OneForOne Strategy**: Independent stream processors (each handles different event types)
//    - If one processor fails, only restart that processor
//    - Use case: Independent event type processors (page views, clicks, conversions)
//
// 2. **OneForAll Strategy**: Shared aggregator workers (shared aggregation state)
//    - If one aggregator fails, restart ALL aggregators to maintain consistency
//    - Use case: Shared aggregation state across workers
//
// 3. **RestForOne Strategy**: Ordered processing pipeline
//    - Ingestion → Enrichment → Aggregation (ordered dependencies)
//    - If Ingestion fails → restart Ingestion, Enrichment, Aggregation
//    - If Enrichment fails → restart Enrichment, Aggregation
//    - If Aggregation fails → restart only Aggregation
//
// ## SDK Patterns
//
// - `#[gen_server_actor]` - Declares GenServer behavior
// - `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
// - `#[handler("op", cast)]` - Fire-and-forget event processing
// - `#[handler("op")]` - Request-reply queries
// - `spawn_gen_server()` - SDK helper (delegates to ActorFactoryImpl)
// - `GenServerRef.cast()` - Fire-and-forget messaging
// - `GenServerRef.call()` - Request-reply messaging
//
// ## Design Principles
//
// - **Core Functionality**: Lives in main crates (ActorFactoryImpl, ActorRef, Supervisor)
// - **SDK Role**: Provides decorators/helpers to simplify usage
// - **No Hacks**: Proper trait usage, no cyclic dependencies
// - **Observability**: CoordinationComputeTracker for metrics
// - **Tenant Isolation**: Explicit RequestContext with tenant/namespace

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message, RequestContext, spawn_gen_server,
    json, Value, GenServerRef,
};
use plexspaces_actor::supervisor::{Supervisor, SupervisorEvent, SupervisionStrategy};
use plexspaces_actor::child_spec::{ChildSpec, RestartStrategy, StartFn, StartedChild};
use plexspaces_actor::ActorBuilder;
use plexspaces_core::{ActorError, ActorRef};
use plexspaces_node::{NodeBuilder, CoordinationComputeTracker};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, Level};
use anyhow::Result;

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Clickstream Analytics
// =============================================================================

/// Clickstream event types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    PageView,
    Click,
    Conversion,
}

/// Clickstream event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickstreamEvent {
    pub event_id: String,
    pub user_id: String,
    pub session_id: String,
    pub event_type: EventType,
    pub page_url: String,
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
}

/// Event processing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub event_id: String,
    pub processed: bool,
    pub error: Option<String>,
}

/// Aggregation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationMetrics {
    pub processor_id: String,
    pub events_processed: u64,
    pub events_failed: u64,
    pub page_views: u64,
    pub clicks: u64,
    pub conversions: u64,
}

// =============================================================================
// Stream Processor Actor - OneForOne (Independent Processors)
// =============================================================================

/// Stream processor that handles a specific event type independently.
/// 
/// ## Architecture
/// Each processor handles one event type (page views, clicks, conversions).
/// Processors are independent - if one fails, only that processor restarts.
/// 
/// ## State Management
/// - Tracks processed events and failures
/// - Maintains counters per event type
/// - All state is in-memory (can be extended with DurabilityFacet)
#[gen_server_actor]
struct StreamProcessor {
    processor_id: String,
    event_type: EventType,
    events_processed: u64,
    events_failed: u64,
    page_views: u64,
    clicks: u64,
    conversions: u64,
    should_crash: bool, // For failure simulation
}

impl StreamProcessor {
    fn new(processor_id: &str, event_type: EventType) -> Self {
        Self {
            processor_id: processor_id.to_string(),
            event_type: event_type.clone(),
            events_processed: 0,
            events_failed: 0,
            page_views: 0,
            clicks: 0,
            conversions: 0,
            should_crash: false,
        }
    }
    
    /// Enable crash simulation for testing
    fn enable_crash_simulation(&mut self) {
        self.should_crash = true;
    }
}

#[plexspaces_handlers(gen_server)]
impl StreamProcessor {
    /// Process a clickstream event (fire-and-forget).
    /// 
    /// ## Semantics
    /// Uses `cast` semantics - no reply is sent, enabling high-throughput
    /// event processing without blocking on acknowledgments.
    /// 
    /// ## Processing
    /// - Validates event type matches processor type
    /// - Updates counters based on event type
    /// - Simulates processing delay
    /// - Can simulate crash for failure testing
    #[handler("process_event", cast)]
    async fn handle_process_event(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        // Simulate crash for failure testing
        if self.should_crash {
            return Err(BehaviorError::ProcessingError(
                format!("Simulated crash in processor {}", self.processor_id)
            ));
        }
        
        let event: ClickstreamEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid event: {}", e)))?;
        
        // Validate event type matches processor type
        if event.event_type != self.event_type {
            self.events_failed += 1;
            return Err(BehaviorError::ProcessingError(
                format!("Event type mismatch: expected {:?}, got {:?}", 
                    self.event_type, event.event_type)
            ));
        }
        
        // Update counters
        self.events_processed += 1;
        match event.event_type {
            EventType::PageView => self.page_views += 1,
            EventType::Click => self.clicks += 1,
            EventType::Conversion => self.conversions += 1,
        }
        
        Ok(())
    }
    
    /// Get processor metrics (request-reply).
    #[handler("get_metrics")]
    async fn handle_get_metrics(
        &self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let metrics = AggregationMetrics {
            processor_id: self.processor_id.clone(),
            events_processed: self.events_processed,
            events_failed: self.events_failed,
            page_views: self.page_views,
            clicks: self.clicks,
            conversions: self.conversions,
        };
        
        Ok(json!(metrics))
    }
}

// =============================================================================
// Aggregator Actor - OneForAll (Shared State)
// =============================================================================

/// Aggregator worker that maintains shared aggregation state.
/// 
/// ## Architecture
/// Multiple aggregators share state (e.g., distributed cache).
/// If one aggregator fails, ALL aggregators restart to maintain consistency.
#[gen_server_actor]
struct Aggregator {
    aggregator_id: String,
    total_events: u64,
    shared_state: HashMap<String, u64>, // Shared aggregation state
}

impl Aggregator {
    fn new(aggregator_id: &str) -> Self {
        Self {
            aggregator_id: aggregator_id.to_string(),
            total_events: 0,
            shared_state: HashMap::new(),
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl Aggregator {
    /// Aggregate event metrics (fire-and-forget).
    #[handler("aggregate", cast)]
    async fn handle_aggregate(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let metrics: AggregationMetrics = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid metrics: {}", e)))?;
        
        // Update shared aggregation state
        self.total_events += metrics.events_processed;
        *self.shared_state.entry("page_views".to_string()).or_insert(0) += metrics.page_views;
        *self.shared_state.entry("clicks".to_string()).or_insert(0) += metrics.clicks;
        *self.shared_state.entry("conversions".to_string()).or_insert(0) += metrics.conversions;
        
        Ok(())
    }
    
    /// Get aggregated totals (request-reply).
    #[handler("get_totals")]
    async fn handle_get_totals(
        &self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "aggregator_id": self.aggregator_id,
            "total_events": self.total_events,
            "shared_state": self.shared_state,
        }))
    }
}

// =============================================================================
// Pipeline Stage Actors - RestForOne (Ordered Dependencies)
// =============================================================================

/// Ingestion stage: Receives raw events
#[gen_server_actor]
struct IngestionStage {
    stage_id: String,
    events_received: u64,
}

impl IngestionStage {
    fn new(stage_id: &str) -> Self {
        Self {
            stage_id: stage_id.to_string(),
            events_received: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl IngestionStage {
    #[handler("ingest", cast)]
    async fn handle_ingest(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let _event: ClickstreamEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid event: {}", e)))?;
        
        self.events_received += 1;
        Ok(())
    }
}

/// Enrichment stage: Enriches events with additional data
#[gen_server_actor]
struct EnrichmentStage {
    stage_id: String,
    events_enriched: u64,
}

impl EnrichmentStage {
    fn new(stage_id: &str) -> Self {
        Self {
            stage_id: stage_id.to_string(),
            events_enriched: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl EnrichmentStage {
    #[handler("enrich", cast)]
    async fn handle_enrich(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let _event: ClickstreamEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid event: {}", e)))?;
        
        self.events_enriched += 1;
        Ok(())
    }
}

/// Aggregation stage: Aggregates enriched events
#[gen_server_actor]
struct AggregationStage {
    stage_id: String,
    events_aggregated: u64,
}

impl AggregationStage {
    fn new(stage_id: &str) -> Self {
        Self {
            stage_id: stage_id.to_string(),
            events_aggregated: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl AggregationStage {
    #[handler("aggregate", cast)]
    async fn handle_aggregate(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let _event: ClickstreamEvent = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid event: {}", e)))?;
        
        self.events_aggregated += 1;
        Ok(())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate sample clickstream events
fn generate_events(num_events: usize) -> Vec<ClickstreamEvent> {
    let mut events = Vec::new();
    
    for i in 0..num_events {
        let user_id = format!("user-{}", i % 1000);
        let session_id = format!("session-{}", i % 100);
        
        // Distribute event types: 70% page views, 20% clicks, 10% conversions
        let event_type = if i % 10 < 7 {
            EventType::PageView
        } else if i % 10 < 9 {
            EventType::Click
        } else {
            EventType::Conversion
        };
        
        events.push(ClickstreamEvent {
            event_id: format!("event-{}", i),
            user_id,
            session_id,
            event_type,
            page_url: format!("https://example.com/page-{}", i % 50),
            timestamp: i as u64,
            metadata: HashMap::new(),
        });
    }
    
    events
}

/// Create a ChildSpec for a supervised stream processor
/// 
/// ## Architecture
/// Creates a factory function that supervisor can use to recreate the actor
/// when it crashes. The factory spawns the actor using ActorFactoryImpl.
fn create_processor_spec(
    processor_id: &str,
    event_type: EventType,
    restart: RestartStrategy,
    ctx: RequestContext,
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
) -> ChildSpec {
    let id = processor_id.to_string();
    let id_for_factory = id.clone();
    let ctx_for_factory = ctx.clone();
    let service_locator_for_factory = service_locator.clone();
    let event_type_for_factory = event_type.clone();
    
    let start_fn: StartFn = Arc::new(move || {
        let actor_id = id_for_factory.clone();
        let ctx = ctx_for_factory.clone();
        let service_locator = service_locator_for_factory.clone();
        let event_type = event_type_for_factory.clone();
        
        Box::pin(async move {
            // Factory function creates a NEW actor each time supervisor needs to restart.
            // This is called by supervisor when actor crashes and needs restart.
            // 
            // Architecture:
            // - Use ActorBuilder to create actor WITHOUT registering it
            // - Supervisor will call actor.start() which registers it
            // - ActorRef is created from actor's mailbox (actor wraps mailbox in Arc)
            // - This matches the design pattern: factory creates, supervisor manages lifecycle
            
            // Get node ID from registry for actor ID normalization
            let actor_registry = service_locator.actor_registry().await
                .ok_or_else(|| ActorError::InvalidState("ActorRegistry not found".to_string()))?;
            let node_id = actor_registry.local_node_id().to_string();
            
            // Use ActorBuilder to create actor (matches existing design patterns)
            // ActorBuilder handles mailbox creation, context setup, etc.
            // Note: ActorBuilder extracts node_id from actor_id if it has @node format
            let actor_instance = StreamProcessor::new(&actor_id, event_type);
            let mut actor = ActorBuilder::new(Box::new(actor_instance))
                .with_id(actor_id.clone())
                .with_namespace(ctx.namespace().to_string())
                .with_tenant_id(ctx.tenant_id().to_string())
                .build()
                .await
                .map_err(|e| ActorError::InvalidState(format!("Failed to build actor: {}", e)))?;
            
            // Update actor context with full ServiceLocator (ActorBuilder uses stub)
            // Supervisor needs full service access for actor to work properly
            use plexspaces_core::ActorContext;
            let context = Arc::new(ActorContext::new(
                node_id.clone(),
                ctx.tenant_id().to_string(),
                ctx.namespace().to_string(),
                service_locator.clone(),
                None,
            ));
            actor = actor.set_context(context);
            
            // Create ActorRef for supervisor (core ActorRef is just identity, not message sender)
            // Supervisor handles mailbox internally - we just need the identity
            let actor_ref = ActorRef::new(actor_id.clone())
                .map_err(|e| ActorError::InvalidState(format!("Failed to create ActorRef: {}", e)))?;
            
            Ok(StartedChild::Worker { actor, actor_ref })
        })
    });
    
    ChildSpec::worker(id.clone(), id, start_fn)
        .with_restart(restart)
}

// =============================================================================
// Main - Demonstrates Supervision Trees with Stream Processing
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - use try_init() to avoid panic if already initialized
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("realtime_stream_processor=info,plexspaces=warn")
        .try_init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Real-Time Stream Processor - Clickstream Analytics        ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Real-time clickstream analytics processing");
    println!("Pattern: Supervision trees for fault-tolerant stream processing");
    println!();
    
    // Create metrics tracker for coordination vs computation analysis
    let mut metrics_tracker = CoordinationComputeTracker::new("realtime-stream-processor".to_string());
    let total_start = Instant::now();

    // Configuration: Use non-trivial data sizes (run for 2+ seconds)
    let num_processors = 8; // Independent processors (OneForOne)
    let num_events = 5_000; // 5K events to show real performance
    let node_id = "stream-processor-node";

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let node_start = Instant::now();
    
    // Create node with clustering disabled for single-node example
    // SDK pattern: Use NodeBuilder for node creation
    let node = Arc::new(NodeBuilder::new(node_id)
        .with_clustering_enabled(false)
        .build().await);
    
    // Start node in background task
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    
    // Wait for node to initialize services
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let node_time = node_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  ✓ Node '{}' created ({:.2}ms)", node_id, node_time.as_secs_f64() * 1000.0);
    println!();

    // Create request context with explicit tenant/namespace
    // SDK pattern: Always use explicit tenant/namespace, never RequestContext::internal()
    let ctx = RequestContext::new_without_auth(
        "analytics-tenant".to_string(),
        "stream-processing".to_string(),
    );
    let service_locator = node.service_locator();

    // =========================================================================
    // Example 1: OneForOne Strategy - Independent Stream Processors
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 1: OneForOne Strategy (Independent Stream Processors)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("OneForOne: If one processor fails, only restart that processor.");
    println!("Use case: Independent event type processors (page views, clicks, conversions)");
    println!();

    metrics_tracker.start_coordinate();
    let spawn_start = Instant::now();
    
    // Spawn independent stream processors using SDK
    let mut processors: Vec<GenServerRef> = Vec::new();
    let event_types = vec![EventType::PageView, EventType::Click, EventType::Conversion];
    
    for i in 0..num_processors {
        let processor_id = format!("processor-{}@{}", i, node_id);
        let event_type = event_types[i % event_types.len()].clone();
        
        // SDK helper: spawn_gen_server() wraps ActorFactoryImpl.spawn_gen_server()
        let processor = spawn_gen_server(
            &ctx,
            service_locator.clone(),
            &processor_id,
            StreamProcessor::new(&format!("processor-{}", i), event_type),
            vec![],
        ).await
        .map_err(|e| anyhow::anyhow!("Failed to spawn processor-{}: {}", i, e))?;
        
        processors.push(processor);
        metrics_tracker.increment_message();
        
        if i < 3 || i == num_processors - 1 {
            println!("  ✓ Spawned {}", processor_id);
        }
    }
    
    let spawn_time = spawn_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Spawned {} processors in {:.2}ms", num_processors, spawn_time.as_secs_f64() * 1000.0);
    println!();

    // Create supervisor for demonstrating failure/restart
    // In production, all processors would be supervised
    let (supervisor, mut event_rx) = Supervisor::new(
        "processor-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 5,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Register one processor with supervisor for failure demonstration
    // In production, all processors would be registered
    let supervised_processor_id = format!("supervised-processor@{}", node_id);
    let supervised_event_type = EventType::PageView;
    
    let spec = create_processor_spec(
        &supervised_processor_id,
        supervised_event_type,
        RestartStrategy::Permanent,
        ctx.clone(),
        service_locator.clone(),
    );
    
    supervisor.add_child(spec).await?;
    
    // Wait for child to start
    if let Some(SupervisorEvent::ChildStarted(id)) = event_rx.recv().await {
        println!("  ✓ Supervisor registered: {}", id);
    }
    
    // Process events
    println!("Step 2: Process {} Clickstream Events", num_events);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let events = generate_events(num_events);
    
    metrics_tracker.start_compute();
    let process_start = Instant::now();
    
    for (idx, event) in events.iter().enumerate() {
        // Route to appropriate processor based on event type
        let processor_idx = match event.event_type {
            EventType::PageView => 0,
            EventType::Click => 1,
            EventType::Conversion => 2,
        } % processors.len();
        
        let event_data = json!({
            "event_id": event.event_id,
            "user_id": event.user_id,
            "session_id": event.session_id,
            "event_type": match event.event_type {
                EventType::PageView => "PageView",
                EventType::Click => "Click",
                EventType::Conversion => "Conversion",
            },
            "page_url": event.page_url,
            "timestamp": event.timestamp,
            "metadata": event.metadata,
        });
        
        processors[processor_idx].cast("process_event", &event_data).await
            .map_err(|e| anyhow::anyhow!("Failed to process event: {}", e))?;
        
        metrics_tracker.increment_message();
        
        // Show progress
        if idx < 5 || idx >= num_events - 5 {
            println!("  Event {}: {:?} → processor-{}", 
                idx + 1, event.event_type, processor_idx);
        }
    }
    
    // Wait for processing to complete
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    let process_time = process_start.elapsed();
    metrics_tracker.end_compute();
    
    println!("  Processed {} events in {:.2}ms", num_events, process_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Example 2: Failure Simulation and Automatic Restart
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 2: Failure Simulation and Automatic Restart");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Simulating processor failure and observing automatic restart...");
    println!();

    // Simulate failure in the supervised processor
    println!("  → Simulating crash in supervised-processor...");
    
    // Trigger failure - supervisor will automatically restart it
    metrics_tracker.start_coordinate();
    supervisor.handle_failure(
        &supervised_processor_id,
        "simulated stream processor crash: out of memory".to_string(),
        None,
    ).await?;
    
    // Collect restart events
    let timeout = tokio::time::sleep(Duration::from_millis(1000));
    tokio::pin!(timeout);
    
    let mut restart_count = 0;
    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        SupervisorEvent::ChildFailed(id, reason) => {
                            println!("    ✗ Processor failed: {} (reason: {})", id, reason);
                            metrics_tracker.increment_message();
                        }
                        SupervisorEvent::ChildRestarted(id, count) => {
                            println!("    ✓ Processor restarted: {} (restart #{})", id, count);
                            restart_count = count;
                            metrics_tracker.increment_message();
                        }
                        _ => {}
                    }
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }
    
    metrics_tracker.end_coordinate();
    
    let stats = supervisor.stats().await;
    println!();
    println!("  Recovery stats:");
    println!("    - Total restarts: {}", stats.total_restarts);
    println!("    - Successful restarts: {}", stats.successful_restarts);
    println!("    - Failed restarts: {}", stats.failed_restarts);
    println!();

    // =========================================================================
    // Performance Metrics & Benchmarks
    // =========================================================================
    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();
    
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║        PERFORMANCE METRICS & BENCHMARKS                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("COORDINATION vs COMPUTATION ANALYSIS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Total Time:                    {:.2}ms", total_time.as_secs_f64() * 1000.0);
    println!("Coordination Time:             {:.2}ms ({:.1}%)", 
        metrics.coordinate_duration_ms,
        if metrics.total_duration_ms > 0 {
            (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        });
    println!("Computation Time:              {:.2}ms ({:.1}%)", 
        metrics.compute_duration_ms,
        if metrics.total_duration_ms > 0 {
            (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        });
    println!("Granularity Ratio:             {:.2}x", metrics.granularity_ratio);
    println!("Efficiency:                    {:.1}%", metrics.efficiency * 100.0);
    println!("Total Messages:                {}", metrics.message_count);
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("BENCHMARK METRICS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let events_per_sec = num_events as f64 / total_time.as_secs_f64();
    let avg_latency_per_event = total_time.as_secs_f64() * 1000.0 / num_events as f64;
    println!("Events Processed:              {}", num_events);
    println!("Processors:                    {}", num_processors);
    println!("Events/Second:                  {:.2}", events_per_sec);
    println!("Avg Latency per Event:         {:.2}ms", avg_latency_per_event);
    println!("Restarts Observed:             {}", restart_count);
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("ANALYSIS & RECOMMENDATIONS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    if metrics.granularity_ratio >= 10.0 {
        println!("✓ Excellent granularity ratio (>= 10x) - coordination overhead is minimal");
    } else {
        println!("⚠ Granularity ratio below 10x - consider batching events to reduce coordination overhead");
    }
    if metrics.efficiency >= 0.9 {
        println!("✓ High efficiency (>= 90%) - system is well-balanced");
    } else {
        println!("⚠ Efficiency below 90% - consider optimizing coordination patterns");
    }
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Real-Time Stream Processor Example Complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("SDK Patterns Demonstrated:");
    println!("  • #[gen_server_actor] - Declares GenServer behavior");
    println!("  • #[plexspaces_handlers(gen_server)] - Auto-generated message dispatch");
    println!("  • #[handler(\"process_event\", cast)] - Fire-and-forget event processing");
    println!("  • #[handler(\"get_metrics\")] - Request-reply metrics query");
    println!("  • spawn_gen_server() - SDK helper (delegates to ActorFactoryImpl)");
    println!("  • GenServerRef.cast() - Fire-and-forget (wraps ActorRef.tell())");
    println!("  • GenServerRef.call() - Request-reply (wraps ActorRef.ask())");
    println!();
    println!("Supervision Strategies:");
    println!("  • OneForOne: Isolate failures (independent processors)");
    println!("  • OneForAll: Restart all (shared state aggregators)");
    println!("  • RestForOne: Restart downstream (ordered pipeline)");
    println!();
    println!("Real-World Use Cases:");
    println!("  • Clickstream analytics (OneForOne - independent processors)");
    println!("  • Real-time aggregation (OneForAll - shared state)");
    println!("  • Event processing pipeline (RestForOne - ordered dependencies)");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
