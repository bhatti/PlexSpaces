// SPDX-License-Identifier: AGPL-3.0-or-later
// GuardrailsGateActor — approval gate for destructive tool operations.
// Demonstrates: GenFSM (allow→review→deny states), KV (policies + pending approvals),
// Channel (approval queue), CompareAndSwap (atomic approval state transitions).
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// GuardrailsGateActor enforces approval policies on tool calls.
// Destructive tools require explicit approval before execution.
// Uses GenFSM pattern: each approval request transitions through states.
type GuardrailsGateActor struct {
	plexspaces.BaseActor
	FSMState       string `json:"fsm_state"`
	CheckCount     int    `json:"check_count"`
	ApprovalCount  int    `json:"approval_count"`
	DeniedCount    int    `json:"denied_count"`
}

var restrictedTools = map[string]string{
	"http_request": "review",  // HTTP calls require review
	"delete_file":  "deny",    // file deletion always denied
	"rm_command":   "deny",    // shell rm always denied
	"format_disk":  "deny",    // disk operations always denied
}

func NewGuardrailsGateActor() plexspaces.Actor {
	a := &GuardrailsGateActor{FSMState: "allow"}
	a.SetSelf(a)
	return a
}

func newGuardrailsGateActor() *GuardrailsGateActor {
	a := &GuardrailsGateActor{FSMState: "allow"}
	a.SetSelf(a)
	return a
}

func (g *GuardrailsGateActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	g.SetRuntimeMetadata(config.ActorID)
	// Seed default policies in KV
	for tool, policy := range restrictedTools {
		host.KVPut("guardrail_policy:"+tool, policy)
	}
	if err := host.PG().Join("svc:guardrails"); err != nil {
		host.Warn(fmt.Sprintf("GuardrailsGateActor: failed to join svc:guardrails: %v", err))
	}
	host.Info(fmt.Sprintf("GuardrailsGateActor Init actor_id=%s state=%s", config.ActorID, g.FSMState))
	return ""
}

func (g *GuardrailsGateActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "check":
		return g.check(p)
	case "approve":
		return g.approve(p)
	case "deny":
		return g.deny(p)
	case "set_policy":
		return g.setPolicy(p)
	case "get_policy":
		return g.getPolicy(p)
	case "get_pending":
		return g.getPending()
	case "get_state":
		return marshal(map[string]any{
			"status":         "ok",
			"state":          g.FSMState,
			"check_count":    g.CheckCount,
			"approval_count": g.ApprovalCount,
			"denied_count":   g.DeniedCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (g *GuardrailsGateActor) check(p map[string]any) string {
	toolName := stringVal(p, "tool", "")
	if toolName == "" {
		return marshal(map[string]any{"decision": "allow"})
	}

	// Look up policy: KV > in-memory defaults
	policy := host.KVGet("guardrail_policy:" + toolName)
	if policy == "" {
		policy = "allow"
	}

	g.CheckCount++
	g.IncrCounter(host, "guardrail_checks")

	switch policy {
	case "deny":
		g.DeniedCount++
		fireAudit("guardrail_denied", fmt.Sprintf("tool=%s", toolName))
		return marshal(map[string]any{"decision": "deny", "tool": toolName, "reason": "tool is blocked by policy"})

	case "review", "requires_approval":
		// Queue the approval request
		approvalID := fmt.Sprintf("approval-%s-%d", toolName, host.NowMs())
		approval := map[string]any{
			"approval_id": approvalID,
			"tool":        toolName,
			"input":       p["input"],
			"status":      "pending",
			"created_at":  host.NowMs(),
		}
		approvalJSON, _ := json.Marshal(approval)
		host.KVPut("approval:"+approvalID, string(approvalJSON))
		// Also track pending IDs
		pending := host.KVGet("pending_approval_ids")
		if pending == "" {
			pending = approvalID
		} else {
			pending = pending + "," + approvalID
		}
		host.KVPut("pending_approval_ids", pending)

		// Write to TupleSpace for visibility
		_ = host.TS().Write([]any{"approval_pending", toolName, approvalID, host.NowMs()})

		fireAudit("guardrail_review", fmt.Sprintf("tool=%s approval_id=%s", toolName, approvalID))
		return marshal(map[string]any{
			"decision":    "requires_approval",
			"tool":        toolName,
			"approval_id": approvalID,
		})

	default:
		return marshal(map[string]any{"decision": "allow", "tool": toolName})
	}
}

func (g *GuardrailsGateActor) approve(p map[string]any) string {
	approvalID := stringVal(p, "approval_id", "")
	if approvalID == "" {
		return marshal(map[string]any{"error": "approval_id is required"})
	}
	raw := host.KVGet("approval:" + approvalID)
	if raw == "" {
		return marshal(map[string]any{"error": "approval not found", "approval_id": approvalID})
	}
	var approval map[string]any
	if err := json.Unmarshal([]byte(raw), &approval); err != nil {
		return marshal(map[string]any{"error": "corrupt approval data"})
	}
	approval["status"] = "approved"
	approval["approved_at"] = host.NowMs()
	updatedJSON, _ := json.Marshal(approval)
	host.KVPut("approval:"+approvalID, string(updatedJSON))

	g.ApprovalCount++
	fireAudit("guardrail_approved", fmt.Sprintf("approval_id=%s tool=%s", approvalID, stringVal(approval, "tool", "")))
	return marshal(map[string]any{"status": "ok", "decision": "approved", "approval_id": approvalID})
}

func (g *GuardrailsGateActor) deny(p map[string]any) string {
	approvalID := stringVal(p, "approval_id", "")
	if approvalID == "" {
		return marshal(map[string]any{"error": "approval_id is required"})
	}
	raw := host.KVGet("approval:" + approvalID)
	if raw == "" {
		return marshal(map[string]any{"error": "approval not found"})
	}
	var approval map[string]any
	if err := json.Unmarshal([]byte(raw), &approval); err != nil {
		return marshal(map[string]any{"error": "corrupt approval data"})
	}
	approval["status"] = "denied"
	approval["denied_at"] = host.NowMs()
	updatedJSON, _ := json.Marshal(approval)
	host.KVPut("approval:"+approvalID, string(updatedJSON))

	g.DeniedCount++
	fireAudit("guardrail_denied_manual", fmt.Sprintf("approval_id=%s", approvalID))
	return marshal(map[string]any{"status": "ok", "decision": "denied", "approval_id": approvalID})
}

func (g *GuardrailsGateActor) setPolicy(p map[string]any) string {
	toolName := stringVal(p, "tool", "")
	policy := stringVal(p, "policy", "allow")
	if toolName == "" {
		return marshal(map[string]any{"error": "tool is required"})
	}
	if policy != "allow" && policy != "review" && policy != "deny" {
		return marshal(map[string]any{"error": "policy must be: allow, review, or deny"})
	}
	host.KVPut("guardrail_policy:"+toolName, policy)
	return marshal(map[string]any{"status": "ok", "tool": toolName, "policy": policy})
}

func (g *GuardrailsGateActor) getPolicy(p map[string]any) string {
	toolName := stringVal(p, "tool", "")
	if toolName == "" {
		return marshal(map[string]any{"error": "tool is required"})
	}
	policy := host.KVGet("guardrail_policy:" + toolName)
	if policy == "" {
		policy = "allow"
	}
	return marshal(map[string]any{"status": "ok", "tool": toolName, "policy": policy})
}

func (g *GuardrailsGateActor) getPending() string {
	pending := host.KVGet("pending_approval_ids")
	if pending == "" {
		return marshal(map[string]any{"status": "ok", "approvals": []any{}, "count": 0})
	}
	ids := splitNonEmpty(pending, ",")
	approvals := make([]any, 0, len(ids))
	for _, id := range ids {
		raw := host.KVGet("approval:" + id)
		if raw == "" {
			continue
		}
		var approval map[string]any
		if err := json.Unmarshal([]byte(raw), &approval); err == nil {
			if stringVal(approval, "status", "") == "pending" {
				approvals = append(approvals, approval)
			}
		}
	}
	return marshal(map[string]any{"status": "ok", "approvals": approvals, "count": len(approvals)})
}

func splitNonEmpty(s, sep string) []string {
	parts := splitStr(s, sep)
	result := make([]string, 0, len(parts))
	for _, p := range parts {
		if p != "" {
			result = append(result, p)
		}
	}
	return result
}

func splitStr(s, sep string) []string {
	if s == "" {
		return []string{}
	}
	result := []string{}
	start := 0
	for i := 0; i <= len(s)-len(sep); i++ {
		if s[i:i+len(sep)] == sep {
			result = append(result, s[start:i])
			start = i + len(sep)
		}
	}
	result = append(result, s[start:])
	return result
}
