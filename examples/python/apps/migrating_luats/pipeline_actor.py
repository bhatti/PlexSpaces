# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# LuaTS → PlexSpaces: Event-driven data pipeline (CDC-style).
#
# Linda-style coordination: events as tuples in TupleSpace; consumers take/read by pattern.
# Tuple fields must be primitives supported by the runtime (no dict/list — use JSON strings).
# Run: write event stream to tuple space, then take events by pattern and aggregate (coord vs compute).
# Publish: write single event tuple. Window flush: take events in window and aggregate (timer-style).

import json

from plexspaces import workflow_actor, state, handler, host

TUPLE_PREFIX = "cdc"
EVENT_TAG = "event"
COMPUTE_MS_PER_EVENT = 0.3
WINDOW_FLUSH_DELAY_MS = 500
MAX_TAKE_POLL = 2000


@workflow_actor(facets=["virtual_actor", "durability"])
class PipelineActor:
    """
    CDC-style event pipeline: TupleSpace as event buffer, Linda-style take/read by pattern.
    run(): write N events as tuples, then take by pattern and aggregate (metrics: coord vs compute).
    publish: write one event tuple. window_flush: take events in window, aggregate (timer callback).
    """

    pipeline_id: str = state(default="")
    events_written: int = state(default=0)
    events_processed: int = state(default=0)
    status: str = state(default="idle")
    total_compute_ms: float = state(default=0.0)
    total_coord_ms: float = state(default=0.0)
    created_at_ms: float = state(default=0.0)
    updated_at_ms: float = state(default=0.0)
    cancel_requested: bool = state(default=False)
    window_aggregate_count: int = state(default=0)

    def run(self, payload: dict) -> dict:
        """Run pipeline: write event stream to tuple space, then take by pattern and aggregate."""
        t0 = host.now_ms()
        if self.created_at_ms == 0:
            self.created_at_ms = float(t0)
        self.pipeline_id = str(payload.get("pipeline_id") or payload.get("run_id") or "run-1")
        self.updated_at_ms = float(host.now_ms())

        if self.cancel_requested:
            self.status = "cancelled"
            return self._finish(t0, 0.0, "cancelled")

        if self.status == "completed":
            return self._finish(t0, 0.0, "completed")

        num_events = int(payload.get("num_events", 200))
        if num_events <= 0:
            return self._finish(t0, 0.0, "no_events")

        self.events_written = 0
        self.events_processed = 0
        self.status = "running"
        compute_ms = 0.0
        coord_start = host.now_ms()

        # TupleSpace: write event stream (coord)
        for seq in range(num_events):
            if self.cancel_requested:
                self.status = "cancelled"
                return self._finish(t0, compute_ms, "cancelled")
            source = payload.get("source", "ingest")
            event_body = json.dumps({"ts": host.now_ms(), "seq": seq})
            t = [TUPLE_PREFIX, self.pipeline_id, EVENT_TAG, source, seq, event_body]
            out = host.ts.write(t)
            if out and out.startswith("ERROR"):
                host.log("warn", f"ts write failed: {out}")
                break
            self.events_written += 1

        coord_elapsed = float(host.now_ms() - coord_start)
        self.total_coord_ms += coord_elapsed

        # Take events by pattern and "process" (compute)
        pattern = [TUPLE_PREFIX, self.pipeline_id, EVENT_TAG, None, None, None]
        processed = 0
        for _ in range(MAX_TAKE_POLL):
            if self.cancel_requested:
                self.status = "cancelled"
                return self._finish(t0, compute_ms, "cancelled")
            taken = host.ts.take(pattern)
            if taken is None:
                if processed > 0:
                    break
                self.updated_at_ms = host.now_ms()
                continue
            processed += 1
            self.events_processed += 1
            compute_ms += COMPUTE_MS_PER_EVENT

        self.status = "completed"
        self.updated_at_ms = host.now_ms()
        return self._finish(t0, compute_ms, "completed")

    def _finish(self, t0: float, compute_ms: float, status: str) -> dict:
        elapsed = float(host.now_ms() - t0)
        if elapsed < compute_ms:
            elapsed = compute_ms
        coord_ms = max(0.0, elapsed - compute_ms)
        self.total_compute_ms += compute_ms
        self.total_coord_ms += coord_ms
        return {
            "status": status,
            "pipeline_id": self.pipeline_id,
            "events_written": self.events_written,
            "events_processed": self.events_processed,
            "total_compute_ms": self.total_compute_ms,
            "total_coord_ms": self.total_coord_ms,
        }

    def signal(self, name: str, data: dict) -> None:
        if name == "cancel":
            self.cancel_requested = True
            self.updated_at_ms = host.now_ms()

    def query(self, name: str, params: dict) -> dict:
        if name == "status":
            return {
                "pipeline_id": self.pipeline_id,
                "status": self.status,
                "events_written": self.events_written,
                "events_processed": self.events_processed,
                "cancel_requested": self.cancel_requested,
                "total_compute_ms": self.total_compute_ms,
                "total_coord_ms": self.total_coord_ms,
                "created_at_ms": self.created_at_ms,
                "updated_at_ms": self.updated_at_ms,
            }
        return {"error": "unknown_query", "name": name}

    @handler("publish")
    def on_publish(
        self,
        source: str = "",
        seq: int = 0,
        payload: dict = None,
        **kwargs,
    ) -> dict:
        """Write one CDC event tuple to tuple space (Linda-style)."""
        if payload is None:
            payload = {}
        pipeline_id = kwargs.get("pipeline_id") or self.pipeline_id or "default"
        body = payload if isinstance(payload, str) else json.dumps(payload or {})
        t = [TUPLE_PREFIX, pipeline_id, EVENT_TAG, source or "api", seq, body]
        out = host.ts.write(t)
        if out and out.startswith("ERROR"):
            return {"ok": False, "error": out}
        self.events_written += 1
        self.updated_at_ms = host.now_ms()
        return {"ok": True, "source": source or "api", "seq": seq}

    @handler("window_flush")
    def on_window_flush(
        self,
        pipeline_id: str = "",
        **kwargs,
    ) -> dict:
        """Timer-style: take events in window from tuple space and aggregate (Linda pattern)."""
        pid = pipeline_id or self.pipeline_id or "default"
        pattern = [TUPLE_PREFIX, pid, EVENT_TAG, None, None, None]
        count = 0
        compute_ms = 0.0
        t0 = host.now_ms()
        for _ in range(MAX_TAKE_POLL):
            taken = host.ts.take(pattern)
            if taken is None:
                break
            count += 1
            compute_ms += COMPUTE_MS_PER_EVENT
        self.window_aggregate_count += count
        self.events_processed += count
        self.total_compute_ms += compute_ms
        self.total_coord_ms += max(0.0, float(host.now_ms() - t0) - compute_ms)
        self.updated_at_ms = host.now_ms()
        return {"ok": True, "taken": count, "window_total": self.window_aggregate_count}
