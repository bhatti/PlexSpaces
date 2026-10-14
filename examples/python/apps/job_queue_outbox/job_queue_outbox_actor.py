# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
"""
Job Queue Outbox Actor — Python WASM

Demonstrates high-scale outbox dispatch with admission control:
  - Leader creates 100 virtual workers via host.create_shard_group()
  - Leader dispatches jobs to all workers via host.outbox.dispatch()
  - Upgrade simulation: Ask workers for upgrade status via host.ask()
  - Heartbeat OVERWRITE: LWW idempotency with conflict_resolution="overwrite"
  - Scaling benchmark: measures throughput at 10/50/100 worker scales

## Python SDK API used

  host.create_shard_group(request)   -> CreateShardGroupResponse
  host.outbox.dispatch(ctx, dispatcher_id, targets, payload, msg_type, idem_key, conflict_resolution)
  host.ask(to, msg_type, payload, timeout_ms)    -> Any
"""

import json
import time

from plexspaces import actor, state, handler, init_handler, host
from plexspaces.shard_types import CreateShardGroupRequest

DISPATCHER_ID = "job-outbox-dispatcher"
HEARTBEAT_IDEM_KEY = "hb-leader"
DEFAULT_WORKERS = 100


# ============================================================================
# JobQueueLeader — Coordinator Actor
# ============================================================================

@actor
class JobQueueLeader:
    """Leader actor: creates shard workers, dispatches jobs, tracks metrics."""

    total_dispatched: int = state(default=0)
    total_accepted: int = state(default=0)
    total_rejected: int = state(default=0)
    dispatch_count: int = state(default=0)
    wall_time_ms: int = state(default=0)
    heartbeat_count: int = state(default=0)
    ask_rtt_min_ms: int = state(default=0)
    ask_rtt_max_ms: int = state(default=0)
    ask_rtt_sum_ms: int = state(default=0)
    ask_rtt_count: int = state(default=0)
    shard_group_id: str = state(default="")
    worker_ids: list = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        host.log("info", "JobQueueLeader initialized")

    @handler("init_workers", "call")
    def init_workers(self, count: int = DEFAULT_WORKERS) -> dict:
        """Create N virtual workers via CreateShardGroup."""
        group_id = f"py-job-workers-{int(time.time() * 1000) % 100000}"
        req = CreateShardGroupRequest(
            group_id=group_id,
            actor_type="JobQueueWorker",
            shard_count=count,
        )
        try:
            resp = host.create_shard_group(req)
            self.shard_group_id = resp.group_id
            self.worker_ids = list(resp.shard_actor_ids)
        except Exception as e:
            return {"error": f"create_shard_group failed: {e}"}

        return {
            "worker_count": len(self.worker_ids),
            "group_id": self.shard_group_id,
        }

    @handler("dispatch_jobs", "call")
    def dispatch_jobs(self, payload: dict = None) -> dict:
        """Dispatch jobs to all shard workers via the outbox relay."""
        if not self.worker_ids:
            return {"error": "no workers — call init_workers first"}

        if payload is None:
            payload = {"task": "process_item"}

        ctx = {"tenant": "default", "namespace": "default"}
        start = time.time()
        accepted, rejected = host.outbox.dispatch(
            ctx, DISPATCHER_ID, self.worker_ids, payload, "cast", "", ""
        )
        wall_ms = max(1, int((time.time() - start) * 1000))

        self.total_dispatched += len(self.worker_ids)
        self.total_accepted += accepted
        self.total_rejected += rejected
        self.wall_time_ms = wall_ms
        self.dispatch_count += 1

        throughput = accepted * 1000 // wall_ms

        return {
            "accepted": accepted,
            "rejected": rejected,
            "wall_time_ms": wall_ms,
            "throughput_per_s": throughput,
            "total_dispatched": self.total_dispatched,
        }

    @handler("upgrade_workers", "call")
    def upgrade_workers(self, count: int = 10) -> dict:
        """Ask N workers for upgrade status, track Ask RTT."""
        n = min(count, len(self.worker_ids))
        if n == 0:
            return {"error": "no workers"}

        ctx = json.dumps({"tenant": "default", "namespace": "default"})
        workers_asked = 0
        for worker_id in self.worker_ids[:n]:
            t0 = time.time()
            try:
                host.ask(worker_id, "upgrade", {"version": "2.0.0"}, timeout_ms=5000)
            except Exception:
                pass
            rtt_ms = max(1, int((time.time() - t0) * 1000))
            workers_asked += 1

            if self.ask_rtt_count == 0 or rtt_ms < self.ask_rtt_min_ms:
                self.ask_rtt_min_ms = rtt_ms
            if rtt_ms > self.ask_rtt_max_ms:
                self.ask_rtt_max_ms = rtt_ms
            self.ask_rtt_sum_ms += rtt_ms
            self.ask_rtt_count += 1

        avg_ms = self.ask_rtt_sum_ms // max(self.ask_rtt_count, 1)
        return {
            "workers_asked": workers_asked,
            "ask_rtt_min_ms": self.ask_rtt_min_ms,
            "ask_rtt_max_ms": self.ask_rtt_max_ms,
            "ask_rtt_avg_ms": avg_ms,
        }

    @handler("send_heartbeat", "call")
    def send_heartbeat(self, seq: int = 0) -> dict:
        """Send a heartbeat with OVERWRITE idempotency (LWW)."""
        ctx = {"tenant": "default", "namespace": "default"}
        my_id = host.self_id() or "leader:JobQueueLeader"
        payload = {"heartbeat": True, "seq": seq}
        host.outbox.dispatch(
            ctx, DISPATCHER_ID, [my_id], payload, "cast",
            HEARTBEAT_IDEM_KEY, "overwrite"
        )
        self.heartbeat_count += 1
        return {"heartbeat_count": self.heartbeat_count, "seq": seq}

    @handler("scaling_benchmark", "call")
    def scaling_benchmark(self) -> dict:
        """Dispatch at 10/50/100 worker scales and return throughput table.

        Reuses the shard group from init_workers (sliced) to avoid the overhead of
        spawning thousands of new virtual actors inside the benchmark call.
        """
        if not self.worker_ids:
            return {"error": "no workers — call init_workers first"}

        available = self.worker_ids
        scales = [n for n in [10, 50, 100] if n <= len(available)]
        if not scales:
            scales = [len(available)]

        rows = []
        base_throughput = 1
        ctx = {"tenant": "default", "namespace": "default"}

        for n in scales:
            targets = available[:n]
            start = time.time()
            try:
                accepted, _ = host.outbox.dispatch(
                    ctx, DISPATCHER_ID, targets,
                    {"task": "benchmark_job"}, "cast", "", ""
                )
            except Exception:
                accepted = 0
            wall_ms = max(1, int((time.time() - start) * 1000))
            throughput = accepted * 1000 // wall_ms

            if n == scales[0]:
                base_throughput = max(throughput, 1)

            speedup_pct = throughput * 100 // base_throughput
            rows.append({
                "workers": n,
                "accepted": accepted,
                "wall_time_ms": wall_ms,
                "throughput": throughput,
                "speedup_pct": speedup_pct,
            })

        return {"scales": rows}

    # ── UC1-UC5: Flow Control Use Cases ─────────────────────────────────────

    @handler("thundering_herd_test", "call")
    def thundering_herd_test(self, count: int = 0) -> dict:
        """UC1: Dispatch to all workers simultaneously."""
        if not self.worker_ids:
            return {"error": "call init_workers first"}
        targets = self.worker_ids[:count] if 0 < count < len(self.worker_ids) else self.worker_ids
        ctx = {"tenant": "default", "namespace": "default"}
        start = time.time()
        accepted, rejected = host.outbox.dispatch(
            ctx, DISPATCHER_ID, targets,
            {"task": "thundering_herd", "priority": "high"}, "cast", "", ""
        )
        wall_ms = max(1, int((time.time() - start) * 1000))
        return {
            "targets": len(targets),
            "accepted": accepted,
            "rejected": rejected,
            "wall_time_ms": wall_ms,
        }

    @handler("config_wave_deploy", "call")
    def config_wave_deploy(self, wave_size: int = 20) -> dict:
        """UC2: Wave-based dispatch with backpressure."""
        if not self.worker_ids:
            return {"error": "call init_workers first"}
        ctx = {"tenant": "default", "namespace": "default"}
        payload = {"task": "config_download", "version": "2.1.0"}
        total_accepted, total_rejected, wave_count = 0, 0, 0
        waves = []
        for i in range(0, len(self.worker_ids), wave_size):
            targets = self.worker_ids[i:i + wave_size]
            start = time.time()
            accepted, rejected = host.outbox.dispatch(
                ctx, DISPATCHER_ID, targets, payload, "cast", "", ""
            )
            wall_ms = max(1, int((time.time() - start) * 1000))
            total_accepted += accepted
            total_rejected += rejected
            wave_count += 1
            waves.append({"wave": wave_count, "accepted": accepted, "rejected": rejected, "wall_time_ms": wall_ms})
        return {
            "wave_count": wave_count,
            "total_accepted": total_accepted,
            "total_rejected": total_rejected,
            "waves": waves,
        }

    @handler("enhanced_heartbeat", "call")
    def enhanced_heartbeat(self, count: int = 50) -> dict:
        """UC3: Rapid heartbeats with OVERWRITE coalescing."""
        workers = self.worker_ids or ["JobQueueWorker:worker-0"]
        ctx = {"tenant": "default", "namespace": "default"}
        total_accepted, total_rejected = 0, 0
        for i in range(1, count + 1):
            accepted, rejected = host.outbox.dispatch(
                ctx, DISPATCHER_ID, workers,
                {"type": "heartbeat", "seq": i, "cpu": 0.42}, "cast",
                HEARTBEAT_IDEM_KEY, "overwrite"
            )
            total_accepted += accepted
            total_rejected += rejected
        self.heartbeat_count = count
        return {
            "heartbeats_sent": count,
            "total_accepted": total_accepted,
            "total_rejected": total_rejected,
            "worker_count": len(workers),
        }

    @handler("telemetry_test", "call")
    def telemetry_test(self, count: int = 50) -> dict:
        """UC4: Fast-path Tell with outbox fallback."""
        if not self.worker_ids:
            return {"error": "call init_workers first"}
        ctx = {"tenant": "default", "namespace": "default"}
        fast_path, fallback = 0, 0
        for i in range(count):
            target = self.worker_ids[i % len(self.worker_ids)]
            try:
                host.ask(target, "process", {"type": "telemetry", "seq": i}, timeout_ms=2000)
                fast_path += 1
            except Exception:
                host.outbox.dispatch(
                    ctx, DISPATCHER_ID, [target],
                    {"type": "telemetry", "seq": i}, "cast", "", ""
                )
                fallback += 1
        return {
            "total_sent": count,
            "fast_path_success": fast_path,
            "outbox_fallback": fallback,
        }

    @handler("reliable_delivery_test", "call")
    def reliable_delivery_test(self, count: int = 50) -> dict:
        """UC5: Outbox store-and-forward reliability."""
        if not self.worker_ids:
            return {"error": "call init_workers first"}
        ctx = {"tenant": "default", "namespace": "default"}
        targets = [self.worker_ids[i % len(self.worker_ids)] for i in range(count)]
        start = time.time()
        accepted, rejected = host.outbox.dispatch(
            ctx, DISPATCHER_ID, targets,
            {"task": "reliable_delivery_item"}, "cast", "", ""
        )
        wall_ms = max(1, int((time.time() - start) * 1000))
        return {
            "dispatched": count,
            "accepted": accepted,
            "rejected": rejected,
            "wall_time_ms": wall_ms,
        }

    @handler("get_metrics", "call")
    def get_metrics(self) -> dict:
        """Return all tracked metrics."""
        throughput = self.total_accepted * 1000 // max(self.wall_time_ms, 1)
        avg_rtt = self.ask_rtt_sum_ms // max(self.ask_rtt_count, 1)
        return {
            "total_dispatched": self.total_dispatched,
            "total_accepted": self.total_accepted,
            "total_rejected": self.total_rejected,
            "dispatch_count": self.dispatch_count,
            "wall_time_ms": self.wall_time_ms,
            "throughput_per_s": throughput,
            "heartbeat_count": self.heartbeat_count,
            "ask_rtt_min_ms": self.ask_rtt_min_ms,
            "ask_rtt_max_ms": self.ask_rtt_max_ms,
            "ask_rtt_avg_ms": avg_rtt,
            "worker_count": len(self.worker_ids),
            "shard_group_id": self.shard_group_id,
        }


# ============================================================================
# JobQueueWorker — Worker Actor
# ============================================================================

@actor
class JobQueueWorker:
    """Worker actor: receives jobs, responds to upgrade queries."""

    jobs_received: int = state(default=0)
    last_heartbeat_seq: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        host.log("info", "JobQueueWorker initialized")

    @handler("process", "cast")
    def process(self, **kwargs) -> None:
        """Receive a job (fire-and-forget)."""
        self.jobs_received += 1

    @handler("upgrade", "call")
    def upgrade(self, **kwargs) -> dict:
        """Reply with upgrade status."""
        return {
            "status": "need_package",
            "version": "2.0.0",
            "worker_id": host.self_id(),
        }

    @handler("stats", "call")
    def stats(self) -> dict:
        """Return worker statistics."""
        return {
            "jobs_received": self.jobs_received,
            "last_heartbeat_seq": self.last_heartbeat_seq,
        }

    @handler("heartbeat", "cast")
    def heartbeat(self, seq: int = 0, **kwargs) -> None:
        """Accept heartbeat; track highest sequence seen."""
        if seq > self.last_heartbeat_seq:
            self.last_heartbeat_seq = seq
