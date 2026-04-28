# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared helpers for MiniClaw actors — utilities for service discovery and audit."""

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


def fire_audit(event_type: str, detail: str) -> None:
    """Fire-and-forget audit event. Failures are logged, never raised."""
    audit_id, err = pg_first("svc:audit")
    if err or not audit_id:
        host.debug(f"fire_audit: {err}")
        return
    try:
        host.send(audit_id, "log_event", {
            "op": "log_event",
            "event_type": event_type,
            "detail": detail,
            "timestamp": host.now_ms(),
        })
    except Exception as e:
        host.warn(f"fire_audit: send failed: {e}")


def write_actor_info(actor_id: str, name: str, description: str, capabilities: list) -> None:
    """Write actor capability tuples to TupleSpace for discovery."""
    try:
        host.ts.write(["agent_card", actor_id, name, description])
        for cap in capabilities:
            host.ts.write(["agent_cap", cap, actor_id])
    except Exception as e:
        host.warn(f"write_actor_info: {e}")


def ask(actor_id: str, op: str, payload: dict, timeout_ms: int = 5000) -> Any:
    """Ask another actor and return the parsed response dict, or None on error."""
    try:
        return host.ask(actor_id, op, {"op": op, **payload}, timeout_ms)
    except Exception as e:
        host.debug(f"ask({actor_id}, {op}): {e}")
        return None
