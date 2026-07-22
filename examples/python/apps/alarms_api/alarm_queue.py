# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Alarms API Example — Python WASM actor
#
# Demonstrates the Cloudflare Durable Objects alarm() pattern: a RequestQueue
# actor that batches incoming requests and processes them 10 seconds after the
# first write, using a durable alarm that survives actor deactivation.
#
# ## Cloudflare DO vs PlexSpaces Python
#
# | Cloudflare DO                             | PlexSpaces Python                      |
# |-------------------------------------------|----------------------------------------|
# | export class RequestQueue extends DO      | @actor class RequestQueueActor         |
# | this.ctx.storage.get('count')             | self.count  (state field)              |
# | this.ctx.storage.put('count', n)          | self.count = n  (auto-persisted)       |
# | this.ctx.storage.setAlarm(Date.now()+10s) | host.alarm.set(host.now_ms() + 10_000) |
# | this.ctx.storage.getAlarm()               | host.alarm.get()                       |
# | async alarm() { ... }                     | @handler("__alarm__")                  |
# | new Response(JSON.stringify(result))      | return {"status": "ok", ...}           |
# | wrangler.toml [[durable_objects]]         | app-config.toml [[supervisor.children]] |

from __future__ import annotations

import json

from plexspaces import actor, handler, host, state


# ─── RequestQueueActor ─────────────────────────────────────────────────────────


@actor(facets=["virtual_actor", "reminder"])
class RequestQueueActor:
    """Batches incoming requests and processes them when the durable alarm fires.

    State:
        items:             list of enqueued items (auto-persisted via actor state).
        count:             current queue depth.
        total_processed:   lifetime count of processed items.
        total_alarm_fires: lifetime count of alarm firings.
    """

    items: list = state(default_factory=list)
    count: int = state(default=0)
    total_processed: int = state(default=0)
    total_alarm_fires: int = state(default=0)

    @handler("enqueue")
    def enqueue(self, item: object = None) -> dict:
        """Add an item to the queue.

        Sets a durable alarm 10 seconds from now on the FIRST item only.
        Equivalent to Cloudflare DO:
            if (count === 0) { this.ctx.storage.setAlarm(Date.now() + 10_000) }
        """
        was_empty = self.count == 0
        queue_item = {
            "id": self.count + 1,
            "data": item,
            "enqueued_at": host.now_ms(),
        }
        self.items = self.items + [queue_item]
        self.count += 1

        alarm_set = False
        if was_empty:
            # First item: schedule alarm 10 seconds from now.
            fire_at = host.now_ms() + 10_000
            host.alarm.set(fire_at)
            alarm_set = True
            host.info(
                f"RequestQueue: first item queued, alarm set to fire in 10s at {fire_at}"
            )

        return {
            "status": "ok",
            "queued": self.count,
            "item_id": queue_item["id"],
            "alarm_set": alarm_set,
        }

    @handler("status")
    def status(self) -> dict:
        """Return current queue depth and next alarm timestamp.

        Equivalent to Cloudflare DO: this.ctx.storage.getAlarm()
        """
        alarm_at = host.alarm.get()
        return {
            "status": "ok",
            "count": self.count,
            "alarm_at": alarm_at,
            "alarm_set": alarm_at > 0,
            "total_processed": self.total_processed,
            "total_alarm_fires": self.total_alarm_fires,
        }

    @handler("reset")
    def reset(self) -> dict:
        """Clear queue and cancel pending alarm (for test repeatability)."""
        self.items = []
        self.count = 0
        host.alarm.delete()
        host.info("RequestQueue: queue reset")
        return {"status": "ok", "reset": True}

    @handler("__alarm__")
    def on_alarm(self) -> dict:
        """Process all queued items when the alarm fires.

        Equivalent to Cloudflare DO: async alarm() { ... }
        Delivered by the PlexSpaces reminder facet as the "__alarm__" message type.
        """
        processed = self.count
        self.total_alarm_fires += 1
        self.total_processed += processed

        host.info(f"RequestQueue: alarm fired, processing {processed} queued items")
        for item in self.items:
            host.info(
                f"RequestQueue: processing item {item['id']}: {json.dumps(item['data'])}"
            )

        # Clear the queue after processing
        self.items = []
        self.count = 0

        return {
            "status": "ok",
            "processed": processed,
            "total_processed": self.total_processed,
            "total_alarm_fires": self.total_alarm_fires,
        }


__all__ = ["RequestQueueActor"]
