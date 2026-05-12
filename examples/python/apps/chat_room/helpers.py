# SPDX-License-Identifier: AGPL-3.0-or-later

"""Shared helpers for the large-scale chat example."""

from __future__ import annotations

from plexspaces import ActorID, host


def _peer(actor_type: str, name: str) -> str:
    """Build a canonical peer address, inheriting namespace/node from self when available."""
    try:
        self = ActorID.parse(host.self_id())
        return self.with_type_and_name(actor_type, name).to_str()
    except ValueError:
        # Not running inside WASM (e.g. unit tests) — use short-form address.
        return f"{actor_type}:{name}"


def guild_actor_id(guild_id: str) -> str:
    return _peer("GuildActor", guild_id)


def channel_actor_id(guild_id: str, channel_id: str) -> str:
    return _peer("ChannelActor", f"{guild_id}__{channel_id}")


def message_store_actor_id(guild_id: str, channel_id: str) -> str:
    return _peer("MessageStoreActor", f"{guild_id}__{channel_id}")


def session_actor_id(session_id: str) -> str:
    return _peer("SessionActor", session_id)


def presence_actor_id(user_id: str) -> str:
    return _peer("PresenceActor", user_id)


def connection_fsm_actor_id(session_id: str) -> str:
    return _peer("ConnectionFSM", session_id)


def moderation_workflow_actor_id(report_id: str) -> str:
    return _peer("ModerationWorkflow", report_id)


def fanout_actor_id() -> str:
    return _peer("FanoutActor", "singleton")


def audit_event_actor_id() -> str:
    return _peer("AuditEventActor", "singleton")


def actor_application_id(actor_id: str) -> str:
    """Extract the application namespace from a canonical runtime actor ID."""
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def actor_instance_name(actor_id: str) -> str:
    """Return the instance name from a canonical actor ID."""
    try:
        return ActorID.parse(actor_id).name
    except ValueError:
        return actor_id


def decode_guild_id(actor_id: str) -> str:
    """Extract guild_id (the instance name) from a GuildActor canonical ID."""
    return actor_instance_name(actor_id)


def decode_channel_parts(actor_id: str) -> tuple[str, str]:
    """Extract (guild_id, channel_id) from a ChannelActor canonical ID (name = guild_id__channel_id)."""
    name = actor_instance_name(actor_id)
    if "__" in name:
        parts = name.split("__", 1)
        return parts[0], parts[1]
    return name, ""


def channel_group(guild_id: str, channel_id: str) -> str:
    return f"channel:{guild_id}__{channel_id}"


def user_session_group(user_id: str) -> str:
    return f"user-session:{user_id}"


def safe_metrics_add(application_id: str, counters: dict[str, int], message_count: int = 1) -> None:
    """Best-effort metrics emission for the example."""
    if not application_id:
        return
    try:
        host.application_metrics_add(
            application_id,
            {
                "message_count": message_count,
                "counter_metrics": counters,
            },
        )
    except Exception:
        pass
