#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Deterministic local verification for the large-scale chat example."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "sdks" / "python"))
sys.path.insert(0, str(Path(__file__).resolve().parent))

from plexspaces import host  # noqa: E402
from plexspaces.decorators import dispatch_message, get_state_dict, set_state_dict  # noqa: E402
from plexspaces.host import _get_host, _get_registry  # noqa: E402

from chat_room_actor import (  # noqa: E402
    AuditEventActor,
    ChannelActor,
    ConnectionFSM,
    FanoutActor,
    GuildActor,
    MessageStoreActor,
    ModerationWorkflow,
    PresenceActor,
    SessionActor,
)


def main() -> None:
    guild = GuildActor()
    guild.guild_id = "guild-acme"
    dispatch_message(
        guild,
        "caller",
        "call",
        {"op": "register_session", "user_id": "alice", "session_id": "alice-mobile", "channels": ["general"]},
    )
    topology = dispatch_message(guild, "caller", "call", {"op": "topology"})
    assert topology["members"] == ["alice"]
    assert topology["channels"] == ["general"]

    channel = ChannelActor()
    channel.guild_id = "guild-acme"
    channel.channel_id = "general"
    assert dispatch_message(
        channel,
        "caller",
        "call",
        {"op": "join_member", "user_id": "alice", "session_id": "alice-mobile"},
    )["member_count"] == 1
    typing = dispatch_message(
        channel,
        "caller",
        "call",
        {"op": "mark_typing", "user_id": "alice", "ttl_ms": 500},
    )
    assert typing["typing_users"] == ["alice"]
    clear = dispatch_message(
        channel,
        "caller",
        "cast",
        {"op": "clear_typing", "user_id": "alice", "deadline_ms": channel.typing_deadlines["alice"]},
    )
    assert clear["status"] == "cleared"

    store = MessageStoreActor()
    store.guild_id = "guild-acme"
    store.channel_id = "general"
    stored = dispatch_message(
        store,
        "caller",
        "call",
        {"op": "append_message", "guild_id": "guild-acme", "channel_id": "general", "user_id": "alice", "text": "hello"},
    )
    assert stored["message_id"] == "general-1"
    history = dispatch_message(store, "caller", "call", {"op": "history", "limit": 10})
    assert history["count"] == 1

    session = SessionActor()
    session.session_id = "alice-mobile"
    session.user_id = "alice"
    session.guild_id = "guild-acme"
    dispatch_message(
        session,
        "fanout",
        "deliver_channel_event",
        {
            "guild_id": "guild-acme",
            "channel_id": "general",
            "message_id": "general-1",
            "from_user": "bob",
            "text": "hi",
            "delivered_at_ms": 1,
        },
    )
    inbox = dispatch_message(session, "caller", "call", {"op": "inbox"})
    assert inbox["unread_by_channel"]["general"] == 1
    dispatch_message(session, "caller", "call", {"op": "read_channel", "channel_id": "general"})
    assert dispatch_message(session, "caller", "call", {"op": "inbox"})["unread_by_channel"]["general"] == 0

    presence = PresenceActor()
    dispatch_message(
        presence,
        "caller",
        "call",
        {"op": "set_presence", "user_id": "alice", "guild_id": "guild-acme", "status": "online", "ttl_ms": 100},
    )
    assert presence.status == "online"
    dispatch_message(
        presence,
        "caller",
        "cast",
        {"op": "expire_presence", "deadline_ms": presence.expiry_deadline_ms},
    )
    assert presence.status == "offline"

    fsm = ConnectionFSM()
    assert dispatch_message(fsm, "caller", "call", {"op": "transition", "to": "connected"})["to"] == "connected"
    assert dispatch_message(fsm, "caller", "call", {"op": "transition", "to": "joined"})["to"] == "joined"

    workflow = ModerationWorkflow()
    assert dispatch_message(
        workflow,
        "caller",
        "workflow_run",
        {"report_id": "report-1", "message_id": "general-1", "reporter_id": "alice", "reason": "spam"},
    )["status"] == "under_review"
    dispatch_message(
        workflow,
        "caller",
        "workflow_signal:review",
        {"moderator_id": "mod-1", "resolution": "warn"},
    )
    assert dispatch_message(workflow, "caller", "workflow_query:status", {})["status"] == "reviewed"

    audit = AuditEventActor()
    dispatch_message(
        audit,
        "caller",
        "record_event",
        {"event_type": "channel_message", "guild_id": "guild-acme", "channel_id": "general", "message_id": "general-1", "user_id": "alice"},
    )
    assert dispatch_message(audit, "caller", "call", {"op": "stats"})["event_count"] == 1

    fanout = FanoutActor()
    fanout.on_init({"actor_id": "singleton//FanoutActor::chat-room-large-scale@test-node"})
    # Verify that FanoutActor registered itself in the object registry under "fanout" category
    mock_registry = _get_registry()
    registered = mock_registry.registry_discover({}, None, "fanout", [], [], None, 0, 10)
    import json as _json
    fanout_regs = _json.loads(registered) if isinstance(registered, str) else registered
    assert len(fanout_regs) >= 1, "FanoutActor must register in object registry with category=fanout"
    fanout_stats = dispatch_message(
        fanout,
        "caller",
        "call",
        {
            "op": "deliver_channel_event",
            "guild_id": "guild-acme",
            "channel_id": "general",
            "message_id": "general-1",
            "from_user": "alice",
            "text": "hello",
            "delivered_at_ms": 1,
        },
    )
    assert fanout_stats["recipient_count"] >= 0

    state_snapshot = get_state_dict(store)
    restored = MessageStoreActor()
    set_state_dict(restored, state_snapshot)
    assert get_state_dict(restored) == state_snapshot

    mock_host = _get_host()
    host.kv.put("chat:test", "ok")
    host.ts.write(["chat", "guild-acme", "general"])
    host.process_groups.join("channel:guild-acme__general")
    host.process_groups.broadcast("channel:guild-acme__general", "deliver_channel_event", {"ok": True})
    host.send("AuditEventActor:singleton", "record_event", {"event_type": "synthetic"})
    assert mock_host._kv["chat:test"] == "ok"
    assert mock_host._tuples[-1] == ["chat", "guild-acme", "general"]
    assert mock_host._group_messages[-1]["group"] == "channel:guild-acme__general"
    assert mock_host._sent_messages[-1]["to"] == "AuditEventActor:singleton"
    # Verify registry discover works for registered singleton services
    fanout_found = host.registry.discover({}, object_category="fanout", limit=1)
    assert len(fanout_found) >= 1, "registry.discover must find fanout actor by category"

    print("OK Python chat room example verified.")


if __name__ == "__main__":
    main()
