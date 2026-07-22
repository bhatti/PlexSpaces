# SPDX-License-Identifier: AGPL-3.0-or-later

"""Routing, fan-out, and storage actors for the chat example."""

from __future__ import annotations

import json

from plexspaces import actor, event_actor, handler, host, init_handler, state

from helpers import actor_application_id, actor_instance_name, audit_event_actor_id, channel_group, decode_channel_parts, decode_guild_id, fanout_actor_id, message_store_actor_id, safe_metrics_add


@actor(facets=["virtual_actor", "durability"])
class GuildActor:
    """Guild/server router that tracks members, sessions, and channels."""

    application_id: str = state(default="")
    guild_id: str = state(default="")
    members: list[str] = state(default_factory=list)
    channels: list[str] = state(default_factory=list)
    session_index: dict = state(default_factory=dict)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.guild_id = decode_guild_id(actor_id)

    @handler("register_session")
    def register_session(self, user_id: str, session_id: str, channels: list[str] | None = None) -> dict:
        channels = list(channels or [])
        members = sorted({*self.members, user_id})
        session_index = dict(self.session_index)
        session_index[session_id] = {
            "user_id": user_id,
            "channels": channels,
        }
        self.members = members
        self.channels = sorted({*self.channels, *channels})
        self.session_index = session_index
        safe_metrics_add(self.application_id, {"chat_guild_registrations": 1})
        return {
            "guild_id": self.guild_id,
            "member_count": len(self.members),
            "session_count": len(self.session_index),
            "channels": list(self.channels),
        }

    @handler("create_channel")
    def create_channel(self, channel_id: str) -> dict:
        self.channels = sorted({*self.channels, channel_id})
        host.kv.put(f"guild:{self.guild_id}:channels", json.dumps(self.channels))
        return {
            "guild_id": self.guild_id,
            "channel_id": channel_id,
            "channels": list(self.channels),
        }

    @handler("topology")
    def topology(self) -> dict:
        return {
            "guild_id": self.guild_id,
            "members": list(self.members),
            "channels": list(self.channels),
            "session_index": dict(self.session_index),
        }


@actor(facets=["virtual_actor", "durability", "timer"])
class ChannelActor:
    """Text channel router that delegates storage and fan-out."""

    application_id: str = state(default="")
    guild_id: str = state(default="")
    channel_id: str = state(default="")
    member_index: dict = state(default_factory=dict)
    typing_deadlines: dict = state(default_factory=dict)
    messages: list[dict] = state(default_factory=list)
    last_message_id: str = state(default="")
    total_messages: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.guild_id, self.channel_id = decode_channel_parts(actor_id)

    @handler("join_member")
    def join_member(self, user_id: str, session_id: str = "") -> dict:
        member_index = dict(self.member_index)
        member_index[user_id] = {"session_id": session_id}
        self.member_index = member_index
        host.kv.put(
            f"channel:{self.guild_id}:{self.channel_id}:members",
            json.dumps(sorted(self.member_index.keys())),
        )
        return {
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "member_count": len(self.member_index),
        }

    @handler("mark_typing")
    def mark_typing(self, user_id: str, ttl_ms: int = 2000) -> dict:
        deadline_ms = host.now_ms() + int(ttl_ms)
        typing = dict(self.typing_deadlines)
        typing[user_id] = deadline_ms
        self.typing_deadlines = typing
        host.send_after(int(ttl_ms), "clear_typing", {"user_id": user_id, "deadline_ms": deadline_ms})
        return {
            "status": "typing",
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "user_id": user_id,
            "typing_users": sorted(self.typing_deadlines.keys()),
        }

    @handler("clear_typing")
    def clear_typing(self, user_id: str, deadline_ms: int) -> dict:
        current = int(self.typing_deadlines.get(user_id, 0))
        if current != int(deadline_ms):
            return {"status": "ignored", "reason": "stale_deadline"}
        typing = dict(self.typing_deadlines)
        typing.pop(user_id, None)
        self.typing_deadlines = typing
        return {"status": "cleared", "user_id": user_id}

    @handler("post_message")
    def post_message(self, user_id: str, text: str, session_id: str = "") -> dict:
        if user_id not in self.member_index:
            return {"error": "user_not_in_channel", "user_id": user_id}

        next_seq = self.total_messages + 1
        message_id = f"{self.channel_id}-{next_seq}"
        stored_at_ms = host.now_ms()
        host.send(
            message_store_actor_id(self.guild_id, self.channel_id),
            "append_message",
            {
                "guild_id": self.guild_id,
                "channel_id": self.channel_id,
                "user_id": user_id,
                "text": text,
                "session_id": session_id,
                "message_id": message_id,
                "stored_at_ms": stored_at_ms,
            },
        )
        event = {
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "message_id": message_id,
            "from_user": user_id,
            "text": text,
            "delivered_at_ms": stored_at_ms,
            "event_type": "message",
        }
        self.messages = [*self.messages, event][-200:]

        host.send(fanout_actor_id(), "deliver_channel_event", event)
        host.send(audit_event_actor_id(), "record_event", {
            "event_type": "channel_message",
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "message_id": message_id,
            "user_id": user_id,
        })

        self.last_message_id = message_id
        self.total_messages += 1
        safe_metrics_add(self.application_id, {"chat_channel_messages": 1})
        return {
            "status": "ok",
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "message_id": message_id,
            "recipient_count": len(self.member_index),
        }

    @handler("history")
    def history(self, limit: int = 50) -> dict:
        count = int(limit) if limit is not None else 50
        recent = self.messages[-count:] if count > 0 else list(self.messages)
        return {
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "messages": list(recent),
            "count": len(recent),
            "message_count": len(self.messages),
        }

    @handler("status")
    def status(self) -> dict:
        return {
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "members": sorted(self.member_index.keys()),
            "typing_users": sorted(self.typing_deadlines.keys()),
            "last_message_id": self.last_message_id,
            "total_messages": self.total_messages,
            "channel_group": channel_group(self.guild_id, self.channel_id),
        }


@actor(facets=["virtual_actor", "durability"])
class MessageStoreActor:
    """Durable per-channel message storage."""

    application_id: str = state(default="")
    guild_id: str = state(default="")
    channel_id: str = state(default="")
    messages: list[dict] = state(default_factory=list)
    next_message_seq: int = state(default=1)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.guild_id, self.channel_id = decode_channel_parts(actor_id)

    @handler("append_message")
    def append_message(
        self,
        guild_id: str,
        channel_id: str,
        user_id: str,
        text: str,
        session_id: str = "",
        message_id: str = "",
        stored_at_ms: int = 0,
    ) -> dict:
        if guild_id:
            self.guild_id = guild_id
        if channel_id:
            self.channel_id = channel_id
        resolved_message_id = message_id or f"{self.channel_id}-{self.next_message_seq}"
        resolved_stored_at_ms = int(stored_at_ms or host.now_ms())
        message = {
            "message_id": resolved_message_id,
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "user_id": user_id,
            "text": text,
            "session_id": session_id,
            "stored_at_ms": resolved_stored_at_ms,
        }
        self.messages = [*self.messages, message]
        self.next_message_seq = max(self.next_message_seq + 1, len(self.messages) + 1)
        safe_metrics_add(self.application_id, {"chat_messages_stored": 1})
        return {
            "status": "stored",
            "message_id": resolved_message_id,
            "stored_at_ms": resolved_stored_at_ms,
            "message_count": len(self.messages),
        }

    @handler("history")
    def history(self, limit: int = 50) -> dict:
        count = int(limit) if limit is not None else 50
        recent = self.messages[-count:] if count > 0 else list(self.messages)
        return {
            "guild_id": self.guild_id,
            "channel_id": self.channel_id,
            "messages": list(recent),
            "count": len(recent),
            "message_count": len(self.messages),
        }


@actor(facets=["virtual_actor"])
class FanoutActor:
    """Offloads broadcast from the channel actor."""

    application_id: str = state(default="")
    actor_name: str = state(default="")
    deliveries: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.actor_name = actor_instance_name(actor_id)
        try:
            host.registry.register({}, actor_id, "actor", "", object_category="fanout")
        except Exception as e:
            host.log("warn", f"FanoutActor failed to register in object registry: {e}")

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
        group = channel_group(guild_id, channel_id)
        recipients = host.process_groups.members(group)
        host.process_groups.broadcast(
            group,
            "deliver_channel_event",
            {
                "guild_id": guild_id,
                "channel_id": channel_id,
                "message_id": message_id,
                "from_user": from_user,
                "text": text,
                "delivered_at_ms": delivered_at_ms,
                "event_type": event_type,
            },
        )
        self.deliveries += 1
        safe_metrics_add(self.application_id, {"chat_fanout_events": 1})
        return {
            "status": "broadcast",
            "group": group,
            "recipient_count": len(recipients),
            "recipients": recipients,
            "deliveries": self.deliveries,
        }

    @handler("stats")
    def stats(self) -> dict:
        return {"deliveries": self.deliveries, "actor_name": self.actor_name}


@event_actor(facets=["virtual_actor", "durability"])
class AuditEventActor:
    """Captures append-only audit events for observability."""

    application_id: str = state(default="")
    actor_name: str = state(default="")
    recent_events: list[dict] = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(actor_id)
        self.actor_name = actor_instance_name(actor_id)
        try:
            host.registry.register({}, actor_id, "actor", "", object_category="audit_event")
        except Exception as e:
            host.log("warn", f"AuditEventActor failed to register in object registry: {e}")

    @handler("record_event")
    def record_event(
        self,
        event_type: str,
        guild_id: str = "",
        channel_id: str = "",
        message_id: str = "",
        user_id: str = "",
    ) -> dict:
        event = {
            "event_type": event_type,
            "guild_id": guild_id,
            "channel_id": channel_id,
            "message_id": message_id,
            "user_id": user_id,
            "recorded_at_ms": host.now_ms(),
        }
        self.recent_events = [*self.recent_events, event][-100:]
        host.ts.write(["audit", event_type, guild_id, channel_id, message_id, user_id])
        safe_metrics_add(self.application_id, {"chat_audit_events": 1})
        return {}

    @handler("stats")
    def stats(self) -> dict:
        return {
            "actor_name": self.actor_name,
            "event_count": len(self.recent_events),
            "recent_events": list(self.recent_events),
        }
