# SPDX-License-Identifier: AGPL-3.0-or-later

"""Session, presence, and connection lifecycle actors for the chat example."""

from __future__ import annotations

from plexspaces import actor, fsm_actor, handler, host, init_handler, state

from helpers import (
    actor_application_id,
    actor_instance_name,
    channel_actor_id,
    channel_group,
    connection_fsm_actor_id,
    guild_actor_id,
    presence_actor_id,
    safe_metrics_add,
    user_session_group,
)


_VALID_CONNECTION_TRANSITIONS = {
    "offline": {"connected"},
    "connected": {"joined"},
    "joined": {"idle", "disconnected"},
    "idle": {"joined", "disconnected"},
    "disconnected": {"connected"},
}


@actor(facets=["virtual_actor", "durability"])
class SessionActor:
    """Represents one connected client session."""

    application_id: str = state(default="")
    session_id: str = state(default="")
    user_id: str = state(default="")
    guild_id: str = state(default="")
    joined_channels: list[str] = state(default_factory=list)
    delivered_events: list[dict] = state(default_factory=list)
    unread_by_channel: dict = state(default_factory=dict)
    last_delivery_ms: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.session_id = actor_instance_name(actor_id)
        args = config.get("args", {})
        self.user_id = args.get("user_id", self.user_id)
        self.guild_id = args.get("guild_id", self.guild_id)
        self.joined_channels = list(args.get("channels", self.joined_channels))

    @handler("connect")
    def connect(
        self,
        user_id: str,
        guild_id: str,
        channels: list[str] | None = None,
        ttl_ms: int = 60000,
    ) -> dict:
        self.user_id = user_id
        self.guild_id = guild_id
        channels = list(channels or [])
        self.joined_channels = channels
        host.process_groups.join(user_session_group(user_id))

        for channel_id in channels:
            host.process_groups.join(channel_group(guild_id, channel_id))
            host.send(
                channel_actor_id(guild_id, channel_id),
                "join_member",
                {"user_id": user_id, "session_id": self.session_id},
            )

        host.send(
            guild_actor_id(guild_id),
            "register_session",
            {
                "user_id": user_id,
                "session_id": self.session_id,
                "channels": channels,
            },
        )
        host.send(
            presence_actor_id(user_id),
            "set_presence",
            {
                "user_id": user_id,
                "guild_id": guild_id,
                "status": "online",
                "ttl_ms": ttl_ms,
            },
        )
        host.send(
            connection_fsm_actor_id(self.session_id),
            "transition",
            {"to": "connected"},
        )
        host.send(
            connection_fsm_actor_id(self.session_id),
            "transition",
            {"to": "joined"},
        )
        safe_metrics_add(self.application_id, {"chat_sessions_connected": 1})
        return {
            "status": "connected",
            "session_id": self.session_id,
            "user_id": self.user_id,
            "guild_id": self.guild_id,
            "channels": list(self.joined_channels),
        }

    @handler("send_channel_message")
    def send_channel_message(self, channel_id: str, text: str) -> dict:
        if not self.user_id or not self.guild_id:
            return {"error": "session_not_connected"}
        return host.ask(
            channel_actor_id(self.guild_id, channel_id),
            "post_message",
            {
                "user_id": self.user_id,
                "session_id": self.session_id,
                "text": text,
            },
            timeout_ms=5000,
        )

    @handler("set_typing")
    def set_typing(self, channel_id: str, ttl_ms: int = 2000) -> dict:
        if not self.user_id or not self.guild_id:
            return {"error": "session_not_connected"}
        return host.ask(
            channel_actor_id(self.guild_id, channel_id),
            "mark_typing",
            {
                "user_id": self.user_id,
                "ttl_ms": ttl_ms,
            },
            timeout_ms=5000,
        )

    @handler("deliver_channel_event")
    def deliver_channel_event(
        self,
        guild_id: str,
        channel_id: str,
        message_id: str,
        from_user: str,
        text: str,
        delivered_at_ms: int,
        event_type: str = "message",
    ) -> dict:
        event = {
            "event_type": event_type,
            "guild_id": guild_id,
            "channel_id": channel_id,
            "message_id": message_id,
            "from_user": from_user,
            "text": text,
            "delivered_at_ms": delivered_at_ms,
        }
        recent = [*self.delivered_events, event]
        self.delivered_events = recent[-50:]
        self.last_delivery_ms = delivered_at_ms
        if from_user != self.user_id and event_type == "message":
            unread = dict(self.unread_by_channel)
            unread[channel_id] = int(unread.get(channel_id, 0)) + 1
            self.unread_by_channel = unread
        return {"status": "delivered", "session_id": self.session_id}

    @handler("read_channel")
    def read_channel(self, channel_id: str) -> dict:
        unread = dict(self.unread_by_channel)
        unread[channel_id] = 0
        self.unread_by_channel = unread
        host.ask(
            connection_fsm_actor_id(self.session_id),
            "transition",
            {"to": "idle"},
            timeout_ms=5000,
        )
        return {
            "status": "read",
            "channel_id": channel_id,
            "session_id": self.session_id,
            "remaining_unread": dict(self.unread_by_channel),
        }

    @handler("inbox")
    def inbox(self) -> dict:
        return {
            "session_id": self.session_id,
            "user_id": self.user_id,
            "guild_id": self.guild_id,
            "joined_channels": list(self.joined_channels),
            "delivered_events": list(self.delivered_events),
            "unread_by_channel": dict(self.unread_by_channel),
            "last_delivery_ms": self.last_delivery_ms,
        }


@actor(facets=["virtual_actor", "durability", "reminder"])
class PresenceActor:
    """Tracks user presence with reminder-style expiry."""

    application_id: str = state(default="")
    user_id: str = state(default="")
    guild_id: str = state(default="")
    status: str = state(default="offline")
    last_seen_ms: int = state(default=0)
    expiry_deadline_ms: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.user_id = actor_instance_name(actor_id)
        args = config.get("args", {})
        self.guild_id = args.get("guild_id", self.guild_id)

    @handler("set_presence")
    def set_presence(
        self,
        user_id: str = "",
        guild_id: str = "",
        status: str = "online",
        ttl_ms: int = 60000,
    ) -> dict:
        if user_id:
            self.user_id = user_id
        if guild_id:
            self.guild_id = guild_id
        now_ms = host.now_ms()
        self.status = status
        self.last_seen_ms = now_ms
        self.expiry_deadline_ms = now_ms + int(ttl_ms)
        host.send_after(int(ttl_ms), "expire_presence", {"deadline_ms": self.expiry_deadline_ms})
        safe_metrics_add(self.application_id, {"chat_presence_updates": 1})
        return {
            "user_id": self.user_id,
            "guild_id": self.guild_id,
            "status": self.status,
            "expires_at_ms": self.expiry_deadline_ms,
        }

    @handler("expire_presence")
    def expire_presence(self, deadline_ms: int) -> dict:
        if int(deadline_ms) != int(self.expiry_deadline_ms):
            return {"status": "ignored", "reason": "stale_deadline"}
        self.status = "offline"
        safe_metrics_add(self.application_id, {"chat_presence_expirations": 1})
        return {"status": "expired", "user_id": self.user_id}

    @handler("status")
    def current_status(self) -> dict:
        return {
            "user_id": self.user_id,
            "guild_id": self.guild_id,
            "status": self.status,
            "last_seen_ms": self.last_seen_ms,
            "expires_at_ms": self.expiry_deadline_ms,
        }


@fsm_actor(states=["offline", "connected", "joined", "idle", "disconnected"], initial="offline")
class ConnectionFSM:
    """Explicit session lifecycle state machine."""

    session_id: str = state(default="")
    application_id: str = state(default="")
    fsm_state: str = state(default="offline")
    transition_count: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.session_id = actor_instance_name(actor_id)

    @handler("transition")
    def transition(self, to: str) -> dict:
        allowed = _VALID_CONNECTION_TRANSITIONS.get(self.fsm_state, set())
        if to not in allowed:
            return {
                "status": "ignored",
                "from": self.fsm_state,
                "to": to,
                "allowed": sorted(allowed),
            }
        previous = self.fsm_state
        self.fsm_state = to
        self.transition_count += 1
        safe_metrics_add(self.application_id, {"chat_connection_transitions": 1})
        return {
            "status": "ok",
            "session_id": self.session_id,
            "from": previous,
            "to": self.fsm_state,
            "transition_count": self.transition_count,
        }

    @handler("status")
    def status(self) -> dict:
        return {
            "session_id": self.session_id,
            "state": self.fsm_state,
            "transition_count": self.transition_count,
        }
