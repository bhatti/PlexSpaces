"""
Order Fulfillment Workflow - Temporal-style saga (Python WASM).

E-commerce order fulfillment with run/signal/query (Workflow behavior).
Same use case as examples/typescript/apps/migrating_temporal and examples/go/apps/migrating_temporal.
"""

from plexspaces import workflow_actor, state, host


@workflow_actor(facets=["virtual_actor", "durability"])
class OrderFulfillmentActor:
    """Workflow actor: run (saga steps), signal(cancel), query(status)."""

    order_id: str = state(default="")
    customer_id: str = state(default="")
    status: str = state(default="pending")
    steps: list = state(default_factory=list)
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
        self.customer_id = str(payload.get("customer_id") or self.customer_id)
        self.updated_at_ms = float(host.now_ms())

        if self.status not in ("pending", "validated"):
            return {
                "status": self.status,
                "order_id": self.order_id,
                "message": "Workflow already completed or cancelled",
            }

        compute_ms = 0.0
        step_names = [s.get("name") for s in self.steps if isinstance(s, dict)]

        if "validate" not in step_names:
            compute_ms += 8
            self.steps = list(self.steps) + [{"name": "validate", "completed_at_ms": host.now_ms()}]
            self.status = "validated"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._compensate("cancel_requested")

        if "reserve_inventory" not in step_names:
            compute_ms += 12
            self.steps = list(self.steps) + [{"name": "reserve_inventory", "completed_at_ms": host.now_ms()}]
            self.status = "inventory_reserved"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._compensate("cancel_requested")

        if "charge_payment" not in step_names:
            compute_ms += 10
            self.steps = list(self.steps) + [{"name": "charge_payment", "completed_at_ms": host.now_ms()}]
            self.status = "payment_charged"
            self.updated_at_ms = host.now_ms()
        if self.cancel_requested:
            return self._compensate("cancel_requested")

        if "ship" not in step_names:
            compute_ms += 15
            self.steps = list(self.steps) + [{"name": "ship", "completed_at_ms": host.now_ms()}]
            self.status = "shipped"
            self.updated_at_ms = host.now_ms()

        total_elapsed = float(host.now_ms() - t0)
        compute_reported = min(compute_ms, total_elapsed)
        coord_reported = max(0.0, total_elapsed - compute_reported)
        self.total_compute_ms += compute_reported
        self.total_coord_ms += coord_reported

        return {
            "status": self.status,
            "order_id": self.order_id,
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
                "order_id": self.order_id,
                "customer_id": self.customer_id,
                "status": self.status,
                "steps_count": len(self.steps),
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}

    def _compensate(self, reason: str) -> dict:
        self.status = "cancelled"
        self.updated_at_ms = host.now_ms()
        return {
            "status": "cancelled",
            "order_id": self.order_id,
            "reason": reason,
            "steps_rolled_back": len(self.steps),
        }
