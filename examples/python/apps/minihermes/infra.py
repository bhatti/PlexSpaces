# SPDX-License-Identifier: AGPL-3.0-or-later
"""SessionManagerActor — KV-backed session lifecycle.
HealthMonitorActor — periodic registry + process-group health polling.
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_all, fire_audit

_HERMES_SERVICE_GROUPS = [
    "svc:llm_gateway", "svc:tools", "svc:agent", "svc:skills",
    "svc:memory", "svc:compressor", "svc:cron", "svc:sessions",
    "svc:guardrails", "svc:audit",
]


@actor
class SessionManagerActor:
    """Manages agent session lifecycle, backed by KV and TupleSpace."""

    active_sessions: int = state(default=0)
    total_created: int = state(default=0)
    session_ids: list = state(default_factory=list)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:sessions")
        try:
            host.registry.register(
                ctx="", object_id=self.actor_id, object_type="actor", grpc_address="",
                object_category="session_manager", capabilities=["create_session"],
            )
        except Exception:
            pass
        host.info(f"SessionManagerActor init actor_id={self.actor_id}")

    @handler("create_session")
    def create_session(self, channel: str = "web", user_id: str = "anonymous") -> dict:
        session_id = f"sess-{channel}-{user_id}-{host.now_ms()}"
        meta = {"session_id": session_id, "channel": channel, "user_id": user_id,
                "created_at": host.now_ms(), "status": "active"}
        host.kv.put(f"session:{session_id}", json.dumps(meta))
        try:
            host.ts.write(["session_active", session_id, channel, user_id])
        except Exception:
            pass
        if session_id not in self.session_ids:
            self.session_ids.append(session_id)
            self.active_sessions += 1
            self.total_created += 1
        fire_audit("session_created", f"session_id={session_id}")
        return {"status": "ok", "session_id": session_id}

    @handler("get_session")
    def get_session(self, session_id: str = "") -> dict:
        if not session_id:
            return {"error": "session_id is required"}
        raw = host.kv.get(f"session:{session_id}")
        if not raw:
            return {"error": "session not found", "session_id": session_id}
        meta = json.loads(raw)
        meta["status_field"] = "ok"
        meta["session_id"] = session_id
        return meta

    @handler("end_session")
    def end_session(self, session_id: str = "") -> dict:
        if not session_id:
            return {"error": "session_id is required"}
        host.kv.delete(f"session:{session_id}")
        if session_id in self.session_ids:
            self.session_ids.remove(session_id)
            self.active_sessions = max(0, self.active_sessions - 1)
        fire_audit("session_ended", f"session_id={session_id}")
        return {"status": "ok", "session_id": session_id}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "active_sessions": self.active_sessions,
                "total_created": self.total_created}


# ──────────────────────────────────────────────────────────────────────────────


@actor
class HealthMonitorActor:
    """Polls process groups and object registry for service health.

    Uses two complementary discovery mechanisms side by side:
    - host.process_groups.members(): fast PG membership count
    - host.registry.discover(): rich capability-aware actor list
    """

    poll_count: int = state(default=0)
    last_poll_ms: int = state(default=0)
    group_health: dict = state(default_factory=dict)
    poll_interval_ms: int = state(default=10000)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        if args.get("poll_interval_ms"):
            iv = int(args["poll_interval_ms"])
            self.poll_interval_ms = max(1000, min(iv, 300_000))
        host.process_groups.join("svc:health_monitor")
        host.send_after(self.poll_interval_ms, "poll_tick", {"op": "poll_tick"})
        host.info(f"HealthMonitorActor init actor_id={self.actor_id} interval_ms={self.poll_interval_ms}")

    @handler("poll_tick", "cast")
    def poll_tick(self) -> None:
        self._do_poll()

    @handler("trigger_poll_tick")
    def trigger_poll_tick(self) -> dict:
        return self._do_poll()

    @handler("get_health")
    def get_health(self) -> dict:
        degraded = [g for g, c in self.group_health.items() if c == 0]
        return {"status": "ok", "group_health": self.group_health,
                "healthy": len(self.group_health) - len(degraded),
                "degraded": degraded, "last_poll_ms": self.last_poll_ms}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "poll_count": self.poll_count,
                "last_poll_ms": self.last_poll_ms, "group_health": self.group_health}

    def _do_poll(self) -> dict:
        health = {}
        for grp in _HERMES_SERVICE_GROUPS:
            try:
                members = host.process_groups.members(grp)
                health[grp] = len(members)
            except Exception:
                health[grp] = 0

        # Also show registry counts for "agent" and "skill_store" categories
        for cat in ("agent", "skill_store", "llm_gateway"):
            try:
                actors = host.registry.discover(ctx="", object_type="actor", object_category=cat)
                health[f"registry:{cat}"] = len(actors)
            except Exception:
                health[f"registry:{cat}"] = 0

        self.group_health = health
        self.poll_count += 1
        self.last_poll_ms = host.now_ms()
        try:
            host.ts.write(["health_snapshot", self.last_poll_ms, json.dumps(health)])
        except Exception:
            pass
        host.send_after(self.poll_interval_ms, "poll_tick", {"op": "poll_tick"})
        return {"status": "ok", "poll_count": self.poll_count, "group_health": health}
