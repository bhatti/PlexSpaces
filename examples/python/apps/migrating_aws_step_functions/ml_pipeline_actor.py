"""
ML Pipeline Workflow - AWS Step Functions style (Python WASM).

AI/ML pipeline: data_prep (fan-out) -> fan-in -> training -> evaluation -> deploy.
Uses @workflow_actor with virtual_actor and durability (same pattern as migrating_temporal).
"""

from plexspaces import workflow_actor, state, host


# Simulated compute ms per step (non-trivial for metrics)
DATA_PREP_SHARD_MS = 25
TRAINING_MS = 80
EVALUATION_MS = 40
DEPLOY_MS = 30
FAN_OUT_SHARDS = 4


@workflow_actor(facets=["virtual_actor", "durability"])
class MLPipelineActor:
    """Workflow actor: run (pipeline steps with fan-out/fan-in), signal(cancel), query(status)."""

    pipeline_id: str = state(default="")
    status: str = state(default="pending")  # pending, data_prep_done, training_done, evaluation_done, deployed, cancelled, failed
    steps: list = state(default_factory=list)
    cancel_requested: bool = state(default=False)
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)
    fan_out_results: list = state(default_factory=list)

    def run(self, payload: dict) -> dict:
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.pipeline_id = str(payload.get("pipeline_id") or self.pipeline_id)
        self.updated_at_ms = float(host.now_ms())

        if self.status not in ("pending", "data_prep_done", "training_done", "evaluation_done"):
            return {
                "status": self.status,
                "pipeline_id": self.pipeline_id,
                "message": "Workflow already completed or cancelled",
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
            }

        compute_ms = 0.0
        step_names = [s.get("name") for s in self.steps if isinstance(s, dict)]

        # Step 1: Data prep (fan-out: N shards, then fan-in)
        if "data_prep" not in step_names:
            for shard in range(FAN_OUT_SHARDS):
                compute_ms += DATA_PREP_SHARD_MS
                self.fan_out_results = list(self.fan_out_results) + [{"shard": shard, "rows": 1000}]
            self.steps = list(self.steps) + [{"name": "data_prep", "completed_at_ms": host.now_ms(), "shards": FAN_OUT_SHARDS}]
            self.status = "data_prep_done"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._finish_cancelled("cancel_requested", compute_ms, t0)

        # Step 2: Training
        if "training" not in step_names:
            compute_ms += TRAINING_MS
            self.steps = list(self.steps) + [{"name": "training", "completed_at_ms": host.now_ms()}]
            self.status = "training_done"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._finish_cancelled("cancel_requested", compute_ms, t0)

        # Step 3: Evaluation
        if "evaluation" not in step_names:
            compute_ms += EVALUATION_MS
            self.steps = list(self.steps) + [{"name": "evaluation", "completed_at_ms": host.now_ms()}]
            self.status = "evaluation_done"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._finish_cancelled("cancel_requested", compute_ms, t0)

        # Step 4: Deploy
        if "deploy" not in step_names:
            compute_ms += DEPLOY_MS
            self.steps = list(self.steps) + [{"name": "deploy", "completed_at_ms": host.now_ms()}]
            self.status = "deployed"
            self.updated_at_ms = host.now_ms()

        total_elapsed = float(host.now_ms() - t0)
        compute_reported = compute_ms
        if total_elapsed < compute_ms:
            total_elapsed = compute_ms
        coord_reported = max(0.0, total_elapsed - compute_reported)
        self.total_compute_ms += compute_reported
        self.total_coord_ms += coord_reported

        return {
            "status": self.status,
            "pipeline_id": self.pipeline_id,
            "steps_completed": len(self.steps),
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }

    def signal(self, name: str, data: dict) -> None:
        if name == "cancel":
            self.cancel_requested = True
            self.updated_at_ms = host.now_ms()

    def query(self, name: str, params: dict) -> dict:
        if name == "status":
            return {
                "pipeline_id": self.pipeline_id,
                "status": self.status,
                "steps_count": len(self.steps),
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}

    def _finish_cancelled(self, reason: str, compute_ms: float, t0: float) -> dict:
        self.status = "cancelled"
        self.updated_at_ms = host.now_ms()
        total_elapsed = float(host.now_ms() - t0)
        compute_reported = compute_ms
        if total_elapsed < compute_ms:
            total_elapsed = compute_ms
        coord_reported = max(0.0, total_elapsed - compute_reported)
        self.total_compute_ms += compute_reported
        self.total_coord_ms += coord_reported
        return {
            "status": "cancelled",
            "pipeline_id": self.pipeline_id,
            "reason": reason,
            "steps_rolled_back": len(self.steps),
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }
