# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Kueue → PlexSpaces: GPU Job Scheduler (Python WASM).
#
# Kueue-style AI training job queue: priority queue, resource quotas (GPUs),
# allocate with lock, preemption. GenServer + KV (job metadata) + host lock for allocate.

import json
from plexspaces import gen_server_actor, state, handler, init_handler, host

KV_PREFIX = "kueue:job:"
QUOTA_KEY = "kueue:quotas"
LOCK_NAME = "kueue-allocator"
DEFAULT_MAX_GPUS = 8


@gen_server_actor(facets=["virtual_actor", "locks"])
class JobSchedulerActor:
    """GPU job scheduler: submit (priority, gpu_request), allocate (with lock), complete, preempt, list_queue, get_quotas."""

    queue: list = state(default_factory=list)   # [{job_id, priority, gpu_request, submitted_at_ms, status}]
    used_gpus: int = state(default=0)
    max_gpus: int = state(default=DEFAULT_MAX_GPUS)
    processed_count: int = state(default=0)
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.queue = config.get("queue", self.queue or [])
        self.max_gpus = int(config.get("max_gpus", self.max_gpus or DEFAULT_MAX_GPUS))
        self.used_gpus = 0
        host.log("info", f"JobScheduler init max_gpus={self.max_gpus}")

    def _tenant_ns(self) -> tuple:
        return ("default", "default")

    @handler("submit", "call")
    def submit(self, job_id: str = "", priority: int = 0, gpu_request: int = 1) -> dict:
        if not job_id:
            return {"ok": False, "error": "job_id required"}
        for j in self.queue:
            if j.get("job_id") == job_id:
                return {"ok": False, "error": f"job {job_id} already queued"}
        t = host.now_ms()
        entry = {
            "job_id": job_id,
            "priority": int(priority),
            "gpu_request": int(gpu_request) if gpu_request > 0 else 1,
            "submitted_at_ms": t,
            "status": "pending",
        }
        self.queue = list(self.queue) + [entry]
        # KV: persist job metadata (Kueue-style)
        tenant, ns = self._tenant_ns()
        key = f"{KV_PREFIX}{job_id}"
        host.kv_put(key, json.dumps(entry))
        return {"ok": True, "job_id": job_id, "priority": entry["priority"], "status": "pending"}

    @handler("allocate", "call")
    def allocate(self) -> dict:
        """Allocate next job by priority; use lock for critical section."""
        t0 = host.now_ms()
        tenant, ns = self._tenant_ns()
        holder = host.self_id()
        lock_res = host.lock_acquire(tenant, ns, holder, LOCK_NAME, 10, 5000)
        if lock_res.startswith("ERROR") or not lock_res:
            return {"ok": False, "error": "lock_acquire failed", "job_id": None}
        try:
            lock_data = json.loads(lock_res)
        except json.JSONDecodeError:
            return {"ok": False, "error": "lock response invalid", "job_id": None}
        try:
            lock_id = lock_data.get("lock_key", LOCK_NAME)
            lock_version = lock_data.get("version", "")
            # Sort pending by priority desc, then by submitted_at asc
            pending = [j for j in self.queue if j.get("status") == "pending"]
            pending.sort(key=lambda j: (-j.get("priority", 0), j.get("submitted_at_ms", 0)))
            for j in pending:
                need = int(j.get("gpu_request", 1))
                if self.used_gpus + need <= self.max_gpus:
                    j["status"] = "allocated"
                    self.used_gpus += need
                    host.lock_release(lock_id, tenant, ns, holder, lock_version)
                    elapsed = host.now_ms() - t0
                    self.total_coord_ms += max(0, elapsed - 5)
                    self.total_compute_ms += 5
                    return {"ok": True, "job_id": j["job_id"], "gpu_request": need, "priority": j["priority"]}
            host.lock_release(lock_id, tenant, ns, holder, lock_version)
        except Exception as e:
            try:
                host.lock_release(LOCK_NAME, tenant, ns, holder, "")
            except Exception:
                pass
            return {"ok": False, "error": str(e), "job_id": None}
        return {"ok": True, "job_id": None, "message": "no capacity"}

    @handler("complete", "call")
    def complete(self, job_id: str = "") -> dict:
        if not job_id:
            return {"ok": False, "error": "job_id required"}
        for j in self.queue:
            if j.get("job_id") == job_id and j.get("status") == "allocated":
                j["status"] = "completed"
                self.used_gpus -= int(j.get("gpu_request", 1))
                if self.used_gpus < 0:
                    self.used_gpus = 0
                self.processed_count += 1
                return {"ok": True, "job_id": job_id}
        return {"ok": False, "error": f"job {job_id} not found or not allocated"}

    @handler("preempt", "call")
    def preempt(self, job_id: str = "") -> dict:
        if not job_id:
            return {"ok": False, "error": "job_id required"}
        for j in self.queue:
            if j.get("job_id") == job_id and j.get("status") == "allocated":
                j["status"] = "preempted"
                self.used_gpus -= int(j.get("gpu_request", 1))
                if self.used_gpus < 0:
                    self.used_gpus = 0
                # Re-queue as pending for retry (Kueue-style)
                j["status"] = "pending"
                return {"ok": True, "job_id": job_id, "requeued": True}
        return {"ok": False, "error": f"job {job_id} not found or not allocated"}

    @handler("list_queue", "call")
    def list_queue(self) -> dict:
        pending = [j for j in self.queue if j.get("status") == "pending"]
        allocated = [j for j in self.queue if j.get("status") == "allocated"]
        return {
            "ok": True,
            "pending": pending,
            "allocated": allocated,
            "pending_count": len(pending),
            "allocated_count": len(allocated),
        }

    @handler("get_quotas", "call")
    def get_quotas(self) -> dict:
        return {
            "ok": True,
            "used_gpus": self.used_gpus,
            "max_gpus": self.max_gpus,
            "processed_count": self.processed_count,
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }
