# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Merlin → PlexSpaces: Scientific simulation manager (parameter sweep).
#
# Worker pool (elastic size): elastic pool API (checkout/checkin) + tuple space (work queue).
# - Coordinator (run): scatter tasks → checkout workers from pool, send work_available → gather → checkin.
#   If pool is not configured, falls back to process group broadcast.
# - Workers (work_available): take tasks from tuple space, run simulation, write results.
# Convention: sweep:coord-1 = coordinator; sweep:worker-0, worker-1, ... = pool workers.

from plexspaces import workflow_actor, state, handler, host

SIMULATION_MS = 22
TUPLE_PREFIX = "merlin"
WORKER_GROUP = "merlin-workers"
POOL_NAME = "merlin-workers"
MAX_CHECKOUT_WORKERS = 10
GATHER_POLL_MAX = 200
CHECKOUT_TIMEOUT_MS = 5000


@workflow_actor(facets=["virtual_actor", "durability"])
class SweepActor:
    """
    Parameter sweep: run() = coordinator (scatter → checkout workers from pool, send work → gather → checkin);
    work_available = worker (take params from tuple space, run simulation, write results).
    Uses elastic pool API when available; falls back to process group broadcast otherwise.
    """

    sweep_id: str = state(default="")
    num_params: int = state(default=0)
    num_completed: int = state(default=0)
    status: str = state(default="idle")
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)
    cancel_requested: bool = state(default=False)
    worker_joined: bool = state(default=False)

    def run(self, payload: dict) -> dict:
        """Coordinator: scatter tasks, checkout workers from pool and send work_available (or broadcast), gather, checkin."""
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.sweep_id = str(
            payload.get("sweep_id") or payload.get("job_id") or self.sweep_id
        )
        self.updated_at_ms = float(host.now_ms())

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        if self.status == "completed":
            return self._finish(t0, 0.0, "completed")

        num_params = int(payload.get("num_params", 10))
        if num_params <= 0:
            return self._finish(t0, 0.0, "no_params")

        self.num_params = num_params
        self.num_completed = 0
        self.status = "scattering"
        compute_ms = 0.0

        # Tuple space: scatter parameter-sweep tasks (list-in/list-out)
        for i in range(num_params):
            param_val = payload.get("param_base", 0) + i
            out = host.ts.write(
                [TUPLE_PREFIX, self.sweep_id, "task", f"p{i}", {"param": param_val}]
            )
            if out and out.startswith("ERROR"):
                host.log("warn", f"ts write failed: {out}")
            compute_ms += 2
        self.updated_at_ms = host.now_ms()
        self.status = "running"

        # Worker pool: use elastic pool API (checkout → send work → gather → checkin); fallback to process group broadcast
        checkout_handles: list = []
        try:
            for _ in range(min(MAX_CHECKOUT_WORKERS, num_params)):
                handle = host.pool_checkout(POOL_NAME, CHECKOUT_TIMEOUT_MS)
                if handle is None:
                    break
                checkout_handles.append(handle)
                out = host.send(
                    handle["actor_id"],
                    "work_available",
                    {"sweep_id": self.sweep_id, "num_params": num_params},
                )
                if out and out.startswith("ERROR"):
                    host.log("warn", f"send to worker failed: {out}")
            if not checkout_handles:
                host.process_groups.broadcast(
                    WORKER_GROUP,
                    "work_available",
                    {"sweep_id": self.sweep_id, "num_params": num_params},
                )
        except Exception as e:
            host.log("warn", f"pool/broadcast failed: {e}")
            try:
                host.process_groups.broadcast(
                    WORKER_GROUP,
                    "work_available",
                    {"sweep_id": self.sweep_id, "num_params": num_params},
                )
            except Exception as e2:
                host.log("warn", f"pg_broadcast failed: {e2}")
        self.total_coord_ms += float(host.now_ms() - t0)

        # Gather results from tuple space
        pattern = [TUPLE_PREFIX, self.sweep_id, "result", None, None]
        for _ in range(GATHER_POLL_MAX):
            if self.cancel_requested:
                self.status = "cancelled"
                return self._finish(t0, compute_ms, "cancelled")
            results = host.ts.read_all(pattern)
            self.num_completed = len(results) if results else 0
            if self.num_completed >= num_params:
                break
            self.updated_at_ms = host.now_ms()

        # Checkin all workers that were checked out from the pool
        for handle in checkout_handles:
            try:
                host.pool_checkin(
                    POOL_NAME,
                    handle["actor_id"],
                    handle["checkout_id"],
                    True,
                )
            except Exception as e:
                host.log("warn", f"pool_checkin failed: {e}")

        self.status = "completed"
        self.updated_at_ms = host.now_ms()
        return self._finish(t0, compute_ms, "completed")

    def _finish(self, t0: float, compute_ms: float, status: str) -> dict:
        elapsed = float(host.now_ms() - t0)
        if elapsed < compute_ms:
            elapsed = compute_ms
        coord_ms = max(0.0, elapsed - compute_ms)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += coord_ms
        # Approximate data size: each task tuple ~100 bytes, result ~80 bytes
        approx_bytes = (self.num_params * 100) + (self.num_completed * 80)
        return {
            "status": status,
            "sweep_id": self.sweep_id,
            "num_params": self.num_params,
            "num_completed": self.num_completed,
            "data_size_bytes": approx_bytes,
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
                "sweep_id": self.sweep_id,
                "status": self.status,
                "num_params": self.num_params,
                "num_completed": self.num_completed,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}

    @handler("work_available")
    def on_work_available(
        self,
        sweep_id: str = "",
        num_params: int = 0,
        **kwargs,
    ) -> dict:
        """Worker: take tasks from tuple space, run simulation, write results. Joins process group for broadcast fallback."""
        if not self.worker_joined:
            try:
                host.process_groups.join(WORKER_GROUP)
                self.worker_joined = True
                host.log("info", f"Joined worker pool {WORKER_GROUP}")
            except Exception as e:
                host.log("warn", f"pg_join failed: {e}")
        if not sweep_id or num_params <= 0:
            return {"ok": True, "message": "warmup"}
        t0 = host.now_ms()
        pattern = [TUPLE_PREFIX, sweep_id, "task", None, None]
        processed = 0
        compute_ms = 0.0
        while True:
            taken = host.ts.take(pattern)
            if taken is None:
                break
            param_id = taken[3] if len(taken) > 3 else ""
            compute_ms += SIMULATION_MS
            host.ts.write(
                [
                    TUPLE_PREFIX,
                    sweep_id,
                    "result",
                    param_id,
                    {"ok": True, "ms": SIMULATION_MS},
                ]
            )
            processed += 1
        elapsed = float(host.now_ms() - t0)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += max(0.0, elapsed - compute_ms)
        return {"ok": True, "processed": processed, "sweep_id": sweep_id}
