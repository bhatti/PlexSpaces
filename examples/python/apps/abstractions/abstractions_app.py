#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Deployable abstractions example for the PlexSpaces Python SDK."""

from __future__ import annotations

from plexspaces import (
    gen_server_actor,
    handler,
    host,
    init_handler,
    query_handler,
    run_handler,
    signal_handler,
    state,
    workflow_actor,
)


DEFAULT_GROUP = "abstractions-group"


def application_id_from_actor_id(actor_id: str) -> str:
    if "//" in actor_id and "::" in actor_id:
        suffix = actor_id.split("//", 1)[1]
        qualified = suffix.split("@", 1)[0]
        parts = qualified.split("::", 1)
        if len(parts) == 2:
            return parts[1]
    if ":" in actor_id and "@" in actor_id:
        return actor_id.split(":", 1)[1].split("@", 1)[0]
    return ""


def canonical_actor_target(target: str) -> str:
    return target


@gen_server_actor(facets=["virtual_actor", "durability", "timer", "reminder"])
class AbstractionsActor:
    application_id: str = state(default="")
    actor_id: str = state(default="")
    role: str = state(default="abstractions")
    count: int = state(default=0)
    workflow_status: str = state(default="")
    workflow_signals: list[str] = state(default_factory=list)
    received: list[str] = state(default_factory=list)
    timer_ticks: int = state(default=0)
    reminder_ticks: int = state(default=0)
    joined_group: str = state(default="")
    last_spawned_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = application_id_from_actor_id(self.actor_id)
        args = config.get("args", {})
        self.role = args.get("role", "abstractions")
        self.count = int(args.get("initial_count", 0) or 0)
        self.workflow_status = ""
        self.workflow_signals = []
        self.received = []
        self.timer_ticks = 0
        self.reminder_ticks = 0
        self.last_spawned_id = ""
        self.joined_group = ""
        if self.role == "channel":
            group = args.get("group", DEFAULT_GROUP)
            host.process_groups.join(group)
            self.joined_group = group

    @handler("increment")
    def increment(self, amount: int = 1) -> dict:
        self.count += amount
        return {"actor_id": self.actor_id, "count": self.count}

    @handler("status")
    def status(self) -> dict:
        return {
            "actor_id": self.actor_id,
            "application_id": self.application_id,
            "count": self.count,
            "joined_group": self.joined_group,
            "last_spawned_id": self.last_spawned_id,
            "received": list(self.received),
            "reminder_ticks": self.reminder_ticks,
            "role": self.role,
            "self_id": host.self_id(),
            "timer_ticks": self.timer_ticks,
            "workflow_signals": list(self.workflow_signals),
            "workflow_status": self.workflow_status,
        }

    @handler("schedule_timer")
    def schedule_timer(self, delay_ms: int = 100) -> dict:
        return {"timer_id": host.send_after(delay_ms, "tick", {"kind": "timer"})}

    @handler("schedule_reminder")
    def schedule_reminder(self, delay_ms: int = 150) -> dict:
        return {"reminder_id": host.send_after(delay_ms, "reminder", {"kind": "reminder"})}

    @handler("tick")
    def tick(self, kind: str = "timer") -> dict:
        if kind == "timer":
            self.timer_ticks += 1
        return {}

    @handler("reminder")
    def reminder(self, kind: str = "reminder") -> dict:
        if kind == "reminder":
            self.reminder_ticks += 1
        return {}

    @handler("kv_put")
    def kv_put(self, key: str, value: str) -> dict:
        result = host.kv_put(key, value)
        if result and result.startswith("ERROR"):
            return {"error": f"kv_put: {result}"}
        return {"ok": True, "key": key, "value": value}

    @handler("kv_get")
    def kv_get(self, key: str) -> dict:
        return {"key": key, "value": host.kv_get(key)}

    @handler("ts_write")
    def ts_write(self, tuple: list) -> dict:  # noqa: A002
        result = host.ts.write(tuple)
        if result:
            return {"error": f"ts_write: {result}"}
        return {"ok": True, "tuple": tuple}

    @handler("ts_read")
    def ts_read(self, pattern: list) -> dict:
        return {"tuple": host.ts.read(pattern)}

    @handler("blob_upload")
    def blob_upload(self, blob_id: str, data: str, content_type: str = "text/plain") -> dict:
        result = host.blob_upload(blob_id, data, content_type)
        if result and result.startswith("ERROR"):
            return {"error": f"blob_upload: {result}"}
        return {"ok": True, "blob_id": blob_id}

    @handler("blob_download")
    def blob_download(self, blob_id: str) -> dict:
        return {"blob_id": blob_id, "data": host.blob_download(blob_id)}

    @handler("group_members")
    def group_members(self, group: str) -> dict:
        try:
            return {"members": host.process_groups.members(group)}
        except Exception as exc:
            return {"error": f"pg_members: ERROR: {exc}"}

    @handler("send_event")
    def send_event(self, target: str, channel: str, body: str) -> dict:
        result = host.send(canonical_actor_target(target), "publish", {"channel": channel, "body": body})
        if result and result.startswith("ERROR"):
            return {"error": f"send: {result}"}
        return {"ok": True}

    @handler("broadcast_event")
    def broadcast_event(self, group: str, channel: str, body: str) -> dict:
        try:
            host.process_groups.broadcast(group, "publish", {"channel": channel, "body": body})
        except Exception as exc:
            return {"error": f"broadcast: ERROR: {exc}"}
        return {"ok": True}

    @handler("publish", "cast")
    def publish(self, channel: str, body: str) -> dict:
        self.received.append(f"{channel}:{body}")
        return {}

    @handler("stop_actor")
    def stop_actor(self, actor_id: str) -> dict:
        try:
            host.stop(actor_id)
        except Exception as exc:
            return {"error": f"stop: ERROR: {exc}"}
        return {"ok": True, "actor_id": actor_id}


@workflow_actor(facets=["virtual_actor", "durability"])
class AbstractionsWorkflow:
    status: str = state(default="pending")
    signals: list[str] = state(default_factory=list)

    @run_handler
    def start(self, order_id: str) -> dict:
        self.status = f"running:{order_id}"
        return {"status": self.status}

    @signal_handler("cancel")
    def cancel(self, reason: str = "unknown") -> None:
        self.signals.append(f"cancel:{reason}")
        self.status = "cancelled"

    @query_handler("status")
    def current_status(self) -> dict:
        return {"status": self.status, "signals": list(self.signals)}


EphemeralActor = AbstractionsActor
AbstractionsChannel = AbstractionsActor
ControllerActor = AbstractionsActor


ACTOR_ROLES = {
    "abstractions": AbstractionsActor,
    "ephemeral": EphemeralActor,
    "workflow": AbstractionsWorkflow,
    "channel": AbstractionsChannel,
    "controller": ControllerActor,
}
