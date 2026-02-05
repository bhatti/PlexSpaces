"""
Audit Log Actor - Event-handler example (Python WASM).

GenEvent-style: fire-and-forget audit events. Logs each event via host.log
only (no ts_write/kv_put) to avoid WASM integration issues; API support
can be added when the WASM runtime is more stable.
"""

import json
from plexspaces import event_actor, handler, init_handler, host


@event_actor
class AuditLog:
    """Fire-and-forget audit events; each event is logged via host.log."""

    @init_handler
    def on_init(self, config: dict) -> None:
        host.log("info", "AuditLog init")

    @handler("log")
    def log_event(
        self,
        action: str = "",
        actor_id: str = "",
        resource: str = "",
        details: str = "",
        ts: str = "",
    ) -> str:
        """Append one audit event. Fire-and-forget; logs via host.log only."""
        msg = f"audit action={action or 'unknown'} actor_id={actor_id or ''} resource={resource or ''} details={details or ''} ts={ts or ''}"
        host.log("info", msg)
        return "ok"
