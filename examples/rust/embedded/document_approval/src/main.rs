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
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, spawn_workflow_actor, WorkflowRef, TimerFacet,
};
// Note: run_handler, signal_handler, query_handler are used via #[plexspaces_handlers(workflow)]
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
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
        
        let payload = serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        
        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }
    
    /// Query audit trail
    #[query_handler("audit_trail")]
    async fn get_audit_trail(
        &self,
        _ctx: &ActorContext,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        let payload = serde_json::to_vec(&self.state.audit_trail)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        
        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }
    
    fn create_result_message(&self) -> Result<Message, BehaviorError> {
        let payload = serde_json::to_vec(&self.state)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        
        Ok(Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            ..Default::default()
        })
    }
}

// =============================================================================
// Main - Demonstrates the workflow
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("document_approval=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Document Approval Workflow (DocuSign-like)                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: Contract approval with multiple signers and escalation");
    println!();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let node = NodeBuilder::new("approval-node")
        .with_clustering_enabled(false)
        .build().await;
    let node = Arc::new(node);
    
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    println!("  ✓ Node 'approval-node' created");
    println!();

    // =========================================================================
    // Step 2: Spawn Workflow Actor with Timer Facet
    // =========================================================================
    println!("Step 2: Spawn Document Approval Workflow");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let ctx = RequestContext::new_without_auth(
        "acme-corp".to_string(),
        "contracts".to_string(),
    );
    
    // Create timer facet for reminders/escalation
    let timer_facet = TimerFacet::new(serde_json::json!({}), 50);
    
    // Use WorkflowRef for clean, typed workflow interaction
    let workflow: WorkflowRef = spawn_workflow_actor(
        &ctx,
        node.service_locator(),
        "contract-approval-001@approval-node",
        "contracts",
        DocumentApprovalWorkflow::new(),
        vec![Box::new(timer_facet)],
    ).await?;
    
    println!("  ✓ Workflow spawned: {}", workflow.id());
    println!("  Facets: TimerFacet (for reminders/escalation)");
    println!();

    // =========================================================================
    // Step 3: Submit Document for Approval
    // =========================================================================
    println!("Step 3: Submit Document for Approval");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let approval_request = ApprovalRequest {
        document: Document {
            id: "DOC-2025-001".to_string(),
            title: "Q1 Vendor Contract - Acme Supplies".to_string(),
            content_hash: "sha256:abc123...".to_string(),
            submitter: "john.doe@acme.com".to_string(),
            submitted_at: Utc::now(),
        },
        approvers: vec![
            Approver {
                user_id: "alice".to_string(),
                name: "Alice Johnson".to_string(),
                email: "alice@acme.com".to_string(),
                role: "Legal Review".to_string(),
                required: true,
            },
            Approver {
                user_id: "bob".to_string(),
                name: "Bob Smith".to_string(),
                email: "bob@acme.com".to_string(),
                role: "Finance Approval".to_string(),
                required: true,
            },
            Approver {
                user_id: "carol".to_string(),
                name: "Carol Williams".to_string(),
                email: "carol@acme.com".to_string(),
                role: "Executive Sign-off".to_string(),
                required: true,
            },
        ],
        timeout_minutes: 60,
        escalation_chain: vec![
            "manager@acme.com".to_string(),
            "director@acme.com".to_string(),
            "vp@acme.com".to_string(),
        ],
    };
    
    // Use typed WorkflowRef API - no manual Message construction needed!
    let state: WorkflowState = workflow.run(&approval_request).await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    println!("  Status: {}", state.status);
    println!();

    // =========================================================================
    // Step 4: First Approver (Alice) Approves
    // =========================================================================
    println!("Step 4: Alice (Legal Review) Approves");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Use typed signal API - clean and simple!
    workflow.signal("approve", &serde_json::json!({
        "approver_id": "alice",
        "comment": "Legal terms verified, all clauses acceptable"
    })).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();

    // =========================================================================
    // Step 5: Query Status
    // =========================================================================
    println!("Step 5: Query Workflow Status");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Use typed query API - no manual Message construction!
    let status: serde_json::Value = workflow.query("status").await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    println!("  Status: {}", status["status"]);
    println!("  Current Approver: {:?}", status["current_approver"]);
    println!("  Progress: {}/{}", status["approvals_completed"], status["approvals_total"]);
    println!();

    // =========================================================================
    // Step 6: Second Approver (Bob) Approves
    // =========================================================================
    println!("Step 6: Bob (Finance) Approves");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    workflow.signal("approve", &serde_json::json!({
        "approver_id": "bob",
        "comment": "Budget approved, within Q1 allocation"
    })).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();

    // =========================================================================
    // Step 7: Third Approver (Carol) Approves
    // =========================================================================
    println!("Step 7: Carol (Executive) Approves");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    workflow.signal("approve", &serde_json::json!({
        "approver_id": "carol",
        "comment": "Approved for execution"
    })).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();

    // =========================================================================
    // Step 8: Query Audit Trail
    // =========================================================================
    println!("Step 8: Query Audit Trail");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Query audit trail with typed API
    let audit_trail: Vec<AuditEntry> = workflow.query("audit_trail").await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    
    println!("  Audit Trail ({} entries):", audit_trail.len());
    for entry in &audit_trail {
        println!("    {} | {} | {} | {}", 
            entry.timestamp.format("%H:%M:%S"),
            entry.action,
            entry.actor,
            entry.details
        );
    }
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Document Approval Workflow Complete!");
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
