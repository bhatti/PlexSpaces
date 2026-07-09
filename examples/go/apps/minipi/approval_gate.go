// SPDX-License-Identifier: AGPL-3.0-or-later
// ApprovalGateActor — human-in-the-loop state machine.
//
// Demonstrates: GenFSM for human-in-the-loop, durable wait (agent can be
// suspended for days), and PlexSpaces's unique capability to resume exactly
// where it left off without losing state.
//
// FSM states: idle → awaiting_approval → approved / rejected → idle
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ApprovalGateActor manages human-in-the-loop approval for high-stakes agent actions.
//
// Agent calls: ask("approval_gate", "request_approval", {...})
// Then the agent suspends.
// A human (or automated policy) calls "approve" or "reject".
// The agent gets signaled and resumes.
//
// Key insight: the agent can wait for days. The DurabilityFacet preserves
// all state durably — no polling, no timeouts burning tokens.
type ApprovalGateActor struct {
	plexspaces.BaseActor
	ActorID         string         `json:"actor_id"`
	FSMState        string         `json:"fsm_state"`
	PendingRequest  map[string]any `json:"pending_request"`
	PendingAgentID  string         `json:"pending_agent_id"`
	DecisionHistory []map[string]any `json:"decision_history"`
}

func NewApprovalGateActor() plexspaces.Actor {
	a := &ApprovalGateActor{
		FSMState:        "idle",
		DecisionHistory: []map[string]any{},
		PendingRequest:  map[string]any{},
	}
	a.SetSelf(a)
	return a
}

func (ag *ApprovalGateActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	ag.SetRuntimeMetadata(config.ActorID)
	ag.ActorID = config.ActorID
	if ag.FSMState == "" {
		ag.FSMState = "idle"
	}
	if ag.DecisionHistory == nil {
		ag.DecisionHistory = []map[string]any{}
	}
	if ag.PendingRequest == nil {
		ag.PendingRequest = map[string]any{}
	}
	if err := host.PG().Join("svc:approval_gate"); err != nil {
		host.Warn(fmt.Sprintf("ApprovalGateActor: failed to join svc:approval_gate: %v", err))
	}
	host.Info(fmt.Sprintf("ApprovalGateActor Init actor_id=%s state=%s", config.ActorID, ag.FSMState))
	return ""
}

func (ag *ApprovalGateActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "request_approval":
		return ag.requestApproval(p)
	case "approve":
		return ag.approve(p)
	case "reject":
		return ag.reject(p)
	case "get_status":
		return ag.getStatus()
	case "get_history":
		return ag.getHistory()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (ag *ApprovalGateActor) requestApproval(p map[string]any) string {
	if ag.FSMState != "idle" {
		return marshal(map[string]any{
			"status":        "busy",
			"message":       fmt.Sprintf("Approval gate is already %s", ag.FSMState),
			"current_agent": ag.PendingAgentID,
		})
	}

	agentID := stringVal(p, "agent_id", "")
	action := stringVal(p, "action", "")
	context, _ := p["context"].(map[string]any)
	if context == nil {
		context = map[string]any{}
	}

	ag.FSMState = "awaiting_approval"
	ag.PendingAgentID = agentID
	ag.PendingRequest = map[string]any{
		"action":            action,
		"context":           context,
		"requested_at_ms":   host.NowMs(),
	}

	host.Info(fmt.Sprintf("ApprovalGate: request from agent=%s action=%s", agentID, action))
	ag.IncrCounter(host, "approval_requests_total")

	// Store request for external review
	reqJSON, _ := json.Marshal(map[string]any{
		"action":          action,
		"context":         context,
		"requested_at_ms": host.NowMs(),
		"agent_id":        agentID,
	})
	host.KVPut("approval_request:"+ag.ActorID, string(reqJSON))

	return marshal(map[string]any{
		"status":  "pending",
		"message": "Approval request submitted. Agent will be notified on decision.",
		"gate_id": ag.ActorID,
	})
}

func (ag *ApprovalGateActor) approve(p map[string]any) string {
	if ag.FSMState != "awaiting_approval" {
		return marshal(map[string]any{
			"error": fmt.Sprintf("No pending approval request (state=%s)", ag.FSMState),
		})
	}

	agentID := ag.PendingAgentID
	approver := stringVal(p, "approver", "")
	comment := stringVal(p, "comment", "")

	ag.FSMState = "approved"

	if ag.DecisionHistory == nil {
		ag.DecisionHistory = []map[string]any{}
	}
	ag.DecisionHistory = append(ag.DecisionHistory, map[string]any{
		"action":        stringVal(ag.PendingRequest, "action", ""),
		"decision":      "approved",
		"approver":      approver,
		"comment":       comment,
		"decided_at_ms": host.NowMs(),
	})

	// Signal the suspended agent to resume
	if agentID != "" {
		if result := host.Send(agentID, "workflow_signal:resume", map[string]any{
			"decision": "approved",
			"approver": approver,
			"comment":  comment,
		}); result != "" {
			host.Warn(fmt.Sprintf("ApprovalGate: failed to signal agent %s: %s", agentID, result))
		}
	}

	ag.FSMState = "idle"
	ag.PendingAgentID = ""
	ag.PendingRequest = map[string]any{}

	ag.IncrCounter(host, "approvals_granted_total")
	host.Info(fmt.Sprintf("ApprovalGate: approved agent=%s approver=%s", agentID, approver))

	return marshal(map[string]any{
		"status":   "approved",
		"agent_id": agentID,
		"approver": approver,
	})
}

func (ag *ApprovalGateActor) reject(p map[string]any) string {
	if ag.FSMState != "awaiting_approval" {
		return marshal(map[string]any{
			"error": fmt.Sprintf("No pending approval request (state=%s)", ag.FSMState),
		})
	}

	agentID := ag.PendingAgentID
	approver := stringVal(p, "approver", "")
	reason := stringVal(p, "reason", "")

	ag.FSMState = "rejected"

	if ag.DecisionHistory == nil {
		ag.DecisionHistory = []map[string]any{}
	}
	ag.DecisionHistory = append(ag.DecisionHistory, map[string]any{
		"action":        stringVal(ag.PendingRequest, "action", ""),
		"decision":      "rejected",
		"approver":      approver,
		"reason":        reason,
		"decided_at_ms": host.NowMs(),
	})

	// Signal agent with rejection
	if agentID != "" {
		if result := host.Send(agentID, "workflow_signal:resume", map[string]any{
			"decision": "rejected",
			"approver": approver,
			"reason":   reason,
		}); result != "" {
			host.Warn(fmt.Sprintf("ApprovalGate: failed to signal agent %s with rejection: %s", agentID, result))
		}
	}

	ag.FSMState = "idle"
	ag.PendingAgentID = ""
	ag.PendingRequest = map[string]any{}

	ag.IncrCounter(host, "approvals_rejected_total")
	host.Info(fmt.Sprintf("ApprovalGate: rejected agent=%s reason=%s", agentID, reason))

	return marshal(map[string]any{
		"status":   "rejected",
		"agent_id": agentID,
		"reason":   reason,
	})
}

func (ag *ApprovalGateActor) getStatus() string {
	return marshal(map[string]any{
		"status":           "ok",
		"state":            ag.FSMState,
		"pending_agent_id": ag.PendingAgentID,
		"pending_request":  ag.PendingRequest,
		"decision_count":   len(ag.DecisionHistory),
	})
}

func (ag *ApprovalGateActor) getHistory() string {
	if ag.DecisionHistory == nil {
		ag.DecisionHistory = []map[string]any{}
	}
	return marshal(map[string]any{
		"status":    "ok",
		"decisions": ag.DecisionHistory,
		"count":     len(ag.DecisionHistory),
	})
}
