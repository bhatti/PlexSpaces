# SPDX-License-Identifier: AGPL-3.0-or-later
"""AuditEventActor — fire-and-forget event logging via GenEvent.

Demonstrates: Pub-Sub (logs all coordination events received via process group).
"""

import json
from plexspaces import event_actor, state, handler, init_handler, host


@event_actor
class AuditEventActor:
    """GenEvent: receives coordination events and stores them in KV for audit trail."""

    log_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.process_groups.join("coordination-events")
        except Exception:
            pass
        host.info(f"AuditEventActor init actor_id={self.actor_id}")

    @handler("coordination_event", "cast")
    def coordination_event(
        self,
        event_type: str = "",
        source: str = "",
        data: dict = None,
        timestamp: int = 0,
    ) -> None:
        ts = timestamp or host.now_ms()
        entry = {
            "event_type": event_type,
            "source": source,
            "data": data or {},
            "timestamp": ts,
        }
        try:
            self.log_count += 1
            key = f"audit:{self.log_count}"
            host.kv.put(key, json.dumps(entry))
            host.kv.put("audit:count", str(self.log_count))
        except Exception as e:
            host.warn(f"AuditEvent: kv write failed: {e}")
        host.debug(f"audit event_type={event_type} source={source}")

    @handler("get_audit_log")
    def get_audit_log(self, limit: int = 20) -> dict:
        entries = []
        count_str = host.kv.get("audit:count")
        total = int(count_str) if count_str else self.log_count

        start = max(1, total - limit + 1)
        for i in range(start, total + 1):
            raw = host.kv.get(f"audit:{i}")
            if raw:
                try:
                    entries.append(json.loads(raw))
                except (json.JSONDecodeError, ValueError):
                    entries.append({"raw": raw})

        return {"entries": entries, "total_count": total}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"log_count": self.log_count}
