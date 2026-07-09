#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "sdks" / "python"))

from plexspaces.decorators import dispatch_message, get_state_dict, set_state_dict  # noqa: E402
from plexspaces import host  # noqa: E402
from plexspaces.host import _get_host  # noqa: E402
from abstractions_app import (  # noqa: E402
    AbstractionsActor,
    AbstractionsChannel,
    AbstractionsWorkflow,
    EphemeralActor,
)


def main() -> None:
    actor = AbstractionsActor()
    actor.actor_id = "cart-1//abstractions::abstractions-python@test-node"
    actor.application_id = "abstractions-python"
    assert dispatch_message(actor, "caller", "call", {"op": "increment", "amount": 3}) == {
        "actor_id": "cart-1//abstractions::abstractions-python@test-node",
        "count": 3,
    }
    status = dispatch_message(actor, "caller", "call", {"op": "status"})
    assert status["count"] == 3

    state_dict = get_state_dict(actor)
    restored = AbstractionsActor()
    set_state_dict(restored, state_dict)
    assert get_state_dict(restored) == state_dict
    assert restored.count == 3

    ephemeral = EphemeralActor()
    ephemeral.role = "ephemeral"
    ephemeral.count = 5
    assert dispatch_message(ephemeral, "caller", "call", {"op": "status"})["count"] == 5
    assert dispatch_message(ephemeral, "caller", "call", {"op": "increment", "amount": 2})["count"] == 7
    reactivated_ephemeral = EphemeralActor()
    reactivated_ephemeral.role = "ephemeral"
    reactivated_ephemeral.count = 5
    assert dispatch_message(reactivated_ephemeral, "caller", "call", {"op": "status"})["count"] == 5

    workflow = AbstractionsWorkflow()
    assert dispatch_message(workflow, "caller", "workflow_run", {"order_id": "o-1"}) == {"status": "running:o-1"}
    assert dispatch_message(workflow, "caller", "workflow_signal:cancel", {"reason": "user"}) == {}
    assert dispatch_message(workflow, "caller", "workflow_query:status", {}) == {
        "status": "cancelled",
        "signals": ["cancel:user"],
    }

    channel = AbstractionsChannel()
    assert dispatch_message(channel, "publisher", "cast", {"op": "publish", "channel": "alerts", "body": "hello"}) == {}
    assert channel.received == ["alerts:hello"]

    mock_host = _get_host()
    host.kv_put("abstractions/config", "ready")
    host.ts.write(["abstractions", "task", "t-1"])
    host.blob_upload("abstractions/blob-1", "aGVsbG8=", "text/plain")
    host.process_groups.join("abstractions-group")
    host.send("abstractions-channel", "publish", {"channel": "alerts", "body": "direct"})
    host.process_groups.broadcast("abstractions-group", "notify", {"ok": True})
    timer_id = host.send_after(250, "tick", {"kind": "timer"})
    spawned_id = host.spawn("abstractions", "abstractions-actor", "", {"count": "1"})
    assert json.loads(host.kv_list("abstractions/")) == ["abstractions/config"]
    assert host.ts.read(["abstractions", "task", None]) == ["abstractions", "task", "t-1"]
    assert json.loads(host.blob_list("abstractions/")) == ["abstractions/blob-1"]
    assert host.process_groups.members("abstractions-group") == ["mock-actor"]
    assert mock_host._sent_messages[-1]["to"] == "abstractions-channel"
    assert mock_host._sent_messages[-1]["msg_type"] == "publish"
    assert mock_host._group_messages[-1]["group"] == "abstractions-group"
    assert mock_host._group_messages[-1]["msg_type"] == "notify"
    assert timer_id == "mock-timer-1"
    assert spawned_id == "abstractions-actor"
    print("OK Python abstractions example verified.")


if __name__ == "__main__":
    main()
