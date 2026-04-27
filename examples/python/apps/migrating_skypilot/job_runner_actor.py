# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# SkyPilot → PlexSpaces: Spot Job Runner with Checkpointing (Python WASM).
#
# SkyPilot-style: launch job on "spot" → run steps → checkpoint to KV after each step;
# on preemption (simulated), next run restores from KV and continues (failover).
# Uses @workflow_actor with virtual_actor, durability; KV for checkpoints.

import json
from plexspaces import workflow_actor, state, host

KV_CHECKPOINT_PREFIX = "skypilot:checkpoint:"
STEP_MS = 25  # simulated work per step


@workflow_actor(facets=["virtual_actor", "durability"])
class JobRunnerActor:
    """Spot job runner: run (resume from KV checkpoint or start fresh), checkpoint each step; signal(cancel), query(status)."""

    job_id: str = state(default="")
    current_step: int = state(default=0)
    total_steps: int = state(default=0)
    status: str = state(default="pending")  # pending, running, preempted, completed, cancelled
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)
    cancel_requested: bool = state(default=False)

    def _checkpoint_key(self) -> str:
        return f"{KV_CHECKPOINT_PREFIX}{self.job_id}"

    def _save_checkpoint(self) -> None:
        data = {
            "job_id": self.job_id,
            "current_step": self.current_step,
            "total_steps": self.total_steps,
            "timestamp_ms": host.now_ms(),
        }
        host.kv_put(self._checkpoint_key(), json.dumps(data))

    def _load_checkpoint(self) -> bool:
        raw = host.kv_get(self._checkpoint_key())
        if not raw or raw.startswith("ERROR"):
            return False
        try:
            data = json.loads(raw)
            self.current_step = int(data.get("current_step", 0))
            self.total_steps = int(data.get("total_steps", self.total_steps))
            return True
        except (json.JSONDecodeError, TypeError):
            return False

    def run(self, payload: dict) -> dict:
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.job_id = str(payload.get("job_id") or self.job_id)
        self.updated_at_ms = float(host.now_ms())

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        if self.status == "completed":
            return self._finish(t0, 0.0, "completed")

        total_steps = int(payload.get("total_steps", 5))
        if total_steps > 0:
            self.total_steps = total_steps
        simulate_preemption_at = int(payload.get("simulate_preemption_at_step") or payload.get("simulate_preemption_at") or -1)

        # Failover: restore from KV checkpoint if present (e.g. after preemption)
        if self.current_step == 0 and self.total_steps > 0:
            if self._load_checkpoint():
                self.status = "running"
                host.log("info", f"Job {self.job_id} restored from checkpoint at step {self.current_step}")

        if self.total_steps <= 0:
            return self._finish(t0, 0.0, "no_steps")

        compute_ms = 0.0
        while self.current_step < self.total_steps:
            self.status = "running"
            compute_ms += STEP_MS
            self.current_step += 1
            self.updated_at_ms = host.now_ms()
            self._save_checkpoint()
            if simulate_preemption_at >= 0 and self.current_step == simulate_preemption_at:
                self.status = "preempted"
                return self._finish(t0, compute_ms, "preempted", at_step=self.current_step)
            if self.cancel_requested:
                self.status = "cancelled"
                return self._finish(t0, compute_ms, "cancelled")

        self.status = "completed"
        host.kv_delete(self._checkpoint_key())
        return self._finish(t0, compute_ms, "completed")

    def _finish(self, t0: float, compute_ms: float, status: str, at_step: int = 0) -> dict:
        elapsed = float(host.now_ms() - t0)
        if elapsed < compute_ms:
            elapsed = compute_ms
        coord_ms = max(0.0, elapsed - compute_ms)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += coord_ms
        out = {
            "status": status,
            "job_id": self.job_id,
            "current_step": self.current_step,
            "total_steps": self.total_steps,
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }
        if at_step > 0:
            out["preempted_at_step"] = at_step
        return out

    def signal(self, name: str, data: dict) -> None:
        if name == "cancel":
            self.cancel_requested = True
            self.updated_at_ms = host.now_ms()

    def query(self, name: str, params: dict) -> dict:
        if name == "status":
            return {
                "job_id": self.job_id,
                "status": self.status,
                "current_step": self.current_step,
                "total_steps": self.total_steps,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}
