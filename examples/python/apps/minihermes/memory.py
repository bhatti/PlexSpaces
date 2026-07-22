# SPDX-License-Identifier: AGPL-3.0-or-later
"""MemoryActor — three-tier memory (core / reachable / deep).
AuditEventActor — watermark-based audit trail with two-cursor polling.
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import fire_audit


@actor
class MemoryActor:
    """Tiered memory: core (KV, no TTL), reachable (KV+TTL), deep (Blob+TupleSpace)."""

    core_count: int = state(default=0)
    reachable_count: int = state(default=0)
    deep_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:memory")
        try:
            host.registry.register(
                ctx="", object_id=self.actor_id, object_type="actor", grpc_address="",
                object_category="memory", capabilities=["store_memory", "recall_memory"],
            )
        except Exception:
            pass
        host.info(f"MemoryActor init actor_id={self.actor_id}")

    @handler("store_memory")
    def store_memory(self, tier: str = "core", key: str = "", value: str = "",
                     scope: str = "global", scope_id: str = "default") -> dict:
        if not key:
            return {"error": "key is required"}
        full_key = f"mem:{tier}:{scope}:{scope_id}:{key}"
        host.kv.put(full_key, value)
        try:
            host.ts.write(["memory", tier, scope, scope_id, key, value[:50]])
        except Exception:
            pass
        if tier == "core":
            self.core_count += 1
        elif tier == "reachable":
            self.reachable_count += 1
        elif tier == "deep":
            try:
                host.blob.upload("memories", f"deep_{scope}_{key}", value.encode())
            except Exception:
                pass
            self.deep_count += 1
        fire_audit("memory_stored", f"tier={tier} key={key}")
        return {"status": "ok", "tier": tier, "key": key}

    @handler("recall_memory")
    def recall_memory(self, query: str = "", scope: str = "global", scope_id: str = "default",
                      tier: str = "") -> dict:
        results = []
        search_tiers = [tier] if tier else ["core", "reachable", "deep"]
        for t in search_tiers:
            try:
                tuples = host.ts.read_all(["memory", t, scope, scope_id, None, None])
                for tup in tuples or []:
                    if len(tup) >= 6:
                        k, v = str(tup[4]), str(tup[5])
                        if not query or query.lower() in k.lower() or query.lower() in v.lower():
                            results.append({"tier": t, "key": k, "value": v})
            except Exception:
                pass
        return {"status": "ok", "memories": results, "count": len(results)}

    @handler("delete_memory")
    def delete_memory(self, tier: str = "core", key: str = "", scope: str = "global",
                      scope_id: str = "default") -> dict:
        if not key:
            return {"error": "key is required"}
        host.kv.delete(f"mem:{tier}:{scope}:{scope_id}:{key}")
        return {"status": "ok", "tier": tier, "key": key}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "core_count": self.core_count,
                "reachable_count": self.reachable_count, "deep_count": self.deep_count}


# ──────────────────────────────────────────────────────────────────────────────


@actor
class AuditEventActor:
    """Monotonic watermark audit log. Two-cursor polling: each consumer tracks its own cursor."""

    watermark: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:audit")
        host.info(f"AuditEventActor init actor_id={self.actor_id}")

    @handler("log_event", "cast")
    def log_event(self, event_type: str = "", detail: str = "", timestamp: int = 0) -> None:
        self.watermark += 1
        seq = self.watermark
        entry = {"seq": seq, "event_type": event_type, "detail": detail,
                 "timestamp": timestamp or host.now_ms()}
        host.kv.put(f"audit:seq:{seq}", json.dumps(entry))
        try:
            host.ts.write(["audit_event", event_type, seq, detail[:80]])
        except Exception:
            pass

    @handler("poll_events")
    def poll_events(self, consumer_id: str = "default", limit: int = 20) -> dict:
        cursor_key = f"audit:cursor:{consumer_id}"
        cursor_raw = host.kv.get(cursor_key)
        cursor = int(cursor_raw) if cursor_raw else 0

        events = []
        for seq in range(cursor + 1, self.watermark + 1):
            raw = host.kv.get(f"audit:seq:{seq}")
            if raw:
                try:
                    events.append(json.loads(raw))
                except Exception:
                    pass
            if len(events) >= int(limit):
                break

        if events:
            new_cursor = events[-1]["seq"]
            host.kv.put(cursor_key, str(new_cursor))

        return {"status": "ok", "events": events, "count": len(events),
                "cursor": events[-1]["seq"] if events else cursor,
                "watermark": self.watermark}
