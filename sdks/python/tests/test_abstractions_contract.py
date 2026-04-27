# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors

"""Cross-SDK abstraction contract tests for Python authoring APIs."""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from plexspaces import (  # noqa: E402
    gen_server_actor,
    event_actor,
    workflow_actor,
    handler,
    state,
    host,
    run_handler,
    signal_handler,
    query_handler,
)
from plexspaces.decorators import dispatch_message, get_state_dict, set_state_dict  # noqa: E402
from plexspaces.host import _get_host  # noqa: E402


@gen_server_actor(facets=["virtual_actor", "durability", "timer", "reminder"])
class AbstractionsActor:
    actor_id: str = state(default="")
    count: int = state(default=0)

    @handler("increment")
    def increment(self, amount: int = 1) -> dict:
        self.count += amount
        return {"count": self.count}

    @handler("status")
    def status(self) -> dict:
        return {"actor_id": self.actor_id, "count": self.count}


@workflow_actor(facets=["virtual_actor", "durability"])
class AbstractionsWorkflow:
    status: str = state(default="pending")
    signals: list = state(default_factory=list)

    @run_handler
    def start(self, order_id: str) -> dict:
        self.status = f"running:{order_id}"
        return {"status": self.status}

    @signal_handler("cancel")
    def cancel(self, reason: str = "unknown") -> None:
        self.signals.append(reason)
        self.status = "cancelled"

    @query_handler("status")
    def current_status(self) -> dict:
        return {"status": self.status, "signals": list(self.signals)}


@event_actor(facets=["process_group"])
class AbstractionsChannel:
    received: list = state(default_factory=list)

    @handler("publish", "cast")
    def publish(self, channel: str, body: str) -> None:
        self.received.append({"channel": channel, "body": body})


def exercise_services() -> dict:
    mock_host = _get_host()
    host.kv_put("abstractions/config", "ready")
    host.ts.write(["abstractions", "task", "t-1"])
    tuple_value = host.ts.read(["abstractions", "task", None])
    taken_value = host.ts.take(["abstractions", "task", None])
    host.blob_upload("abstractions/blob-1", "aGVsbG8=", "text/plain")
    blob_value = host.blob_download("abstractions/blob-1")
    host.process_groups.join("abstractions-group")
    members = host.process_groups.members("abstractions-group")
    host.send("abstractions-channel", "publish", {"channel": "alerts", "body": "direct"})
    host.process_groups.broadcast("abstractions-group", "notify", {"ok": True})
    timer_id = host.send_after(250, "tick", {"kind": "timer"})
    spawned_id = host.spawn("abstractions", "abstractions-actor", {"count": 1})
    host.stop(spawned_id)
    return {
        "kv_keys": json.loads(host.kv_list("abstractions/")),
        "tuple_read": tuple_value,
        "tuple_take": taken_value,
        "blob_ids": json.loads(host.blob_list("abstractions/")),
        "blob_value": blob_value,
        "members": members,
        "last_send": mock_host._sent_messages[-1],
        "last_group_message": mock_host._group_messages[-1],
        "timer_id": timer_id,
        "spawned_id": spawned_id,
    }


def test_gen_server_contract_and_state_roundtrip():
    actor = AbstractionsActor()
    actor.actor_id = "cart-1"

    response = dispatch_message(actor, "caller", "call", {"op": "increment", "amount": 3})
    assert response == {"count": 3}
    assert dispatch_message(actor, "caller", "call", {"op": "status"}) == {
        "actor_id": "cart-1",
        "count": 3,
    }

    state_dict = get_state_dict(actor)
    restored = AbstractionsActor()
    set_state_dict(restored, state_dict)
    assert get_state_dict(restored) == {"actor_id": "cart-1", "count": 3}


def test_workflow_contract_uses_named_handlers():
    workflow = AbstractionsWorkflow()

    assert dispatch_message(
        workflow,
        "caller",
        "workflow_run",
        {"order_id": "o-1"},
    ) == {"status": "running:o-1"}
    assert dispatch_message(
        workflow,
        "caller",
        "workflow_signal:cancel",
        {"reason": "user"},
    ) == {}
    assert dispatch_message(
        workflow,
        "caller",
        "workflow_query:status",
        {},
    ) == {"status": "cancelled", "signals": ["user"]}


def test_event_actor_models_channel_style_event_delivery():
    channel = AbstractionsChannel()

    assert dispatch_message(
        channel,
        "publisher",
        "cast",
        {"op": "publish", "channel": "alerts", "body": "hello"},
    ) == {}
    assert channel.received == [{"channel": "alerts", "body": "hello"}]


def test_host_service_contract_uses_sdk_surface():
    result = exercise_services()

    assert result["kv_keys"] == ["abstractions/config"]
    assert result["tuple_read"] == ["abstractions", "task", "t-1"]
    assert result["tuple_take"] == ["abstractions", "task", "t-1"]
    assert result["blob_ids"] == ["abstractions/blob-1"]
    assert result["blob_value"] == "aGVsbG8="
    assert result["members"] == ["mock-actor"]
    assert result["last_send"]["to"] == "abstractions-channel"
    assert result["last_send"]["msg_type"] == "publish"
    assert result["last_group_message"]["group"] == "abstractions-group"
    assert result["last_group_message"]["msg_type"] == "notify"
    assert result["timer_id"] == "mock-timer-1"
    assert result["spawned_id"] == "abstractions-actor"
