# SPDX-License-Identifier: AGPL-3.0-or-later
"""ApprovalGateActor — human-in-the-loop state machine.

Demonstrates: GenFSM for human-in-the-loop, durable wait (agent can be
suspended for days), and PlexSpaces's unique capability to resume exactly
where it left off without losing state.

FSM states: idle → awaiting_approval → approved / rejected → idle
"""

import json
from plexspaces import fsm_actor, state, init_handler, handler, host


@fsm_actor(states=["idle", "awaiting_approval", "approved", "rejected"], initial="idle")
class ApprovalGateActor:
    """
    Human-in-the-loop approval gate.

    Agent calls: host.ask("approval_gate", "request_approval", {...})
    Then the agent suspends (self.suspend()).
    A human (or automated policy) calls "approve" or "reject".
    The agent gets signaled and resumes.

    Key insight: the agent can wait for days. The DurabilityFacet preserves
    all state durably — no polling, no timeouts burning tokens.
    """

    actor_id: str = state(default="")
    fsm_state: str = state(default="idle")
    pending_request: dict = state(default_factory=dict)
    pending_agent_id: str = state(default="")
    decision_history: list = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv.put("svc:approval_gate", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="approval_gate")
        except Exception:
            pass
        host.info(f"ApprovalGateActor init actor_id={self.actor_id}")

    @handler("request_approval")
    def request_approval(self, agent_id: str = "", action: str = "", context: dict = None) -> dict:
        """An agent requests human approval for a high-stakes action."""
        if self.fsm_state != "idle":
            return {
                "status": "busy",
                "message": f"Approval gate is already {self.fsm_state}",
                "current_agent": self.pending_agent_id,
            }

        self.fsm_state = "awaiting_approval"
        self.pending_agent_id = agent_id
        self.pending_request = {
            "action": action,
            "context": context or {},
            "requested_at_ms": host.now_ms(),
        }

        host.info(f"ApprovalGate: request from agent={agent_id} action={action}")
        host.incr_counter("approval_requests_total", 1)

        # Store request for external review (e.g., dashboard display)
        try:
            host.kv.put(
                f"approval_request:{self.actor_id}",
                json.dumps({**self.pending_request, "agent_id": agent_id})
            )
        except Exception:
            pass

        return {
            "status": "pending",
            "message": "Approval request submitted. Agent will be notified on decision.",
            "gate_id": self.actor_id,
        }

    @handler("approve")
    def approve(self, approver: str = "", comment: str = "") -> dict:
        """Human approves the pending action."""
        if self.fsm_state != "awaiting_approval":
            return {"error": f"No pending approval request (state={self.fsm_state})"}

        agent_id = self.pending_agent_id
        self.fsm_state = "approved"

        self.decision_history.append({
            "action": self.pending_request.get("action", ""),
            "decision": "approved",
            "approver": approver,
            "comment": comment,
            "decided_at_ms": host.now_ms(),
        })

        # Signal the suspended agent to resume
        try:
            host.send(agent_id, "workflow_signal:resume", {
                "decision": "approved",
                "approver": approver,
                "comment": comment,
            })
        except Exception as e:
            host.warn(f"Failed to signal agent {agent_id}: {e}")

        self.fsm_state = "idle"
        self.pending_agent_id = ""
        self.pending_request = {}

        host.incr_counter("approvals_granted_total", 1)
        host.info(f"ApprovalGate: approved agent={agent_id} approver={approver}")

        return {"status": "approved", "agent_id": agent_id, "approver": approver}

    @handler("reject")
    def reject(self, approver: str = "", reason: str = "") -> dict:
        """Human rejects the pending action."""
        if self.fsm_state != "awaiting_approval":
            return {"error": f"No pending approval request (state={self.fsm_state})"}

        agent_id = self.pending_agent_id
        self.fsm_state = "rejected"

        self.decision_history.append({
            "action": self.pending_request.get("action", ""),
            "decision": "rejected",
            "approver": approver,
            "reason": reason,
            "decided_at_ms": host.now_ms(),
        })

        # Signal agent with rejection
        try:
            host.send(agent_id, "workflow_signal:resume", {
                "decision": "rejected",
                "approver": approver,
                "reason": reason,
            })
        except Exception as e:
            host.warn(f"Failed to signal agent {agent_id} with rejection: {e}")

        self.fsm_state = "idle"
        self.pending_agent_id = ""
        self.pending_request = {}

        host.incr_counter("approvals_rejected_total", 1)
        host.info(f"ApprovalGate: rejected agent={agent_id} reason={reason}")

        return {"status": "rejected", "agent_id": agent_id, "reason": reason}

    @handler("get_status")
    def get_status(self) -> dict:
        return {
            "status": "ok",
            "state": self.fsm_state,
            "pending_agent_id": self.pending_agent_id,
            "pending_request": self.pending_request,
            "decision_count": len(self.decision_history),
        }

    @handler("get_history")
    def get_history(self) -> dict:
        return {
            "status": "ok",
            "decisions": self.decision_history,
            "count": len(self.decision_history),
        }
