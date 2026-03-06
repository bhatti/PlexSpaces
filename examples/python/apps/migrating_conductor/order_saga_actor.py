"""
Order Saga Workflow - Netflix Conductor/Orkes style (Python WASM).

E-commerce order saga: reserve_payment -> reserve_inventory -> check_discount
-> create_shipment -> publish_event; compensation (release/restore/cancel) on failure.
Uses @workflow_actor with virtual_actor and durability facets.
"""

from plexspaces import workflow_actor, state, host


# Saga step names (order matters for compensation reverse order)
SAGA_STEPS = ["process_payment", "update_inventory", "check_discount", "create_shipment", "publish_event"]
# Simulated ms per step (non-trivial for metrics)
STEP_MS = {"process_payment": 35, "update_inventory": 30, "check_discount": 15, "create_shipment": 40, "publish_event": 20}


@workflow_actor(facets=["virtual_actor", "durability"])
class OrderSagaActor:
    """Workflow actor: run (saga steps + optional compensation), signal(cancel), query(status)."""

    order_id: str = state(default="")
    status: str = state(default="pending")  # pending, completed, compensated, cancelled, failed
    steps_completed: list = state(default_factory=list)
    results: dict = state(default_factory=dict)
    failed_at_step: str = state(default="")
    compensation_done: bool = state(default=False)
    cancel_requested: bool = state(default=False)
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)

    def run(self, payload: dict) -> dict:
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.order_id = str(payload.get("order_id") or self.order_id)
        self.updated_at_ms = float(host.now_ms())

        if self.status not in ("pending", "completed", "compensated", "cancelled", "failed"):
            self.status = "pending"

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        simulate_fail_at = (payload.get("simulate_fail_at") or "").strip()
        compute_ms = 0.0
        done = list(self.steps_completed)

        for step in SAGA_STEPS:
            if step in done:
                continue
            step_ms = STEP_MS.get(step, 25)
            compute_ms += step_ms
            if simulate_fail_at and step == simulate_fail_at:
                self.failed_at_step = step
                self.status = "failed"
                comp_ms = self._run_compensation(done)
                compute_ms += comp_ms
                self.compensation_done = True
                self.status = "compensated"
                self.updated_at_ms = host.now_ms()
                return self._finish(t0, compute_ms, "compensated", failed_at=step)
            self.results = dict(self.results)
            self.results[step] = {"ok": True, "ms": step_ms}
            done = list(done) + [step]
            self.steps_completed = done
            self.updated_at_ms = host.now_ms()

        self.status = "completed"
        return self._finish(t0, compute_ms, "completed")

    def _run_compensation(self, completed_steps: list) -> float:
        """Run compensation in reverse order; return total compensation compute ms."""
        comp_ms = 0.0
        reverse = [s for s in reversed(SAGA_STEPS) if s in completed_steps]
        for step in reverse:
            if step == "process_payment":
                comp_ms += 20
            elif step == "update_inventory":
                comp_ms += 25
            elif step == "create_shipment":
                comp_ms += 30
        return comp_ms

    def _finish(
        self, t0: float, compute_ms: float, status: str, failed_at: str = ""
    ) -> dict:
        total_elapsed = float(host.now_ms() - t0)
        if total_elapsed < compute_ms:
            total_elapsed = compute_ms
        coord_ms = max(0.0, total_elapsed - compute_ms)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += coord_ms
        out = {
            "status": status,
            "order_id": self.order_id,
            "steps_completed": len(self.steps_completed),
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }
        if failed_at:
            out["failed_at_step"] = failed_at
            out["compensation_done"] = self.compensation_done
        return out

    def signal(self, name: str, data: dict) -> None:
        if name == "cancel":
            self.cancel_requested = True
            self.updated_at_ms = host.now_ms()

    def query(self, name: str, params: dict) -> dict:
        if name == "status":
            return {
                "order_id": self.order_id,
                "status": self.status,
                "steps_completed": list(self.steps_completed),
                "failed_at_step": self.failed_at_step or None,
                "compensation_done": self.compensation_done,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}
