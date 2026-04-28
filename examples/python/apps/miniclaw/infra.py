# SPDX-License-Identifier: AGPL-3.0-or-later
"""TaskQueueActor — Channel-backed durable task queue.

HealthMonitorActor — periodic process-group membership polling.
"""

from plexspaces import actor, state, handler, init_handler, host
from .helpers import fire_audit

_TASK_CHANNEL = "tasks:pending"
_SERVICE_GROUPS = [
    "svc:llm_router",
    "svc:tool_registry",
    "svc:agent",
    "svc:session_manager",
    "svc:memory",
    "svc:audit",
    "svc:agent_fsm",
    "svc:task_queue",
]


# ---------------------------------------------------------------------------
# TaskQueueActor
# ---------------------------------------------------------------------------


@actor
class TaskQueueActor:
    """Thin actor wrapper around the host Channel primitive.

    Producers call "enqueue"; consumers call "dequeue", then "ack" or "nack"
    each message-id after processing.  The Channel handles durability and
    redelivery transparently.
    """

    enqueued: int = state(default=0)
    completed: int = state(default=0)
    failed: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:task_queue")
        host.info(f"TaskQueueActor init actor_id={self.actor_id}")

    @handler("enqueue")
    def enqueue(self, task_type: str = "generic", payload: dict = None) -> dict:
        task = {"task_type": task_type, "payload": payload or {}, "enqueued_at": host.now_ms()}
        try:
            msg_id = host.channel.send("", _TASK_CHANNEL, task_type, task)
        except Exception as e:
            host.warn(f"TaskQueue: channel send failed: {e}")
            return {"error": "failed to enqueue task"}
        self.enqueued += 1
        fire_audit("task_enqueued", f"msg_id={msg_id} type={task_type}")
        return {"status": "ok", "msg_id": msg_id}

    @handler("dequeue")
    def dequeue(self, limit: int = 1, timeout_ms: int = 0) -> dict:
        tasks = []
        for _ in range(int(limit)):
            try:
                msg, ok, _ = host.channel.receive("", _TASK_CHANNEL, int(timeout_ms))
                if not ok:
                    break
                tasks.append(msg)
            except Exception as e:
                host.warn(f"TaskQueue: channel receive failed: {e}")
                break
        return {"status": "ok", "tasks": tasks, "count": len(tasks)}

    @handler("ack")
    def ack(self, msg_id: str = "") -> dict:
        if not msg_id:
            return {"error": "msg_id is required"}
        try:
            host.channel.ack("", _TASK_CHANNEL, msg_id)
        except Exception as e:
            return {"error": f"ack failed: {e}"}
        self.completed += 1
        fire_audit("task_completed", f"msg_id={msg_id}")
        return {"status": "ok", "msg_id": msg_id}

    @handler("nack")
    def nack(self, msg_id: str = "", requeue: bool = True) -> dict:
        if not msg_id:
            return {"error": "msg_id is required"}
        try:
            host.channel.nack("", _TASK_CHANNEL, msg_id, requeue)
        except Exception as e:
            return {"error": f"nack failed: {e}"}
        self.failed += 1
        fire_audit("task_failed", f"msg_id={msg_id} requeue={requeue}")
        return {"status": "ok", "msg_id": msg_id, "requeue": requeue}

    @handler("get_stats")
    def get_stats(self) -> dict:
        try:
            depth = host.channel.depth("", _TASK_CHANNEL)
        except Exception:
            depth = 0
        return {
            "status": "ok",
            "enqueued": self.enqueued,
            "completed": self.completed,
            "failed": self.failed,
            "depth": depth,
        }


# ---------------------------------------------------------------------------
# HealthMonitorActor
# ---------------------------------------------------------------------------


@actor
class HealthMonitorActor:
    """Polls process group membership on a fixed interval using send_after.

    Polling eliminates event-subscription races and is correct when
    sub-second latency is not required.
    """

    poll_count: int = state(default=0)
    last_poll_ms: int = state(default=0)
    group_health: dict = state(default_factory=dict)
    poll_interval_ms: int = state(default=5000)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        if args.get("poll_interval_ms"):
            iv = int(args["poll_interval_ms"])
            self.poll_interval_ms = min(max(iv, 1000), 300_000)
        host.process_groups.join("svc:health_monitor")
        host.send_after(self.poll_interval_ms, "poll_tick", {"op": "poll_tick"})
        host.info(f"HealthMonitorActor init actor_id={self.actor_id} interval_ms={self.poll_interval_ms}")

    @handler("poll_tick", "cast")
    def poll_tick(self) -> None:
        health = {}
        for grp in _SERVICE_GROUPS:
            try:
                members = host.process_groups.members(grp)
                health[grp] = len(members)
            except Exception:
                health[grp] = 0
        self.group_health = health
        self.poll_count += 1
        self.last_poll_ms = host.now_ms()
        try:
            import json
            host.ts.write(["health_snapshot", self.last_poll_ms, json.dumps(health)])
        except Exception:
            pass
        host.send_after(self.poll_interval_ms, "poll_tick", {"op": "poll_tick"})

    @handler("get_health")
    def get_health(self) -> dict:
        degraded = [g for g, c in self.group_health.items() if c == 0]
        return {
            "status": "ok",
            "group_health": self.group_health,
            "healthy": len(self.group_health) - len(degraded),
            "degraded": degraded,
            "last_poll_ms": self.last_poll_ms,
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "poll_count": self.poll_count,
            "last_poll_ms": self.last_poll_ms,
            "group_health": self.group_health,
        }
