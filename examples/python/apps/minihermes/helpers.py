# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared helpers for MiniHermes actors — service discovery and audit utilities.

Service discovery: we show two patterns side by side.
  - pg_first()       — SDK built-in, finds any member of a named process group.
  - registry_first() — wraps host.registry.discover for capability-aware routing;
                       falls back to pg_first on empty results (startup race).

SDK gap note: the PlexSpaces Python SDK exports `pg_first` directly but does not
yet export a `registry_first` equivalent. The wrapper below fills that gap.
"""

from typing import Any, List, Optional, Tuple
from plexspaces import host, pg_first


def registry_first(category: str, capabilities: Optional[List[str]] = None,
                   fallback_group: Optional[str] = None) -> Tuple[Optional[str], Optional[str]]:
    """Return (object_id, None) for the first registry match, or (None, error).

    Uses host.registry.discover — richer than process groups because callers
    can filter by capability (e.g. only find actors that support 'tool_use').
    Falls back to pg_first when the registry is empty (first-start race).
    """
    try:
        results = host.registry.discover(
            ctx="",
            object_type="actor",
            object_category=category,
            capabilities=capabilities or [],
        )
        if results:
            return results[0].get("object_id", ""), None
    except Exception as e:
        host.debug(f"registry_first({category}): registry error: {e}")

    # Fall back to process group
    if fallback_group:
        actor_id = pg_first(fallback_group)
        if actor_id:
            return actor_id, None
        return None, f"no members in {fallback_group}"
    return None, f"no {category} actor found in registry"


def registry_all(category: str, capabilities: Optional[List[str]] = None) -> List[str]:
    """Return all object IDs matching the given category for fan-out / scatter-gather."""
    try:
        results = host.registry.discover(
            ctx="",
            object_type="actor",
            object_category=category,
            capabilities=capabilities or [],
        )
        return [r.get("object_id", "") for r in results if r.get("object_id")]
    except Exception as e:
        host.debug(f"registry_all({category}): {e}")
        return []


def fire_audit(event_type: str, detail: str) -> None:
    """Fire-and-forget audit event. Failures are logged, never raised."""
    audit_id = pg_first("svc:audit")
    if not audit_id:
        host.debug(f"fire_audit: no audit actor in svc:audit")
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


def truncate_str(s: str, n: int) -> str:
    """Truncate string to at most n characters."""
    return s[:n] + "..." if len(s) > n else s
