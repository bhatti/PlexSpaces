# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared helpers for multi-agent coordination actors."""

from typing import Any, Optional, Tuple
from plexspaces import host


def pg_first(group: str) -> Tuple[Optional[str], Optional[str]]:
    """Return (actor_id, None) for the first member of a process group, or (None, error)."""
    try:
        members = host.process_groups.members(group)
        if members:
            return members[0], None
        return None, f"no members in {group}"
    except Exception as e:
        return None, str(e)


def fire_audit(event_type: str, source: str, data: dict = None) -> None:
    """Broadcast coordination event to the coordination-events process group."""
    try:
        host.process_groups.broadcast(
            "coordination-events",
            "coordination_event",
            {
                "op": "coordination_event",
                "event_type": event_type,
                "source": source,
                "data": data or {},
                "timestamp": host.now_ms(),
            },
        )
    except Exception as e:
        host.debug(f"fire_audit: broadcast failed: {e}")


def discover_service(role: str) -> Optional[str]:
    """Discover a service actor via TupleSpace registration."""
    try:
        tup = host.ts.read(["svc", role, None])
        if tup and len(tup) >= 3:
            return str(tup[2])
    except Exception:
        pass
    return None


def sibling_actor_target(role: str) -> str:
    """Discover sibling actor by role, falling back to ActorID construction."""
    discovered = discover_service(role)
    if discovered:
        return discovered
    from plexspaces import ActorID
    self_id = host.self_id()
    try:
        return str(ActorID.parse(self_id).with_name(role))
    except Exception:
        return role


def ask(actor_id: str, op: str, payload: dict, timeout_ms: int = 5000) -> Any:
    """Ask another actor and return parsed response, or None on error."""
    try:
        return host.ask(actor_id, op, {"op": op, **payload}, timeout_ms)
    except Exception as e:
        host.debug(f"ask({actor_id}, {op}): {e}")
        return None
