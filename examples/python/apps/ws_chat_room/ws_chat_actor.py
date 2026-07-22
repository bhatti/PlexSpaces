# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# WebSocket Chat Room — Python WASM actors.
#
# Python port of examples/typescript/apps/ws_chat_room/chat_server_actor.ts.
#
# Deployed to a PlexSpaces node and driven by browser thin-node clients via
# the WsFrame binary WebSocket protocol.
#
# Actors:
#   ChatRoomActor  — per-room member registry; fans out chat_message to all
#                    member actor_ids via host.send(), routing each tell through
#                    WsActorTransportClient -> WsRegistry -> thin-node WS session.
#   PresenceActor  — per-user online/offline tracking with reminder timeout.
#
# Note on routing: `host.send(actorId, ...)` is the correct fan-out primitive
# for thin-node clients. Their actor_ids (e.g. alice//ChatClient::ns@<thin-node>)
# are stored in ChatRoomActor state; the ActorRegistry routes each send to the
# appropriate WS session via WsActorTransportClient and WsRegistry.

from __future__ import annotations

from plexspaces import actor, handler, host, state

_MAX_HISTORY = 50


# ─── ChatRoomActor ─────────────────────────────────────────────────────────────


@actor(facets=["virtual_actor", "process_group"])
class ChatRoomActor:
    """Per-room member registry with fan-out message delivery.

    State:
        members:  actorId -> username mapping for all connected members.
        history:  bounded ring buffer of the last 50 messages.
        msg_seq:  monotonically increasing message sequence counter.
    """

    members: dict = state(default_factory=dict)   # actorId -> username
    history: list = state(default_factory=list)
    msg_seq: int = state(default=0)

    def _room_id(self) -> str:
        self_id = host.self_id()
        return self_id.split("//")[0] if "//" in self_id else self_id

    @handler("join")
    def join(self, actor_id: str = "", username: str = "") -> dict:
        """Join the room.

        Removes any stale entry for the same username (reconnect with a new
        actor_id), then fans out a member_joined event to already-present
        members and returns the current member list and history to the joiner.
        """
        if not actor_id:
            return {"error": "actor_id required"}

        username = username or actor_id

        # Remove stale entries for same username (reconnect scenario)
        stale = [
            aid for aid, uname in self.members.items()
            if uname == username and aid != actor_id
        ]
        for aid in stale:
            del self.members[aid]

        existing_ids = list(self.members.keys())
        self.members[actor_id] = username
        all_ids = list(self.members.keys())
        member_info = dict(self.members)

        member_joined_event = {
            "room_id": self._room_id(),
            "members": all_ids,
            "member_info": member_info,
            "joined_actor_id": actor_id,
            "joined_username": username,
        }
        for mid in existing_ids:
            if mid != actor_id:
                host.send(mid, "member_joined", member_joined_event)

        return {
            "success": True,
            "members": all_ids,
            "member_info": member_info,
            "room_id": self._room_id(),
            "history": self.history,
        }

    @handler("leave")
    def leave(self, actor_id: str = "") -> dict:
        """Leave the room and notify remaining members."""
        if not actor_id:
            return {"success": True}

        self.members.pop(actor_id, None)
        all_ids = list(self.members.keys())
        member_info = dict(self.members)
        member_left_event = {
            "room_id": self._room_id(),
            "members": all_ids,
            "member_info": member_info,
        }
        for mid in all_ids:
            host.send(mid, "member_left", member_left_event)

        return {"success": True}

    @handler("send")
    def send(self, sender_actor_id: str = "", text: str = "") -> dict:
        """Broadcast a chat message to all room members including the sender."""
        if not sender_actor_id or not text:
            return {"error": "sender_actor_id and text required"}

        sender_username = self.members.get(sender_actor_id, sender_actor_id)
        ts = host.now_ms()
        self.msg_seq += 1

        entry = {
            "seq": self.msg_seq,
            "senderActorId": sender_actor_id,
            "sender": sender_username,
            "text": text,
            "ts": ts,
        }
        self.history = (self.history + [entry])[-_MAX_HISTORY:]

        event = {
            "sender": sender_actor_id,
            "sender_username": sender_username,
            "text": text,
            "room_id": self._room_id(),
            "ts": ts,
        }
        member_ids = list(self.members.keys())
        for mid in member_ids:
            host.send(mid, "chat_message", event)

        return {"success": True, "members_notified": len(member_ids)}

    @handler("members")
    def get_members(self) -> dict:
        """Return current member list and username map."""
        return {
            "members": list(self.members.keys()),
            "usernames": dict(self.members),
            "room_id": self._room_id(),
        }

    @handler("status")
    def status(self) -> dict:
        """Return lightweight room status."""
        return {
            "room_id": self._room_id(),
            "member_count": len(self.members),
            "messages": self.msg_seq,
        }

    @handler("__alarm__")
    def on_alarm(self) -> dict:
        """Periodic flush hook — logs room stats, useful for monitoring."""
        host.info(
            f"ChatRoom periodic flush: {self.msg_seq} messages, "
            f"{len(self.members)} members in {self._room_id()}"
        )
        return {"flushed": self.msg_seq}


# ─── PresenceActor ─────────────────────────────────────────────────────────────


@actor(facets=["virtual_actor", "reminder"])
class PresenceActor:
    """Per-user online/offline tracking with idle-timeout via reminder facet.

    State:
        username:   display name set when going online.
        online:     current online flag.
        last_seen:  epoch-ms timestamp of the last activity.
    """

    username: str = state(default="")
    online: bool = state(default=False)
    last_seen: int = state(default=0)

    def _user_id(self) -> str:
        self_id = host.self_id()
        return self_id.split("//")[0] if "//" in self_id else self_id

    @handler("online")
    def go_online(self, actor_id: str = "", username: str = "") -> dict:
        """Mark the user online and schedule a 60s idle-timeout reminder."""
        self.username = username or actor_id or self._user_id()
        self.online = True
        self.last_seen = host.now_ms()

        host.kv.put_json(
            f"presence:{self._user_id()}",
            {"online": True, "last_seen": self.last_seen},
        )
        # Schedule idle timeout check via reminder facet
        host.send_after(60_000, "timeout_check", {})

        return {"success": True, "online": True}

    @handler("offline")
    def go_offline(self, actor_id: str = "") -> dict:
        """Mark the user offline and persist the updated presence record."""
        self.online = False
        self.last_seen = host.now_ms()

        host.kv.put_json(
            f"presence:{self._user_id()}",
            {"online": False, "last_seen": self.last_seen},
        )
        return {"success": True, "online": False}

    @handler("timeout_check")
    def timeout_check(self) -> dict:
        """Invoked by host.send_after — marks user offline if idle for >55s."""
        idle_ms = host.now_ms() - self.last_seen
        if idle_ms > 55_000:
            self.online = False
            host.kv.put_json(
                f"presence:{self._user_id()}",
                {"online": False, "last_seen": self.last_seen},
            )
        return {"checked": True, "idle_ms": idle_ms, "online": self.online}

    @handler("status")
    def status(self) -> dict:
        """Return current presence status."""
        return {
            "user_id": self._user_id(),
            "username": self.username,
            "online": self.online,
            "last_seen": self.last_seen,
        }


__all__ = ["ChatRoomActor", "PresenceActor"]
