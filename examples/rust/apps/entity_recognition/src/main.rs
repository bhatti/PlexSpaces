// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Entity Recognition App – Showcases FSM, GenServer, and GenEvent patterns
//
// Demonstrates **PlexSpaces Rust SDK annotations** for multiple behavior types:
// - `#[fsm_actor]` - Finite State Machine for document processing pipeline
// - `#[gen_server_actor]` - Request-reply for entity extraction service
// - `#[event_actor]` - Fire-and-forget for audit logging
//
// Use case: Document processing pipeline with entity extraction and audit trail.
//
// Pipeline:
//   Document → [DocumentProcessor FSM] → [EntityExtractor GenServer] → [AuditLogger GenEvent]
//              (Pending→Processing→Done)   (extract entities)          (log events)

use plexspaces_sdk::{
    // Actor annotations
    fsm_actor, gen_server_actor, event_actor,
    plexspaces_handlers,
    // Core types
    ActorContext, ActorId, BehaviorError, Message,
    // Node and context
    NodeBuilder, RequestContext,
    // Spawn helpers - use spawn_with_facets for explicit facets
    spawn_with_facets,
    // Facets
    TimerFacet,
    // Message helpers - invocation-based constructors
    call_message, cast_message,
    // Utilities
    json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, Level};

// =============================================================================
// Domain Types
// =============================================================================

/// Entity extracted from a document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_type: String,  // PERSON, ORG, LOCATION, DATE, etc.
    pub value: String,
    pub confidence: f32,
}

/// Document processing state (FSM states)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessingState {
    Pending,
    Processing,
    Extracting,
    Complete,
    Failed,
}

impl Default for ProcessingState {
    fn default() -> Self {
        Self::Pending
    }
}

// =============================================================================
// DocumentProcessor - FSM Actor (State Machine Pattern)
// =============================================================================

/// Document processor using FSM pattern for pipeline state management.
/// 
/// States: Pending → Processing → Extracting → Complete
///                              ↘ Failed
/// 
/// ## Annotations
/// - `#[fsm_actor(facets = ["timer"])]` - FSM with timer for timeouts
/// - `#[plexspaces_handlers(fsm)]` - Generates FSM dispatch
/// - `#[handler("event")]` - State transition handlers
#[fsm_actor(facets = ["timer"])]
pub struct DocumentProcessor {
    doc_id: String,
    state: ProcessingState,
    content: String,
    entities: Vec<Entity>,
    error: Option<String>,
}

impl DocumentProcessor {
    pub fn new(doc_id: &str) -> Self {
        Self {
            doc_id: doc_id.to_string(),
            state: ProcessingState::Pending,
            content: String::new(),
            entities: Vec::new(),
            error: None,
        }
    }
    
    pub fn state(&self) -> ProcessingState {
        self.state
    }
}

#[plexspaces_handlers(fsm)]
impl DocumentProcessor {
    /// Submit document for processing (Pending → Processing)
    #[handler("submit")]
    async fn handle_submit(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        if self.state != ProcessingState::Pending {
            info!("[DocumentProcessor] Cannot submit - not in Pending state");
            return Err(BehaviorError::TransitionFailed("Not in Pending state".to_string()));
        }
        
        // Parse document content from payload
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .unwrap_or_else(|_| json!({}));
        self.content = payload.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        
        self.state = ProcessingState::Processing;
        info!(
            "[DocumentProcessor] {} submitted, {} chars - state: {:?}",
            self.doc_id,
            self.content.len(),
            self.state
        );
        Ok(())
    }
    
    /// Start entity extraction (Processing → Extracting)
    #[handler("extract")]
    async fn handle_extract(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<(), BehaviorError> {
        if self.state != ProcessingState::Processing {
            info!("[DocumentProcessor] Cannot extract - not in Processing state");
            return Err(BehaviorError::TransitionFailed("Not in Processing state".to_string()));
        }
        
        self.state = ProcessingState::Extracting;
        info!("[DocumentProcessor] {} extracting entities - state: {:?}", self.doc_id, self.state);
        Ok(())
    }
    
    /// Complete with extracted entities (Extracting → Complete)
    #[handler("complete")]
    async fn handle_complete(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        if self.state != ProcessingState::Extracting {
            info!("[DocumentProcessor] Cannot complete - not in Extracting state");
            return Err(BehaviorError::TransitionFailed("Not in Extracting state".to_string()));
        }
        
        // Parse entities from payload
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .unwrap_or_else(|_| json!({}));
        if let Some(entities) = payload.get("entities") {
            self.entities = serde_json::from_value(entities.clone()).unwrap_or_default();
        }
        
        self.state = ProcessingState::Complete;
        info!(
            "[DocumentProcessor] {} complete with {} entities - state: {:?}",
            self.doc_id,
            self.entities.len(),
            self.state
        );
        Ok(())
    }
    
    /// Mark as failed (Any → Failed)
    #[handler("fail")]
    async fn handle_fail(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .unwrap_or_else(|_| json!({}));
        self.error = payload.get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        self.state = ProcessingState::Failed;
        info!(
            "[DocumentProcessor] {} failed: {:?} - state: {:?}",
            self.doc_id,
            self.error,
            self.state
        );
        Ok(())
    }
    
    /// Query current state (FSM with call semantics - returns state to caller)
    /// 
    /// Non-GenServer actors can opt into request-reply by using `#[handler("op", call)]`.
    /// The macro will serialize the return value and call ctx.send_reply().
    #[handler("get_state", call)]
    async fn handle_get_state(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        info!(
            "[DocumentProcessor] State query: doc_id={}, state={:?}, entities={}, error={:?}",
            self.doc_id, self.state, self.entities.len(), self.error
        );
        
        // Return state - macro handles serialization and ctx.send_reply()
        Ok(json!({
            "doc_id": self.doc_id,
            "state": format!("{:?}", self.state),
            "entity_count": self.entities.len(),
            "error": self.error,
        }))
    }
}

// =============================================================================
// EntityExtractor - GenServer Actor (Request-Reply Pattern)
// =============================================================================

/// Entity extractor using GenServer pattern for synchronous extraction.
/// 
/// ## Annotations
/// - `#[gen_server_actor]` - Request-reply behavior
/// - `#[plexspaces_handlers]` - Generates GenServer dispatch (call by default)
/// - `#[handler("op")]` - Call handlers (return value sent as reply via ctx.send_reply)
/// 
/// ## GenServer Call Semantics
/// GenServer handlers default to "call" - the macro automatically:
/// 1. Invokes the handler method
/// 2. Serializes the return value (serde_json::Value)
/// 3. Calls ctx.send_reply() to send the response back to the caller
#[gen_server_actor]
pub struct EntityExtractor {
    model_name: String,
    extraction_count: u32,
}

impl EntityExtractor {
    pub fn new(model_name: &str) -> Self {
        Self {
            model_name: model_name.to_string(),
            extraction_count: 0,
        }
    }
}

#[plexspaces_handlers]
impl EntityExtractor {
    /// Extract entities from text (GenServer call - returns entities to caller)
    /// 
    /// The macro generates code that:
    /// 1. Calls this method
    /// 2. Serializes the returned serde_json::Value
    /// 3. Sends reply via ctx.send_reply(correlation_id, sender_id, target_id, reply)
    #[handler("extract")]
    async fn handle_extract(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .unwrap_or_else(|_| json!({}));
        
        let text = payload.get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        self.extraction_count += 1;
        
        // Simulate entity extraction (in real app, this would call an ML model)
        let entities = self.extract_entities(text);
        
        info!(
            "[EntityExtractor] Extracted {} entities from {} chars (total: {})",
            entities.len(),
            text.len(),
            self.extraction_count
        );
        
        // Return entities - macro handles serialization and ctx.send_reply()
        Ok(json!({
            "entities": entities,
            "count": entities.len(),
            "model": self.model_name,
        }))
    }
    
    /// Get extraction statistics (GenServer call - returns stats to caller)
    #[handler("stats")]
    async fn handle_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        info!(
            "[EntityExtractor] Stats requested: model={}, total_extractions={}",
            self.model_name, self.extraction_count
        );
        
        // Return stats - macro handles serialization and ctx.send_reply()
        Ok(json!({
            "model": self.model_name,
            "total_extractions": self.extraction_count,
        }))
    }
}

impl EntityExtractor {
    /// Simple entity extraction (mock implementation)
    fn extract_entities(&self, text: &str) -> Vec<Entity> {
        let mut entities = Vec::new();
        
        // Simple pattern matching for demo (real impl would use NLP/ML)
        let words: Vec<&str> = text.split_whitespace().collect();
        
        for (i, word) in words.iter().enumerate() {
            // Detect capitalized words as potential entities
            if word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                let entity_type = if i == 0 || words.get(i.saturating_sub(1)).map(|w| *w == "Mr." || *w == "Ms." || *w == "Dr.").unwrap_or(false) {
                    "PERSON"
                } else if word.ends_with("Corp") || word.ends_with("Inc") || word.ends_with("LLC") {
                    "ORG"
                } else if ["New", "San", "Los", "Las"].contains(word) {
                    "LOCATION"
                } else {
                    "UNKNOWN"
                };
                
                entities.push(Entity {
                    entity_type: entity_type.to_string(),
                    value: word.to_string(),
                    confidence: 0.85,
                });
            }
        }
        
        entities
    }
}

// =============================================================================
// AuditLogger - GenEvent Actor (Fire-and-Forget Pattern)
// =============================================================================

/// Audit logger using GenEvent pattern for fire-and-forget logging.
/// 
/// ## Annotations
/// - `#[event_actor]` - Fire-and-forget behavior
/// - `#[plexspaces_handlers(event)]` - Generates EventHandler dispatch (cast by default)
/// - `#[handler("event", cast)]` - Cast handlers (no reply)
#[event_actor]
pub struct AuditLogger {
    event_count: u32,
    events: Vec<AuditEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: String,
    pub event_type: String,
    pub actor_id: String,
    pub details: serde_json::Value,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            event_count: 0,
            events: Vec::new(),
        }
    }
}

#[plexspaces_handlers(event)]
impl AuditLogger {
    /// Log document submitted event (fire-and-forget)
    #[handler("document_submitted", cast)]
    async fn handle_document_submitted(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.log_event("document_submitted", &msg.sender_id, &msg.payload);
        Ok(())
    }
    
    /// Log extraction started event
    #[handler("extraction_started", cast)]
    async fn handle_extraction_started(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.log_event("extraction_started", &msg.sender_id, &msg.payload);
        Ok(())
    }
    
    /// Log extraction completed event
    #[handler("extraction_completed", cast)]
    async fn handle_extraction_completed(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.log_event("extraction_completed", &msg.sender_id, &msg.payload);
        Ok(())
    }
    
    /// Log processing failed event
    #[handler("processing_failed", cast)]
    async fn handle_processing_failed(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.log_event("processing_failed", &msg.sender_id, &msg.payload);
        Ok(())
    }
    
    /// Generic event handler
    #[handler("log", cast)]
    async fn handle_log(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .unwrap_or_else(|_| json!({}));
        let event_type = payload.get("event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        self.log_event(event_type, &msg.sender_id, &msg.payload);
        Ok(())
    }
}

impl AuditLogger {
    fn log_event(&mut self, event_type: &str, actor_id: &str, payload: &[u8]) {
        self.event_count += 1;
        
        let details: serde_json::Value = serde_json::from_slice(payload)
            .unwrap_or_else(|_| json!({}));
        
        let event = AuditEvent {
            timestamp: timestamp_now(),
            event_type: event_type.to_string(),
            actor_id: actor_id.to_string(),
            details,
        };
        
        info!(
            "[AuditLogger] #{} {} from {} - {}",
            self.event_count,
            event_type,
            actor_id,
            serde_json::to_string(&event.details).unwrap_or_default()
        );
        
        self.events.push(event);
    }
}

/// Returns current timestamp as ISO 8601 string.
fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    // Format as ISO 8601 (simplified - real apps should use chrono)
    let secs = duration.as_secs();
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // Approximate date (good enough for demo)
    let year = 1970 + (days_since_epoch / 365);
    format!("{}-01-01T{:02}:{:02}:{:02}Z", year, hours, minutes, seconds)
}

// =============================================================================
// Main - Demonstrate All Three Patterns
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Suppress all logs to show clean example output only
    tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_env_filter("entity_recognition=warn,plexspaces=off,plexspaces_node=off,plexspaces_core=off,plexspaces_actor=off")
        .init();

    println!();
    println!("╔════════════════════════════════════════════════════════════════════════╗");
    println!("║                                                                        ║");
    println!("║     📝 ENTITY RECOGNITION - NLP Pipeline Demo                          ║");
    println!("║                                                                        ║");
    println!("║     Real-world use case: Document processing with NER extraction       ║");
    println!("║     Demonstrates: FSM + GenServer + GenEvent actor patterns            ║");
    println!("║                                                                        ║");
    println!("╚════════════════════════════════════════════════════════════════════════╝");
    println!();

    // =========================================================================
    // Infrastructure Setup
    // =========================================================================
    print!("🔧 Initializing PlexSpaces node... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    let node = Arc::new(
        NodeBuilder::new("entity-node")
            .with_clustering_enabled(false)
            .build()
            .await,
    );
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("acme-corp".to_string(), "entity-recognition".to_string());

    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    sleep(Duration::from_millis(300)).await;
    println!("✓");

    // =========================================================================
    // Spawn Actors
    // =========================================================================
    println!();
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  SPAWNING ACTORS                                                       │");
    println!("├────────────────────────────────────────────────────────────────────────┤");

    // 1. Spawn AuditLogger (GenEvent - fire-and-forget)
    print!("│  [1/3] AuditLogger (GenEvent)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let audit_logger = spawn_with_facets(
        &ctx,
        service_locator.clone(),
        ActorId::from("audit-logger@entity-node"),
        "entity-recognition",
        AuditLogger::new(),
        vec![],
    ).await.map_err(|e| format!("spawn AuditLogger: {}", e))?;
    println!("✓ fire-and-forget logging                │");

    // 2. Spawn EntityExtractor (GenServer - request-reply)
    print!("│  [2/3] EntityExtractor (GenServer)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let entity_extractor = spawn_with_facets(
        &ctx,
        service_locator.clone(),
        ActorId::from("entity-extractor@entity-node"),
        "entity-recognition",
        EntityExtractor::new("simple-ner-v1"),
        vec![],
    ).await.map_err(|e| format!("spawn EntityExtractor: {}", e))?;
    println!("✓ request-reply NER            │");

    // 3. Spawn DocumentProcessor (FSM - state machine with TimerFacet)
    print!("│  [3/3] DocumentProcessor (FSM)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let timer_facet = TimerFacet::new(json!({}), 50);
    let doc_processor = spawn_with_facets(
        &ctx,
        service_locator.clone(),
        ActorId::from("doc-processor-001@entity-node"),
        "entity-recognition",
        DocumentProcessor::new("doc-001"),
        vec![Box::new(timer_facet)],
    ).await.map_err(|e| format!("spawn DocumentProcessor: {}", e))?;
    println!("✓ state machine pipeline           │");
    println!("└────────────────────────────────────────────────────────────────────────┘");

    // =========================================================================
    // Pipeline Input
    // =========================================================================
    println!();
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  PIPELINE INPUT                                                        │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    
    let document_content = "John Smith works at Acme Corp in New York. \
        He met with Dr. Jane Doe from TechStart Inc yesterday.";
    
    println!("│  Document: \"{}\"  │", &document_content[..50]);
    println!("│            \"{}\"│", &document_content[50..]);
    println!("│  Length: {} characters                                                │", document_content.len());
    println!("└────────────────────────────────────────────────────────────────────────┘");

    // =========================================================================
    // Run Pipeline
    // =========================================================================
    println!();
    println!("▶ Running entity recognition pipeline...");
    println!();
    
    let pipeline_start = std::time::Instant::now();

    // Step 1: Submit document (FSM: Pending → Processing)
    print!("  [1/6] Submit document (FSM cast)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let submit_msg = cast_message(json!({
        "event": "submit",
        "content": document_content,
    }));
    doc_processor.tell(submit_msg).await?;
    sleep(Duration::from_millis(50)).await;
    
    // Log to audit
    let audit_msg = cast_message(json!({
        "event_type": "document_submitted",
        "doc_id": "doc-001",
        "size": document_content.len(),
    }));
    audit_logger.tell(audit_msg).await?;
    println!("Pending → Processing ✓");

    // Step 2: Start extraction (FSM: Processing → Extracting)
    print!("  [2/6] Start extraction (FSM cast)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let extract_msg = cast_message(json!({
        "event": "extract",
    }));
    doc_processor.tell(extract_msg).await?;
    sleep(Duration::from_millis(50)).await;
    println!("Processing → Extracting ✓");

    // Step 3: Extract entities (GenServer call)
    print!("  [3/6] Extract entities (GenServer ask)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let extraction_request = call_message(json!({
        "action": "extract",
        "text": document_content,
    }));
    
    let mut extracted_entities: Vec<Entity> = vec![];
    match entity_extractor.ask(extraction_request, Duration::from_secs(5)).await {
        Ok(reply) => {
            let result: serde_json::Value = serde_json::from_slice(&reply.payload)
                .unwrap_or_else(|_| json!({}));
            if let Some(entities) = result["entities"].as_array() {
                extracted_entities = entities.iter()
                    .filter_map(|e| serde_json::from_value(e.clone()).ok())
                    .collect();
            }
            println!("{} entities found ✓", extracted_entities.len());
        }
        Err(e) => {
            println!("error: {} ✗", e);
        }
    }
    sleep(Duration::from_millis(50)).await;
    
    // Step 4: Complete processing (FSM: Extracting → Complete)
    print!("  [4/6] Complete processing (FSM cast)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let complete_msg = cast_message(json!({
        "event": "complete",
        "entities": extracted_entities,
    }));
    doc_processor.tell(complete_msg).await?;
    
    // Log completion
    let audit_complete = cast_message(json!({
        "event_type": "extraction_completed",
        "doc_id": "doc-001",
    }));
    audit_logger.tell(audit_complete).await?;
    sleep(Duration::from_millis(50)).await;
    println!("Extracting → Complete ✓");

    // Step 5: Query FSM state (FSM call)
    print!("  [5/6] Query FSM state (FSM ask)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let state_request = call_message(json!({
        "event": "get_state",
    }));
    
    let mut final_state: Option<serde_json::Value> = None;
    match doc_processor.ask(state_request, Duration::from_secs(5)).await {
        Ok(reply) => {
            final_state = serde_json::from_slice(&reply.payload).ok();
            println!("state={} ✓", final_state.as_ref().map(|s| s["state"].as_str().unwrap_or("?")).unwrap_or("?"));
        }
        Err(e) => {
            println!("error: {} ✗", e);
        }
    }
    sleep(Duration::from_millis(50)).await;

    // Step 6: Get extractor stats (GenServer call)
    print!("  [6/6] Get extractor stats (GenServer ask)... ");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
    let stats_request = call_message(json!({
        "action": "stats",
    }));
    
    let mut extractor_stats: Option<serde_json::Value> = None;
    match entity_extractor.ask(stats_request, Duration::from_secs(5)).await {
        Ok(reply) => {
            extractor_stats = serde_json::from_slice(&reply.payload).ok();
            let total = extractor_stats.as_ref().map(|s| s["total_extractions"].as_u64().unwrap_or(0)).unwrap_or(0);
            println!("{} extractions ✓", total);
        }
        Err(e) => {
            println!("error: {} ✗", e);
        }
    }
    
    let pipeline_elapsed = pipeline_start.elapsed();

    // =========================================================================
    // Results
    // =========================================================================
    println!();
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  PIPELINE RESULTS                                                      │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Document ID:      doc-001                                             │");
    println!("│  Final State:      {} ✓                                            │", 
        final_state.as_ref().map(|s| s["state"].as_str().unwrap_or("?")).unwrap_or("?"));
    println!("│  Pipeline Time:    {} ms                                              │", pipeline_elapsed.as_millis());
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Extracted Entities:                                                   │");
    
    // Group entities by type
    let persons: Vec<_> = extracted_entities.iter().filter(|e| e.entity_type == "PERSON").collect();
    let orgs: Vec<_> = extracted_entities.iter().filter(|e| e.entity_type == "ORG").collect();
    let locations: Vec<_> = extracted_entities.iter().filter(|e| e.entity_type == "LOC").collect();
    let others: Vec<_> = extracted_entities.iter().filter(|e| !["PERSON", "ORG", "LOC"].contains(&e.entity_type.as_str())).collect();
    
    if !persons.is_empty() {
        let names: Vec<_> = persons.iter().map(|e| e.value.as_str()).collect();
        println!("│    • PERSON:       {} ({})                                     │", persons.len(), names.join(", "));
    }
    if !orgs.is_empty() {
        let names: Vec<_> = orgs.iter().map(|e| e.value.as_str()).collect();
        println!("│    • ORG:          {} ({})                               │", orgs.len(), names.join(", "));
    }
    if !locations.is_empty() {
        let names: Vec<_> = locations.iter().map(|e| e.value.as_str()).collect();
        println!("│    • LOC:          {} ({})                                        │", locations.len(), names.join(", "));
    }
    if !others.is_empty() {
        println!("│    • OTHER:        {}                                               │", others.len());
    }
    println!("│    • Total:        {} entities                                         │", extracted_entities.len());
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  Extractor Stats:                                                      │");
    if let Some(stats) = &extractor_stats {
        println!("│    • Model:        {}                                       │", stats["model"].as_str().unwrap_or("?"));
        println!("│    • Extractions:  {}                                                  │", stats["total_extractions"].as_u64().unwrap_or(0));
    }
    println!("└────────────────────────────────────────────────────────────────────────┘");

    // =========================================================================
    // SDK Patterns Summary
    // =========================================================================
    println!();
    println!("┌────────────────────────────────────────────────────────────────────────┐");
    println!("│  SDK PATTERNS DEMONSTRATED                                             │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  FSM Actor (DocumentProcessor):                                        │");
    println!("│    • #[fsm_actor] - Finite state machine behavior                      │");
    println!("│    • #[handler(\"event\")] - Cast handlers (fire-and-forget)             │");
    println!("│    • #[handler(\"get_state\", call)] - Call handler (request-reply)      │");
    println!("│    • State transitions: Pending → Processing → Extracting → Complete   │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  GenServer Actor (EntityExtractor):                                    │");
    println!("│    • #[gen_server_actor] - Request-reply behavior (call default)       │");
    println!("│    • ask() + call_message() - Synchronous request-reply                │");
    println!("│    • Returns Result<Value, BehaviorError> - Auto-serialized reply      │");
    println!("├────────────────────────────────────────────────────────────────────────┤");
    println!("│  GenEvent Actor (AuditLogger):                                         │");
    println!("│    • #[event_actor] - Fire-and-forget behavior (cast default)          │");
    println!("│    • tell() + cast_message() - Asynchronous notification               │");
    println!("│    • No reply expected - Best for logging, metrics, events             │");
    println!("└────────────────────────────────────────────────────────────────────────┘");
    println!();

    // Shutdown (silent)
    node.shutdown(Duration::from_secs(2)).await?;
    
    Ok(())
}
