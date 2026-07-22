// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WebSocket Chat Room — WASM actors.
//
// Deployed to a PlexSpaces node and driven by browser thin-node clients via
// the WsFrame binary WebSocket protocol.
//
// Actors:
//   ChatRoomActor  — per-room member registry; fans out chat_message to all
//                    member actor_ids via host.send(), routing each tell through
//                    WsActorTransportClient → WsRegistry → thin-node WS session.
//   PresenceActor  — per-user online/offline tracking with reminder timeout.
//
// Note on routing: `host.send(actorId, ...)` is the correct fan-out primitive
// for thin-node clients. Their actor_ids (e.g. alice//ChatClient::ns@<thin-node>)
// are stored in ChatRoomActor state; the ActorRegistry routes each send to the
// appropriate WS session via WsActorTransportClient and WsRegistry.

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

// ─── ChatRoomActor ────────────────────────────────────────────────────────────

const MAX_HISTORY = 50;

type HistoryEntry = { senderActorId: string; sender: string; text: string; ts: number };

type ChatRoomState = {
  roomId: string;
  members: Record<string, string>;  // actorId -> username
  history: HistoryEntry[];
}

interface JoinPayload { actor_id: string; username: string }
interface LeavePayload { actor_id: string }
interface SendPayload { sender_actor_id: string; text: string }

class ChatRoomActor extends PlexSpacesActor<ChatRoomState> {
  getDefaultState(): ChatRoomState {
    const selfId = host.selfId();
    // Actor canonical ID: "{name}//{type}::{ns}@{nodeId}" — name is the room ID
    const roomId = selfId.includes("//") ? selfId.split("//")[0]! : selfId;
    return { roomId, members: {}, history: [] };
  }

  onJoin(payload: JoinPayload): unknown {
    if (!payload?.actor_id) return { error: "actor_id required" };
    const username = payload.username ?? payload.actor_id;

    // Remove stale entry for same username (reconnect with new actor_id)
    for (const [existingActorId, existingUsername] of Object.entries(this.state.members)) {
      if (existingUsername === username && existingActorId !== payload.actor_id) {
        delete this.state.members[existingActorId];
      }
    }

    const existingActorIds = Object.keys(this.state.members);
    this.state.members[payload.actor_id] = username;

    // Build member_info map for the joiner: actorId -> username
    const member_info: Record<string, string> = { ...this.state.members };

    // Fan-out member_joined to previously existing members
    const allActorIds = Object.keys(this.state.members);
    const memberJoinedEvent = {
      room_id: this.state.roomId,
      members: allActorIds,
      member_info,
      joined_actor_id: payload.actor_id,
      joined_username: username,
    };
    for (const actorId of existingActorIds) {
      if (actorId !== payload.actor_id) {
        host.send(actorId, "member_joined", memberJoinedEvent);
      }
    }

    return {
      success: true,
      members: allActorIds,
      member_info,
      room_id: this.state.roomId,
      history: this.state.history,
    };
  }

  onLeave(payload: LeavePayload): unknown {
    if (!payload?.actor_id) return { success: true };
    delete this.state.members[payload.actor_id];
    const allActorIds = Object.keys(this.state.members);
    const member_info: Record<string, string> = { ...this.state.members };
    const memberLeftEvent = {
      room_id: this.state.roomId,
      members: allActorIds,
      member_info,
    };
    for (const actorId of allActorIds) {
      host.send(actorId, "member_left", memberLeftEvent);
    }
    return { success: true };
  }

  onSend(payload: SendPayload): unknown {
    if (!payload?.sender_actor_id || !payload?.text) {
      return { error: "sender_actor_id and text required" };
    }
    const senderUsername = this.state.members[payload.sender_actor_id] ?? payload.sender_actor_id;
    const ts = host.nowMs();
    const entry: HistoryEntry = {
      senderActorId: payload.sender_actor_id,
      sender: senderUsername,
      text: payload.text,
      ts,
    };
    this.state.history.push(entry);
    if (this.state.history.length > MAX_HISTORY) {
      this.state.history = this.state.history.slice(-MAX_HISTORY);
    }

    const event = {
      sender: payload.sender_actor_id,
      sender_username: senderUsername,
      text: payload.text,
      room_id: this.state.roomId,
      ts,
    };
    // Fan out to all members INCLUDING sender so they see confirmation
    const memberIds = Object.keys(this.state.members);
    for (const actorId of memberIds) {
      host.send(actorId, "chat_message", event);
    }
    return { success: true, members_notified: memberIds.length };
  }

  onMembers(): unknown {
    return {
      members: Object.keys(this.state.members),
      usernames: this.state.members,
      room_id: this.state.roomId,
    };
  }

  onStatus(): unknown {
    return {
      room_id: this.state.roomId,
      member_count: Object.keys(this.state.members).length,
    };
  }
}

// ─── PresenceActor ────────────────────────────────────────────────────────────

type PresenceState = {
  userId: string;
  online: boolean;
  last_seen: number;
}

interface OnlinePayload { actor_id: string }
interface OfflinePayload { actor_id: string }

class PresenceActor extends PlexSpacesActor<PresenceState> {
  getDefaultState(): PresenceState {
    const selfId = host.selfId();
    const userId = selfId.includes("//") ? selfId.split("//")[0]! : selfId;
    return { userId, online: false, last_seen: 0 };
  }

  onOnline(_payload: OnlinePayload): unknown {
    this.state.online = true;
    this.state.last_seen = host.nowMs();
    host.kv.putJson(`presence:${this.state.userId}`, {
      online: true,
      last_seen: this.state.last_seen,
    });
    // Schedule an idle timeout check in 60s via the reminder facet
    host.sendAfter(60_000, "timeout_check", {});
    return { success: true, online: true };
  }

  onOffline(_payload: OfflinePayload): unknown {
    this.state.online = false;
    this.state.last_seen = host.nowMs();
    host.kv.putJson(`presence:${this.state.userId}`, {
      online: false,
      last_seen: this.state.last_seen,
    });
    return { success: true, online: false };
  }

  // Invoked by host.sendAfter — marks user offline if idle for >55s
  onTimeout_check(): unknown {
    const idleSince = host.nowMs() - this.state.last_seen;
    if (idleSince > 55_000) {
      this.state.online = false;
      host.kv.putJson(`presence:${this.state.userId}`, {
        online: false,
        last_seen: this.state.last_seen,
      });
    }
    return { checked: true, idle_ms: idleSince };
  }

  onStatus(): unknown {
    return {
      user_id: this.state.userId,
      online: this.state.online,
      last_seen: this.state.last_seen,
    };
  }
}

// ─── Router ───────────────────────────────────────────────────────────────────

const router = new ActorRouter({
  "ChatRoomActor": () => new ChatRoomActor(),
  "PresenceActor": () => new PresenceActor(),
});

export const actor = {
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
