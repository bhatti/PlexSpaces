# SPDX-License-Identifier: AGPL-3.0-or-later
"""MemoryActor — scoped KV + TupleSpace memory store.

AuditEventActor — GenEvent fire-and-forget audit trail.
AgentStateFSM — agent lifecycle state machine.
"""

from plexspaces import actor, event_actor, fsm_actor, state, handler, init_handler, host
from .helpers import fire_audit

_VALID_FSM_TRANSITIONS = {
    "idle": {"processing", "tool_executing"},
    "processing": {"tool_executing", "responding", "idle"},
    "tool_executing": {"processing", "idle"},
    "responding": {"idle"},
}


# ---------------------------------------------------------------------------
# MemoryActor
# ---------------------------------------------------------------------------


@actor
class MemoryActor:
    """Scoped memory backed by KV (persistent) and TupleSpace (queryable).

    Supports three scopes: global, agent-scoped, and session-scoped.
    """

    memory_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:memory")
        host.info(f"MemoryActor init actor_id={self.actor_id}")

    @handler("store_memory")
    def store_memory(self, key: str = "", value: str = "", scope: str = "global", agent_id: str = "", session_id: str = "") -> dict:
        if not key:
            return {"error": "key is required"}
        scoped_key = _scoped_key(scope, agent_id, session_id, key)
        host.kv.put(scoped_key, str(value))
        host.ts.write(["memory", scope, key, str(value)])
        self.memory_count += 1
        fire_audit("memory_stored", f"scope={scope} key={key}")
        return {"status": "ok", "key": key, "scope": scope}

    @handler("recall_memory")
    def recall_memory(self, key: str = "", scope: str = "global", agent_id: str = "", session_id: str = "") -> dict:
        if not key:
            return {"error": "key is required"}
        scoped_key = _scoped_key(scope, agent_id, session_id, key)
        value = host.kv.get(scoped_key)
        return {"status": "ok", "key": key, "value": value, "found": bool(value)}

    @handler("list_memories")
    def list_memories(self, scope: str = "global") -> dict:
        # TupleSpace query: find all ["memory", scope, ?, ?] tuples
        try:
            tuples = host.ts.read_all(["memory", scope, None, None])
            memories = [{"key": t[2], "value": t[3]} for t in tuples if len(t) >= 4]
        except Exception:
            memories = []
        return {"status": "ok", "memories": memories, "scope": scope}

    @handler("delete_memory")
    def delete_memory(self, key: str = "", scope: str = "global", agent_id: str = "", session_id: str = "") -> dict:
        if not key:
            return {"error": "key is required"}
        scoped_key = _scoped_key(scope, agent_id, session_id, key)
        host.kv.delete(scoped_key)
        return {"status": "ok", "key": key}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "memory_count": self.memory_count}


def _scoped_key(scope: str, agent_id: str, session_id: str, key: str) -> str:
    if scope == "agent" and agent_id:
        return f"mem:agent:{agent_id}:{key}"
    if scope == "session" and session_id:
        return f"mem:session:{session_id}:{key}"
    return f"mem:global:{key}"


# ---------------------------------------------------------------------------
# AuditEventActor
# ---------------------------------------------------------------------------


@event_actor
class AuditEventActor:
    """GenEvent actor: receives fire-and-forget audit events and stores them in TupleSpace."""

    event_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:audit")
        host.info(f"AuditEventActor init actor_id={self.actor_id}")

    @handler("log_event", "cast")
    def log_event(self, event_type: str = "", detail: str = "", timestamp: int = 0) -> None:
        ts = timestamp or host.now_ms()
        try:
            host.ts.write(["audit", event_type, ts, detail])
        except Exception as e:
            host.warn(f"AuditEvent: ts.write failed: {e}")
        self.event_count += 1
        host.debug(f"audit event_type={event_type} detail={detail}")

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "event_count": self.event_count}


# ---------------------------------------------------------------------------
# AgentStateFSM
# ---------------------------------------------------------------------------


@fsm_actor(states=["idle", "processing", "tool_executing", "responding"], initial="idle")
class AgentStateFSM:
    """Agent lifecycle FSM: idle → processing → tool_executing → responding → idle."""

    fsm_state: str = state(default="idle")
    transition_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:agent_fsm")
        host.info(f"AgentStateFSM init actor_id={self.actor_id}")

    @handler("transition")
    def transition(self, to: str = "") -> dict:
        allowed = _VALID_FSM_TRANSITIONS.get(self.fsm_state, set())
        if to not in allowed:
            host.debug(f"FSM: invalid transition {self.fsm_state} → {to}")
            return {"status": "ignored", "from": self.fsm_state, "to": to}
        prev = self.fsm_state
        self.fsm_state = to
        self.transition_count += 1
        host.debug(f"FSM: {prev} → {to}")
        return {"status": "ok", "from": prev, "to": to}

    @handler("get_state")
    def get_state(self) -> dict:
        return {"status": "ok", "state": self.fsm_state, "transitions": self.transition_count}
