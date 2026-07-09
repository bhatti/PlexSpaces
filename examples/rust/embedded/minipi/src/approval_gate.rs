// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ApprovalGateActor — human-in-the-loop state machine.
//
// Demonstrates: FSM pattern for human-in-the-loop, durable wait.
// FSM states: idle → awaiting_approval → approved / rejected → idle

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, Value, json,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Human-in-the-loop approval gate implemented as a GenServer with FSM semantics.
///
/// Agent calls request_approval → suspends.
/// A human (or policy) calls approve/reject.
/// Agent gets signaled and resumes.
///
/// Key insight: agent can wait for days. DurabilityFacet preserves all state.
#[gen_server_actor(name = "approval_gate")]
pub struct ApprovalGateActor {
    actor_id: String,
    fsm_state: String,
    pending_request: Value,
    pending_agent_id: String,
    decision_history: Vec<Value>,
}

impl ApprovalGateActor {
    pub fn new() -> Self {
        Self {
            actor_id: String::new(),
            fsm_state: "idle".to_string(),
            pending_request: json!({}),
            pending_agent_id: String::new(),
            decision_history: Vec::new(),
        }
    }
}

#[plexspaces_handlers]
impl ApprovalGateActor {
    /// An agent requests human approval for a high-stakes action.
    #[handler("request_approval")]
    async fn request_approval(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);

        if self.fsm_state != "idle" {
            return Ok(json!({
                "status": "busy",
                "message": format!("Approval gate is already {}", self.fsm_state),
                "current_agent": self.pending_agent_id,
            }));
        }

        let agent_id = payload
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let action = payload
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let context = payload.get("context").cloned().unwrap_or(json!({}));

        self.fsm_state = "awaiting_approval".to_string();
        self.pending_agent_id = agent_id.clone();
        self.pending_request = json!({
            "action": action,
            "context": context,
            "requested_at_ms": now_ms(),
        });

        info!(
            "ApprovalGate: request from agent={} action={}",
            agent_id, action
        );

        Ok(json!({
            "status": "pending",
            "message": "Approval request submitted. Agent will be notified on decision.",
            "gate_id": self.actor_id,
        }))
    }

    /// Human approves the pending action.
    #[handler("approve")]
    async fn approve(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);

        if self.fsm_state != "awaiting_approval" {
            return Ok(json!({
                "error": format!("No pending approval request (state={})", self.fsm_state)
            }));
        }

        let approver = payload
            .get("approver")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let comment = payload
            .get("comment")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let agent_id = self.pending_agent_id.clone();

        self.decision_history.push(json!({
            "action": self.pending_request.get("action").cloned().unwrap_or(json!("")),
            "decision": "approved",
            "approver": approver,
            "comment": comment,
            "decided_at_ms": now_ms(),
        }));

        self.fsm_state = "idle".to_string();
        self.pending_agent_id = String::new();
        self.pending_request = json!({});

        info!(
            "ApprovalGate: approved agent={} approver={}",
            agent_id, approver
        );

        Ok(json!({
            "status": "approved",
            "agent_id": agent_id,
            "approver": approver,
        }))
    }

    /// Human rejects the pending action.
    #[handler("reject")]
    async fn reject(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(Value::Null);

        if self.fsm_state != "awaiting_approval" {
            return Ok(json!({
                "error": format!("No pending approval request (state={})", self.fsm_state)
            }));
        }

        let approver = payload
            .get("approver")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let reason = payload
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let agent_id = self.pending_agent_id.clone();

        self.decision_history.push(json!({
            "action": self.pending_request.get("action").cloned().unwrap_or(json!("")),
            "decision": "rejected",
            "approver": approver,
            "reason": reason,
            "decided_at_ms": now_ms(),
        }));

        self.fsm_state = "idle".to_string();
        self.pending_agent_id = String::new();
        self.pending_request = json!({});

        info!(
            "ApprovalGate: rejected agent={} reason={}",
            agent_id, reason
        );

        Ok(json!({
            "status": "rejected",
            "agent_id": agent_id,
            "reason": reason,
        }))
    }

    /// Get the current FSM state and pending request info.
    #[handler("get_status")]
    async fn get_status(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "state": self.fsm_state,
            "pending_agent_id": self.pending_agent_id,
            "pending_request": self.pending_request,
            "decision_count": self.decision_history.len(),
        }))
    }

    /// Return decision history.
    #[handler("get_history")]
    async fn get_history(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({
            "status": "ok",
            "decisions": self.decision_history,
            "count": self.decision_history.len(),
        }))
    }
}
