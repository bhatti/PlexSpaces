# SPDX-License-Identifier: AGPL-3.0-or-later
"""CronSchedulerActor — distributed cron with leader election via DistributedLock.

Single leader across the cluster dispatches due jobs to svc:agent via Channel.
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_first, fire_audit, ask

_SCHEDULE_MS = {
    "every_1m": 60 * 1000,
    "every_5m": 5 * 60 * 1000,
    "every_1h": 3600 * 1000,
    "every_24h": 24 * 3600 * 1000,
}
_CRON_CHANNEL = "cron:pending"


def schedule_to_ms(schedule: str) -> int:
    return _SCHEDULE_MS.get(schedule, _SCHEDULE_MS["every_1h"])


@actor
class CronSchedulerActor:
    """Tick-driven cron with distributed leader election."""

    job_count: int = state(default=0)
    tick_count: int = state(default=0)
    dispatched_count: int = state(default=0)
    tick_interval_ms: int = state(default=60000)
    job_ids: list = state(default_factory=list)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        if args.get("tick_interval_ms"):
            iv = int(args["tick_interval_ms"])
            self.tick_interval_ms = max(1000, min(iv, 3_600_000))
        host.process_groups.join("svc:cron")
        try:
            host.registry.register(
                ctx="", object_id=self.actor_id, object_type="actor", grpc_address="",
                object_category="cron_scheduler", capabilities=["create_job", "trigger_tick"],
            )
        except Exception:
            pass
        host.send_after(self.tick_interval_ms, "tick", {"op": "trigger_tick"})
        host.info(f"CronSchedulerActor init actor_id={self.actor_id} interval_ms={self.tick_interval_ms}")

    @handler("create_job")
    def create_job(self, job_id: str = "", prompt: str = "", schedule: str = "every_1h") -> dict:
        if not job_id:
            return {"error": "job_id is required"}
        interval_ms = schedule_to_ms(schedule)
        job = {
            "job_id": job_id,
            "prompt": prompt,
            "schedule": schedule,
            "interval_ms": interval_ms,
            "created_at": host.now_ms(),
            "last_run_at": 0,
        }
        host.kv.put(f"cron_job:{job_id}", json.dumps(job))
        if job_id not in self.job_ids:
            self.job_ids.append(job_id)
            self.job_count += 1
        fire_audit("cron_job_created", f"job_id={job_id} schedule={schedule}")
        return {"status": "ok", "job_id": job_id, "interval_ms": interval_ms}

    @handler("list_jobs")
    def list_jobs(self) -> dict:
        jobs = []
        for jid in self.job_ids:
            raw = host.kv.get(f"cron_job:{jid}")
            if raw:
                try:
                    jobs.append(json.loads(raw))
                except Exception:
                    pass
        return {"status": "ok", "jobs": jobs, "count": len(jobs)}

    @handler("delete_job")
    def delete_job(self, job_id: str = "") -> dict:
        if not job_id:
            return {"error": "job_id is required"}
        host.kv.delete(f"cron_job:{job_id}")
        if job_id in self.job_ids:
            self.job_ids.remove(job_id)
            self.job_count = max(0, self.job_count - 1)
        return {"status": "ok", "job_id": job_id}

    @handler("trigger_tick")
    def trigger_tick(self) -> dict:
        return self._tick()

    @handler("tick", "cast")
    def tick(self) -> None:
        self._tick()

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "job_count": self.job_count,
                "tick_count": self.tick_count, "dispatched_count": self.dispatched_count}

    def _tick(self) -> dict:
        # DistributedLock: only the elected leader dispatches jobs
        try:
            acquired, _ = host.lock.try_acquire("minihermes", "cron_leader", 90000)
        except Exception:
            acquired = True  # stub: assume leader

        self.tick_count += 1
        dispatched = []
        now = host.now_ms()

        if acquired:
            for job_id in list(self.job_ids):
                raw = host.kv.get(f"cron_job:{job_id}")
                if not raw:
                    continue
                try:
                    job = json.loads(raw)
                    interval = int(job.get("interval_ms", 3_600_000))
                    last_run = int(job.get("last_run_at", 0))
                    if now - last_run >= interval:
                        try:
                            host.channel.send("", _CRON_CHANNEL, "cron_job", {
                                "job_id": job_id,
                                "prompt": job.get("prompt", ""),
                            })
                            job["last_run_at"] = now
                            host.kv.put(f"cron_job:{job_id}", json.dumps(job))
                            dispatched.append(job_id)
                            self.dispatched_count += 1
                        except Exception:
                            pass
                except Exception:
                    pass

            if dispatched:
                fire_audit("cron_tick_dispatched", f"jobs={','.join(dispatched)}")

        # Reschedule next tick
        host.send_after(self.tick_interval_ms, "tick", {"op": "trigger_tick"})
        return {"status": "ok", "tick_count": self.tick_count, "dispatched": dispatched}
