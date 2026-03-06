# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# EFlows4HPC → PlexSpaces: HPC Ensemble Simulation (Python WASM).
#
# Single actor class: coordinator (run) and workers (tasks_ready handler).
# - Tuple space: host.ts.write / host.ts.take / host.ts.read_all (list-in, list-out).
# - Process group: workers join on first use; coordinator host.process_groups.broadcast.
# Convention: ensemble:coord-1 = coordinator; ensemble:worker-0, ensemble:worker-1 = workers.

from plexspaces import workflow_actor, state, handler, host

TASK_MS = 18
RESULT_PREFIX = "ensemble"
WORKER_GROUP = "ensemble-workers"
GATHER_POLL_MAX = 200


@workflow_actor(facets=["virtual_actor", "durability"])
class EnsembleActor:
    """
    Ensemble: run() = coordinator (scatter → broadcast → gather);
    tasks_ready = worker (join group, take tasks from tuple space, write results).
    """

    ensemble_id: str = state(default="")
    num_tasks: int = state(default=0)
    num_completed: int = state(default=0)
    status: str = state(default="idle")
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)
    cancel_requested: bool = state(default=False)
    worker_joined: bool = state(default=False)

    def run(self, payload: dict) -> dict:
        """Coordinator: scatter to tuple space, broadcast to workers, gather results."""
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.ensemble_id = str(payload.get("ensemble_id") or payload.get("job_id") or self.ensemble_id)
        self.updated_at_ms = float(host.now_ms())

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        if self.status == "completed":
            return self._finish(t0, 0.0, "completed")

        num_tasks = int(payload.get("num_tasks", 10))
        if num_tasks <= 0:
            return self._finish(t0, 0.0, "no_tasks")

        self.num_tasks = num_tasks
        self.num_completed = 0
        self.status = "scattering"
        compute_ms = 0.0

        # Tuple space: scatter task tuples (host.ts = list-in/list-out)
        for i in range(num_tasks):
            out = host.ts.write([RESULT_PREFIX, self.ensemble_id, "task", f"t{i}", i])
            if out and out.startswith("ERROR"):
                host.log("warn", f"ts write failed: {out}")
            compute_ms += 2
        self.updated_at_ms = host.now_ms()
        self.status = "running"

        # Process group: broadcast (msg_type used for routing; payload data-only)
        try:
            host.process_groups.broadcast(
                WORKER_GROUP,
                "tasks_ready",
                {"ensemble_id": self.ensemble_id, "num_tasks": num_tasks},
            )
        except Exception as e:
            host.log("warn", f"pg_broadcast failed: {e}")
        self.total_coord_ms += float(host.now_ms() - t0)

        # Tuple space: gather result tuples
        pattern = [RESULT_PREFIX, self.ensemble_id, "result", None, None]
        for _ in range(GATHER_POLL_MAX):
            if self.cancel_requested:
                self.status = "cancelled"
                return self._finish(t0, compute_ms, "cancelled")
            results = host.ts.read_all(pattern)
            self.num_completed = len(results) if results else 0
            if self.num_completed >= num_tasks:
                break
            self.updated_at_ms = host.now_ms()

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
        return {
            "status": status,
            "ensemble_id": self.ensemble_id,
            "num_tasks": self.num_tasks,
            "num_completed": self.num_completed,
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
                "ensemble_id": self.ensemble_id,
                "status": self.status,
                "num_tasks": self.num_tasks,
                "num_completed": self.num_completed,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}

    @handler("tasks_ready")
    def on_tasks_ready(
        self,
        ensemble_id: str = "",
        num_tasks: int = 0,
        **kwargs,
    ) -> dict:
        """Worker: join group on first use; take tasks from tuple space, write results."""
        if not self.worker_joined:
            try:
                host.process_groups.join(WORKER_GROUP)
                self.worker_joined = True
                host.log("info", f"Joined process group {WORKER_GROUP}")
            except Exception as e:
                host.log("warn", f"pg_join failed: {e}")
        if not ensemble_id or num_tasks <= 0:
            return {"ok": True, "message": "warmup"}
        t0 = host.now_ms()
        pattern = [RESULT_PREFIX, ensemble_id, "task", None, None]
        processed = 0
        compute_ms = 0.0
        while True:
            taken = host.ts.take(pattern)
            if taken is None:
                break
            task_id = taken[3] if len(taken) > 3 else ""
            compute_ms += TASK_MS
            host.ts.write([RESULT_PREFIX, ensemble_id, "result", task_id, {"ok": True, "ms": TASK_MS}])
            processed += 1
        elapsed = float(host.now_ms() - t0)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += max(0.0, elapsed - compute_ms)
        return {"ok": True, "processed": processed, "ensemble_id": ensemble_id}
