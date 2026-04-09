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
# Full scheduler snapshot (queue + counters). Per-actor KV namespace survives WASM re-instantiation
# when component state round-trip is lossy; keeps allocate/complete consistent.
SCHEDULER_STATE_KEY = "kueue:scheduler_state"
LOCK_NAME = "kueue-allocator"
DEFAULT_MAX_GPUS = 8


@gen_server_actor(facets=["virtual_actor", "keyvalue", "locks"])
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
        self.max_gpus = int(config.get("max_gpus", self.max_gpus or DEFAULT_MAX_GPUS))
        if "queue" in config and config["queue"] is not None:
            self.queue = list(config["queue"])
        self.used_gpus = 0
        host.log("info", f"JobScheduler init max_gpus={self.max_gpus}")

    def _restore_scheduler_state(self) -> None:
        """Load queue and counters from KV (authoritative across WASM handle/re-instantiate cycles)."""
        raw = host.kv_get(SCHEDULER_STATE_KEY)
        if not raw or not str(raw).strip():
            return
        try:
            d = json.loads(raw)
        except json.JSONDecodeError:
            return
        q = d.get("queue")
        if isinstance(q, list):
            self.queue = q
        if "used_gpus" in d:
            self.used_gpus = int(d["used_gpus"])
        if "processed_count" in d:
            self.processed_count = int(d["processed_count"])
        if "total_compute_ms" in d:
            self.total_compute_ms = float(d["total_compute_ms"])
        if "total_coord_ms" in d:
            self.total_coord_ms = float(d["total_coord_ms"])

    def _save_scheduler_state(self) -> None:
        """Persist scheduler state so the next message sees post-allocate / post-complete queue."""
        blob = json.dumps(
            {
                "queue": self.queue,
                "used_gpus": self.used_gpus,
                "processed_count": self.processed_count,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
            }
        )
        err = host.kv_put(SCHEDULER_STATE_KEY, blob)
        if err and str(err).startswith("ERROR"):
            host.log("warn", f"scheduler KV save failed: {err}")

    def _tenant_ns(self) -> tuple:
        return ("default", "default")

    def _job_kv_key(self, job_id: str) -> str:
        return f"{KV_PREFIX}{job_id}"

    def _put_job_entry(self, entry: dict) -> None:
        """Persist single job record; survives WASM re-instantiation when host KV is available."""
        err = host.kv_put(self._job_kv_key(entry["job_id"]), json.dumps(entry))
        if err and str(err).startswith("ERROR"):
            host.log("warn", f"job KV put failed job_id={entry.get('job_id')}: {err}")

    @handler("submit", "call")
    def submit(self, job_id: str = "", priority: int = 0, gpu_request: int = 1) -> dict:
        self._restore_scheduler_state()
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
        self._put_job_entry(entry)
        self._save_scheduler_state()
        return {"ok": True, "job_id": job_id, "priority": entry["priority"], "status": "pending"}

    @handler("allocate", "call")
    def allocate(self) -> dict:
        """Allocate next job by priority; use lock for critical section."""
        self._restore_scheduler_state()
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
                    # Per-job KV must reflect allocated so complete() works after WASM re-instantiation
                    self._put_job_entry(j)
                    host.lock_release(lock_id, tenant, ns, holder, lock_version)
                    elapsed = host.now_ms() - t0
                    self.total_coord_ms += max(0, elapsed - 5)
                    self.total_compute_ms += 5
                    self._save_scheduler_state()
                    return {"ok": True, "job_id": j["job_id"], "gpu_request": need, "priority": j["priority"]}
            host.lock_release(lock_id, tenant, ns, holder, lock_version)
        except Exception as e:
            try:
                host.lock_release(LOCK_NAME, tenant, ns, holder, "")
            except Exception:
                pass
            return {"ok": False, "error": str(e), "job_id": None}
        self._save_scheduler_state()
        return {"ok": True, "job_id": None, "message": "no capacity"}

    @handler("complete", "call")
    def complete(self, job_id: str = "") -> dict:
        self._restore_scheduler_state()
        if not job_id:
            return {"ok": False, "error": "job_id required"}

        # Prefer per-job KV (authoritative across WASM handle cycles; allocate updates this key).
        raw = host.kv_get(self._job_kv_key(job_id))
        if raw and not str(raw).startswith("ERROR") and str(raw).strip():
            try:
                entry = json.loads(raw)
            except json.JSONDecodeError:
                entry = None
            if isinstance(entry, dict) and entry.get("status") == "allocated":
                gpu_req = int(entry.get("gpu_request", 1))
                entry["status"] = "completed"
                self._put_job_entry(entry)
                merged = False
                for j in self.queue:
                    if j.get("job_id") == job_id:
                        j["status"] = "completed"
                        merged = True
                        break
                if not merged:
                    self.queue = list(self.queue) + [entry]
                self.used_gpus = max(0, int(self.used_gpus) - gpu_req)
                self.processed_count += 1
                self.total_compute_ms += 5.0
                self._save_scheduler_state()
                return {"ok": True, "job_id": job_id}

        for j in self.queue:
            if j.get("job_id") == job_id and j.get("status") == "allocated":
                j["status"] = "completed"
                self.used_gpus -= int(j.get("gpu_request", 1))
                if self.used_gpus < 0:
                    self.used_gpus = 0
                self.processed_count += 1
                self._put_job_entry(j)
                self._save_scheduler_state()
                return {"ok": True, "job_id": job_id}
        return {"ok": False, "error": f"job {job_id} not found or not allocated"}

    @handler("preempt", "call")
    def preempt(self, job_id: str = "") -> dict:
        self._restore_scheduler_state()
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
                self._put_job_entry(j)
                self._save_scheduler_state()
                return {"ok": True, "job_id": job_id, "requeued": True}
        rawp = host.kv_get(self._job_kv_key(job_id))
        if rawp and str(rawp).strip() and not str(rawp).startswith("ERROR"):
            try:
                ent = json.loads(rawp)
            except json.JSONDecodeError:
                ent = None
            if isinstance(ent, dict) and ent.get("status") == "allocated":
                ent["status"] = "pending"
                self.used_gpus = max(0, int(self.used_gpus) - int(ent.get("gpu_request", 1)))
                self._put_job_entry(ent)
                for x in self.queue:
                    if x.get("job_id") == job_id:
                        x["status"] = "pending"
                        break
                self._save_scheduler_state()
                return {"ok": True, "job_id": job_id, "requeued": True}
        return {"ok": False, "error": f"job {job_id} not found or not allocated"}

    @handler("list_queue", "call")
    def list_queue(self) -> dict:
        self._restore_scheduler_state()
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
        self._restore_scheduler_state()
        return {
            "ok": True,
            "used_gpus": self.used_gpus,
            "max_gpus": self.max_gpus,
            "processed_count": self.processed_count,
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }
