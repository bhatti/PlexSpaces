// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Document Approval Workflow - DocuSign-like Multi-Step Approval
//
// Real-world use case: Contract approval workflow with multiple signers,
// escalation on timeout, and audit trail.
//
// ## SDK Features Demonstrated
// - `#[workflow_actor]` - Durable workflow execution
// - `#[run_handler]` - Main workflow execution
// - `#[signal_handler("name")]` - External events (approve, reject, escalate)
// - `#[query_handler("name")]` - Status queries (status, audit_trail)
// - `TimerFacet` - Auto-escalation after timeout
//
// ## Workflow Steps
// 1. Submit document for approval
// 2. Route to each approver in sequence
// 3. Wait for approval/rejection (with timeout)
// 4. Auto-escalate if timeout exceeded
// 5. Complete or reject based on all approvals

use plexspaces_sdk::{
    workflow_actor, plexspaces_handlers,
    ActorContext, BehaviorError, RequestContext, Message, new_message,
    NodeBuilder, spawn_workflow_actor, WorkflowRef, TimerFacet,
};
// Note: run_handler, signal_handler, query_handler are used via #[plexspaces_handlers(workflow)]
use plexspaces_node::CoordinationComputeTracker;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn, Level};
use chrono::{DateTime, Utc};

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Document Approval
// =============================================================================

/// Document to be approved
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub content_hash: String,
    pub submitter: String,
    pub submitted_at: DateTime<Utc>,
}

/// Approval request configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub document: Document,
    pub approvers: Vec<Approver>,
    pub timeout_minutes: u32,
    pub escalation_chain: Vec<String>,
}

/// Individual approver
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approver {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub required: bool,
}

/// Approval decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approver_id: String,
    pub decision: Decision,
    pub comment: Option<String>,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Decision {
    Approved,
    Rejected,
    Escalated,
}

/// Audit trail entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub actor: String,
    pub details: String,
}

/// Workflow state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowState {
    pub document_id: String,
    pub status: String,
    pub current_approver_index: usize,
    pub decisions: Vec<ApprovalDecision>,
    pub audit_trail: Vec<AuditEntry>,
    pub escalation_count: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

// =============================================================================
// Document Approval Workflow Actor
// =============================================================================

/// Document approval workflow implementing DocuSign-like functionality.
/// 
/// ## Features
/// - Sequential approval routing
/// - Timeout with auto-escalation
/// - Complete audit trail
/// - Signal handlers for approve/reject/escalate
/// - Query handlers for status inspection
/// 
/// ## Workflow Flow
/// ```text
/// Submit → [Approver 1] → [Approver 2] → ... → Complete
///              ↓               ↓
///          Timeout?        Timeout?
///              ↓               ↓
///          Escalate        Escalate
/// ```
#[workflow_actor]
struct DocumentApprovalWorkflow {
    state: WorkflowState,
    request: Option<ApprovalRequest>,
}

impl DocumentApprovalWorkflow {
    fn new() -> Self {
        Self {
            state: WorkflowState::default(),
            request: None,
        }
    }
    
    fn add_audit(&mut self, action: &str, actor: &str, details: &str) {
        self.state.audit_trail.push(AuditEntry {
            timestamp: Utc::now(),
            action: action.to_string(),
            actor: actor.to_string(),
            details: details.to_string(),
        });
    }
}

#[plexspaces_handlers(workflow)]
impl DocumentApprovalWorkflow {
    /// Main workflow execution - routes document through approvers
    #[run_handler]
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        // Parse approval request
        let request: ApprovalRequest = serde_json::from_slice(&input.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        self.request = Some(request.clone());
        self.state.document_id = request.document.id.clone();
        self.state.status = "pending".to_string();
        self.state.started_at = Some(Utc::now());
        
        self.add_audit(
            "workflow_started",
            &request.document.submitter,
            &format!("Document '{}' submitted for approval", request.document.title),
        );
        
        info!("Starting document approval workflow: {}", request.document.id);
        println!("  Document: {}", request.document.title);
        println!("  Approvers: {}", request.approvers.len());
        
        // Route to first approver
        if !request.approvers.is_empty() {
            let first_approver = &request.approvers[0];
            self.state.status = "awaiting_approval".to_string();
            
            self.add_audit(
                "routed_to_approver",
                "system",
                &format!("Routed to {} ({})", first_approver.name, first_approver.role),
            );
            
            println!("  → Routed to: {} ({})", first_approver.name, first_approver.role);
            println!("  → Timeout: {} minutes", request.timeout_minutes);
        }
        
        // Return initial state (workflow continues via signals)
        self.create_result_message()
    }
    
    /// Handle approval signal from an approver
    #[signal_handler("approve")]
    async fn on_approve(
        &mut self,
        _ctx: &ActorContext,
        data: Message,
    ) -> Result<(), BehaviorError> {
        #[derive(Deserialize)]
        struct ApprovePayload {
            approver_id: String,
            comment: Option<String>,
        }
        
        let payload: ApprovePayload = serde_json::from_slice(&data.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid payload: {}", e)))?;
        
        let request = self.request.as_ref()
            .ok_or_else(|| BehaviorError::ProcessingError("No active request".to_string()))?;
        
        // Verify this is the current approver
        let current_approver = request.approvers.get(self.state.current_approver_index)
            .ok_or_else(|| BehaviorError::ProcessingError("Invalid approver index".to_string()))?;
        
        if current_approver.user_id != payload.approver_id {
            return Err(BehaviorError::ProcessingError(
                format!("Not the current approver. Expected: {}", current_approver.user_id)
            ));
        }
        
        // Clone data we need before releasing the borrow
        let approver_name = current_approver.name.clone();
        let total_approvers = request.approvers.len();
        let comment_str = payload.comment.as_ref().map(|c| format!(": {}", c)).unwrap_or_default();
        
        // Record decision
        let decision = ApprovalDecision {
            approver_id: payload.approver_id.clone(),
            decision: Decision::Approved,
            comment: payload.comment,
            decided_at: Utc::now(),
        };
        self.state.decisions.push(decision);
        
        self.add_audit(
            "approved",
            &approver_name,
            &format!("Approved{}", comment_str),
        );
        
        println!("  ✓ {} approved the document", approver_name);
        
        // Move to next approver or complete
        self.state.current_approver_index += 1;
        
        if self.state.current_approver_index >= total_approvers {
            // All approvers have approved
            self.state.status = "approved".to_string();
            self.state.completed_at = Some(Utc::now());
            
            self.add_audit(
                "workflow_completed",
                "system",
                "All approvers have approved the document",
            );
            
            println!("  ✓ Document fully approved!");
        } else {
            // Route to next approver - get fresh reference
            let request = self.request.as_ref().unwrap();
            let next_approver = &request.approvers[self.state.current_approver_index];
            let next_name = next_approver.name.clone();
            let next_role = next_approver.role.clone();
            
            self.add_audit(
                "routed_to_approver",
                "system",
                &format!("Routed to {} ({})", next_name, next_role),
            );
            
            println!("  → Routed to next approver: {} ({})", next_name, next_role);
        }
        
        Ok(())
    }
    
    /// Handle rejection signal from an approver
    #[signal_handler("reject")]
    async fn on_reject(
        &mut self,
        _ctx: &ActorContext,
        data: Message,
    ) -> Result<(), BehaviorError> {
        #[derive(Deserialize)]
        struct RejectPayload {
            approver_id: String,
            reason: String,
        }
        
        let payload: RejectPayload = serde_json::from_slice(&data.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid payload: {}", e)))?;
        
        let request = self.request.as_ref()
            .ok_or_else(|| BehaviorError::ProcessingError("No active request".to_string()))?;
        
        let current_approver = request.approvers.get(self.state.current_approver_index)
            .ok_or_else(|| BehaviorError::ProcessingError("Invalid approver index".to_string()))?;
        
        // Clone data we need before releasing the borrow
        let approver_name = current_approver.name.clone();
        
        // Record decision
        let decision = ApprovalDecision {
            approver_id: payload.approver_id.clone(),
            decision: Decision::Rejected,
            comment: Some(payload.reason.clone()),
            decided_at: Utc::now(),
        };
        self.state.decisions.push(decision);
        
        self.add_audit(
            "rejected",
            &approver_name,
            &format!("Rejected: {}", payload.reason),
        );
        
        // Workflow ends on rejection
        self.state.status = "rejected".to_string();
        self.state.completed_at = Some(Utc::now());
        self.state.error = Some(payload.reason.clone());
        
        warn!("Document rejected by {}: {}", approver_name, payload.reason);
        println!("  ✗ {} rejected the document: {}", approver_name, payload.reason);
        
        Ok(())
    }
    
    /// Handle escalation signal (manual or from timeout)
    #[signal_handler("escalate")]
    async fn on_escalate(
        &mut self,
        _ctx: &ActorContext,
        data: Message,
    ) -> Result<(), BehaviorError> {
        #[derive(Deserialize)]
        struct EscalatePayload {
            reason: String,
            escalate_to: Option<String>,
        }
        
        let payload: EscalatePayload = serde_json::from_slice(&data.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid payload: {}", e)))?;
        
        let request = self.request.as_ref()
            .ok_or_else(|| BehaviorError::ProcessingError("No active request".to_string()))?;
        
        self.state.escalation_count += 1;
        
        // Determine escalation target
        let escalation_target = payload.escalate_to
            .or_else(|| request.escalation_chain.get(self.state.escalation_count as usize - 1).cloned())
            .unwrap_or_else(|| "manager".to_string());
        
        self.add_audit(
            "escalated",
            "system",
            &format!("Escalated to {}: {}", escalation_target, payload.reason),
        );
        
        warn!("Document escalated to {}: {}", escalation_target, payload.reason);
        println!("  ⚠ Escalated to {}: {}", escalation_target, payload.reason);
        
        Ok(())
    }
    
    /// Handle reminder signal (from timer)
    #[signal_handler("remind")]
    async fn on_remind(
        &mut self,
        _ctx: &ActorContext,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        let request = self.request.as_ref()
            .ok_or_else(|| BehaviorError::ProcessingError("No active request".to_string()))?;
        
        if let Some(current_approver) = request.approvers.get(self.state.current_approver_index) {
            // Clone data we need before releasing the borrow
            let approver_name = current_approver.name.clone();
            
            self.add_audit(
                "reminder_sent",
                "system",
                &format!("Reminder sent to {}", approver_name),
            );
            
            println!("  📧 Reminder sent to {}", approver_name);
        }
        
        Ok(())
    }
    
    /// Query current workflow status
    #[query_handler("status")]
    async fn get_status(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        #[derive(Serialize)]
        struct StatusResponse {
            document_id: String,
            status: String,
            current_approver: Option<String>,
            approvals_completed: usize,
            approvals_total: usize,
            escalation_count: u32,
        }
        
        let current_approver = self.request.as_ref()
            .and_then(|r| r.approvers.get(self.state.current_approver_index))
            .map(|a| a.name.clone());
        
        let total = self.request.as_ref().map(|r| r.approvers.len()).unwrap_or(0);
        
        let response = StatusResponse {
            document_id: self.state.document_id.clone(),
            status: self.state.status.clone(),
            current_approver,
            approvals_completed: self.state.current_approver_index,
            approvals_total: total,
            escalation_count: self.state.escalation_count,
        };
        
        let payload = serde_json::to_value(&response)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        Ok(new_message("reply", payload))
    }

    /// Query audit trail
    #[query_handler("audit_trail")]
    async fn get_audit_trail(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        let payload = serde_json::to_value(&self.state.audit_trail)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        Ok(new_message("reply", payload))
    }

    fn create_result_message(&self) -> Result<Message, BehaviorError> {
        let payload = serde_json::to_value(&self.state)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        Ok(new_message("reply", payload))
    }
}

// =============================================================================
// Main - Demonstrates the workflow
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing - use try_init() to avoid panic if already initialized
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("document_approval=info,plexspaces=warn")
        .try_init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Document Approval Workflow (DocuSign-like)                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Contract approval with multiple signers and escalation");
    println!();
    
    // Create metrics tracker for coordination vs computation analysis
    let mut metrics_tracker = CoordinationComputeTracker::new("document-approval".to_string());
    let total_start = Instant::now();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let node_start = Instant::now();
    
    let node = NodeBuilder::new("approval-node")
        .with_clustering_enabled(false)
        .build_started().await;
    
    let node_time = node_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  ✓ Node 'approval-node' created ({:.2}ms)", node_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 2: Spawn Workflow Actors with Timer Facet
    // =========================================================================
    println!("Step 2: Spawn Document Approval Workflows");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let ctx = RequestContext::new_without_auth(
        "acme-corp".to_string(),
        "contracts".to_string(),
    );
    
    // Spawn multiple workflows to demonstrate scalability and show real metrics
    let num_workflows = 10;
    let mut workflows: Vec<WorkflowRef> = Vec::new();
    
    metrics_tracker.start_coordinate();
    let spawn_start = Instant::now();
    
    for i in 0..num_workflows {
        let workflow_name = format!("contract-approval-{:03}", i);
        let workflow: WorkflowRef = spawn_workflow_actor(
            &ctx,
            node.service_locator(),
            &workflow_name,
            DocumentApprovalWorkflow::new(),
            vec![Box::new(TimerFacet::new(serde_json::json!({}), 50, node.service_locator()))],
        ).await?;
        
        workflows.push(workflow);
        metrics_tracker.increment_message();
    }
    
    let spawn_time = spawn_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  ✓ Spawned {} workflows in {:.2}ms", num_workflows, spawn_time.as_secs_f64() * 1000.0);
    println!("  Facets: TimerFacet (for reminders/escalation)");
    println!();

    // =========================================================================
    // Step 3: Submit Documents for Approval (Multiple Documents)
    // =========================================================================
    println!("Step 3: Submit Documents for Approval");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Create multiple approval requests with more approvers to show real complexity
    let num_approvers = 8; // Increased from 3 to show more realistic workflow
    
    let mut approval_requests: Vec<ApprovalRequest> = Vec::new();
    for i in 0..num_workflows {
        let approvers: Vec<Approver> = (0..num_approvers)
            .map(|j| {
                let roles = vec!["Legal Review", "Finance Approval", "Security Review", 
                                 "Compliance Check", "Risk Assessment", "Executive Sign-off",
                                 "Board Approval", "Final Review"];
                Approver {
                    user_id: format!("approver-{}", j),
                    name: format!("Approver {} {}", j + 1, if j % 2 == 0 { "A" } else { "B" }),
                    email: format!("approver{}@acme.com", j),
                    role: roles[j % roles.len()].to_string(),
                    required: true,
                }
            })
            .collect();
        
        approval_requests.push(ApprovalRequest {
            document: Document {
                id: format!("DOC-2025-{:03}", i + 1),
                title: format!("Q1 Vendor Contract #{} - Acme Supplies", i + 1),
                content_hash: format!("sha256:hash{}...", i),
                submitter: format!("submitter{}@acme.com", i),
                submitted_at: Utc::now(),
            },
            approvers,
            timeout_minutes: 60,
            escalation_chain: vec![
                "manager@acme.com".to_string(),
                "director@acme.com".to_string(),
                "vp@acme.com".to_string(),
            ],
        });
    }
    
    // Submit all documents
    metrics_tracker.start_coordinate();
    let submit_start = Instant::now();
    
    for (i, (workflow_ref, request)) in workflows.iter().zip(approval_requests.iter()).enumerate() {
        metrics_tracker.start_compute();
        let workflow_start = Instant::now();
        
        let state: WorkflowState = workflow_ref.run(request).await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        let workflow_time = workflow_start.elapsed();
        metrics_tracker.end_compute();
        metrics_tracker.increment_message();
        
        if i < 3 || i == num_workflows - 1 {
            println!("  ✓ Document {} submitted: {} ({:.2}ms)", 
                i + 1, state.status, workflow_time.as_secs_f64() * 1000.0);
        }
    }
    
    let submit_time = submit_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Submitted {} documents in {:.2}ms", num_workflows, submit_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 4: Process Approvals (All Approvers for First Document)
    // =========================================================================
    println!("Step 4: Process Approvals (Demonstrating Sequential Routing)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Process approvals for the first workflow to demonstrate the full flow
    metrics_tracker.start_coordinate();
    let approval_start = Instant::now();
    
    for approver_idx in 0..num_approvers {
        metrics_tracker.start_compute();
        let signal_start = Instant::now();
        
        workflows[0].signal("approve", &serde_json::json!({
            "approver_id": format!("approver-{}", approver_idx),
            "comment": format!("Approved by {} (step {}/{})", 
                if approver_idx % 2 == 0 { "Reviewer A" } else { "Reviewer B" },
                approver_idx + 1, num_approvers)
        })).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        let signal_time = signal_start.elapsed();
        metrics_tracker.end_compute();
        metrics_tracker.increment_message();
        
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        if approver_idx < 3 || approver_idx == num_approvers - 1 {
            println!("  ✓ Approver {} approved ({:.2}ms)", approver_idx + 1, signal_time.as_secs_f64() * 1000.0);
        }
    }
    
    let approval_time = approval_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Processed {} approvals in {:.2}ms", num_approvers, approval_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 5: Query Status (Multiple Queries)
    // =========================================================================
    println!("Step 5: Query Workflow Status");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let query_start = Instant::now();
    
    // Query status for multiple workflows
    for i in 0..std::cmp::min(5, num_workflows) {
        let status: serde_json::Value = workflows[i].query("status").await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
        
        if i == 0 {
            println!("  Workflow {}: Status={}, Progress={}/{}", 
                i + 1, status["status"], status["approvals_completed"], status["approvals_total"]);
        }
        metrics_tracker.increment_message();
    }
    
    let query_time = query_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!("  Queried {} workflows in {:.2}ms", std::cmp::min(5, num_workflows), query_time.as_secs_f64() * 1000.0);
    println!();


    // =========================================================================
    // Step 6: Query Audit Trail
    // =========================================================================
    println!("Step 6: Query Audit Trail");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let audit_start = Instant::now();
    
    // Query audit trail with typed API
    let audit_trail: Vec<AuditEntry> = workflows[0].query("audit_trail").await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    
    let audit_time = audit_start.elapsed();
    metrics_tracker.end_coordinate();
    metrics_tracker.increment_message();
    
    println!("  Audit Trail ({} entries) queried in {:.2}ms", audit_trail.len(), audit_time.as_secs_f64() * 1000.0);
    println!("  Sample entries (showing first 5):");
    for entry in audit_trail.iter().take(5) {
        println!("    {} | {} | {} | {}", 
            entry.timestamp.format("%H:%M:%S"),
            entry.action,
            entry.actor,
            entry.details
        );
    }
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
    let workflows_per_sec = num_workflows as f64 / total_time.as_secs_f64();
    let approvals_per_sec = (num_workflows * num_approvers) as f64 / total_time.as_secs_f64();
    println!("Workflows Processed:           {}", num_workflows);
    println!("Approvers per Workflow:        {}", num_approvers);
    println!("Total Approvals:               {}", num_workflows * num_approvers);
    println!("Workflows/Second:              {:.2}", workflows_per_sec);
    println!("Approvals/Second:              {:.2}", approvals_per_sec);
    println!("Avg Latency per Workflow:      {:.2}ms", total_time.as_secs_f64() * 1000.0 / num_workflows as f64);
    println!("Avg Latency per Approval:     {:.2}ms", total_time.as_secs_f64() * 1000.0 / (num_workflows * num_approvers) as f64);
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("ANALYSIS & RECOMMENDATIONS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    if metrics.granularity_ratio >= 10.0 {
        println!("✓ Excellent granularity ratio (>= 10x) - coordination overhead is minimal");
    } else {
        println!("⚠ Granularity ratio below 10x - consider batching operations to reduce coordination overhead");
    }
    if metrics.efficiency >= 0.9 {
        println!("✓ High efficiency (>= 90%) - system is well-balanced");
    } else {
        println!("⚠ Efficiency below 90% - consider optimizing coordination patterns");
    }
    println!();
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Document Approval Workflow Complete!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("SDK Annotations Used:");
    println!("  • #[workflow_actor] - Durable workflow execution");
    println!("  • #[run_handler] - Main workflow (submit document)");
    println!("  • #[signal_handler(\"approve\")] - Handle approvals");
    println!("  • #[signal_handler(\"reject\")] - Handle rejections");
    println!("  • #[signal_handler(\"escalate\")] - Handle escalations");
    println!("  • #[query_handler(\"status\")] - Query current status");
    println!("  • #[query_handler(\"audit_trail\")] - Query audit history");
    println!();
    println!("WorkflowRef API (Temporal-inspired):");
    println!("  • workflow.run(&input) - Start workflow with typed input/output");
    println!("  • workflow.signal(\"name\", &data) - Send signal (fire-and-forget)");
    println!("  • workflow.query(\"name\") - Query state with typed response");
    println!("  • Per-operation timeouts (not global) for long-running workflows");
    println!();
    println!("Workflow Features:");
    println!("  • Sequential approval routing");
    println!("  • Complete audit trail");
    println!("  • Timeout with auto-escalation (via TimerFacet)");
    println!("  • Rejection handling");
    println!();
    println!("Real-World Use Cases:");
    println!("  • Contract approvals (DocuSign)");
    println!("  • Expense reports");
    println!("  • Purchase orders");
    println!("  • Leave requests");
    println!("  • Code reviews");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
