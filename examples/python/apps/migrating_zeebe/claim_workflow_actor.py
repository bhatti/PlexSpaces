# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Zeebe/Camunda → PlexSpaces: Insurance Claim Workflow (Python WASM).
#
# BPMN-style claim: submit → validate → (human) review → approve/reject.
# SLA and escalation: if pending_review exceeds sla_ms, status becomes escalated;
# Reminder facet (when host fires ReminderFired) can trigger escalation on next run.
# Uses @workflow_actor with virtual_actor, durability, and reminder facets.

from plexspaces import workflow_actor, state, host


STEP_MS = {"submit": 10, "validate": 25, "human_review": 30, "approved": 15, "rejected": 10, "escalated": 20}
DEFAULT_SLA_MS = 60_000  # 60s SLA for review (use lower in tests via payload)


@workflow_actor(facets=["virtual_actor", "durability", "reminder"])
class ClaimWorkflowActor:
    """Insurance claim workflow: run (submit/validate/review/approve/reject/escalate), signal(escalate, cancel), query(status)."""

    claim_id: str = state(default="")
    status: str = state(default="pending")  # pending, validated, pending_review, approved, rejected, escalated, cancelled
    steps_completed: list = state(default_factory=list)
    submitted_at_ms: float = state(default=0.0)
    sla_ms: float = state(default=0.0)  # 0 = use default
    escalation_requested: bool = state(default=False)
    cancel_requested: bool = state(default=False)
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)

    def run(self, payload: dict) -> dict:
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.claim_id = str(payload.get("claim_id") or self.claim_id)
        self.updated_at_ms = float(host.now_ms())
        sla_ms = float(payload.get("sla_ms") or 0) or self.sla_ms or DEFAULT_SLA_MS
        if sla_ms > 0:
            self.sla_ms = sla_ms

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        if self.status in ("approved", "rejected", "escalated", "cancelled"):
            return self._finish(t0, 0.0, self.status)

        compute_ms = 0.0
        done = list(self.steps_completed)

        # Submit
        if "submit" not in done:
            compute_ms += STEP_MS["submit"]
            self.submitted_at_ms = float(host.now_ms())
            done = list(done) + ["submit"]
            self.steps_completed = done
            self.status = "validated" if "validate" in done else "pending"
            self.updated_at_ms = host.now_ms()
            return self._finish(t0, compute_ms, self.status)

        # Validate
        if "validate" not in done:
            compute_ms += STEP_MS["validate"]
            done = list(done) + ["validate"]
            self.steps_completed = done
            self.status = "pending_review"
            self.updated_at_ms = host.now_ms()
            return self._finish(t0, compute_ms, "pending_review")

        # Human review phase: we have submit+validate, not yet approved/rejected/escalated
        if "approved" not in done and "rejected" not in done and "escalated" not in done:
            now = host.now_ms()
            elapsed = now - self.submitted_at_ms if self.submitted_at_ms else 0
            if self.escalation_requested or (self.sla_ms and elapsed >= self.sla_ms):
                compute_ms += STEP_MS["escalated"]
                done = list(done) + ["escalated"]
                self.steps_completed = done
                self.status = "escalated"
                self.updated_at_ms = host.now_ms()
                return self._finish(t0, compute_ms, "escalated")
            action = (payload.get("action") or "").strip().lower()
            if action == "approve":
                compute_ms += STEP_MS["approved"]
                done = list(done) + ["approved"]
                self.steps_completed = done
                self.status = "approved"
                self.updated_at_ms = host.now_ms()
                return self._finish(t0, compute_ms, "approved")
            if action == "reject":
                compute_ms += STEP_MS["rejected"]
                done = list(done) + ["rejected"]
                self.steps_completed = done
                self.status = "rejected"
                self.updated_at_ms = host.now_ms()
                return self._finish(t0, compute_ms, "rejected")
            return self._finish(t0, compute_ms, "pending_review")

        return self._finish(t0, compute_ms, self.status)

    def _finish(
        self, t0: float, compute_ms: float, status: str
    ) -> dict:
        total_elapsed = float(host.now_ms() - t0)
        if total_elapsed < compute_ms:
            total_elapsed = compute_ms
        coord_ms = max(0.0, total_elapsed - compute_ms)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += coord_ms
        return {
            "status": status,
            "claim_id": self.claim_id,
            "steps_completed": len(self.steps_completed),
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }

    def signal(self, name: str, data: dict) -> None:
        if name == "cancel":
            self.cancel_requested = True
            self.updated_at_ms = host.now_ms()
        elif name == "escalate":
            self.escalation_requested = True
            self.updated_at_ms = host.now_ms()
        elif name == "ReminderFired":
            # When ReminderFacet fires (SLA reminder), request escalation on next run
            self.escalation_requested = True
            self.updated_at_ms = host.now_ms()

    def query(self, name: str, params: dict) -> dict:
        if name == "status":
            return {
                "claim_id": self.claim_id,
                "status": self.status,
                "steps_completed": list(self.steps_completed),
                "submitted_at_ms": self.submitted_at_ms,
                "sla_ms": self.sla_ms,
                "escalation_requested": self.escalation_requested,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}
