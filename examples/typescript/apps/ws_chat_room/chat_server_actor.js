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
class ChatRoomActor extends PlexSpacesActor {
    getDefaultState() {
        const selfId = host.selfId();
        // Actor canonical ID: "{name}//{type}::{ns}@{nodeId}" — name is the room ID
        const roomId = selfId.includes("//") ? selfId.split("//")[0] : selfId;
        return { roomId, members: {}, history: [] };
    }
    onJoin(payload) {
        if (!payload?.actor_id)
            return { error: "actor_id required" };
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
        const member_info = { ...this.state.members };
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
    onLeave(payload) {
        if (!payload?.actor_id)
            return { success: true };
        delete this.state.members[payload.actor_id];
        const allActorIds = Object.keys(this.state.members);
        const member_info = { ...this.state.members };
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
    onSend(payload) {
        if (!payload?.sender_actor_id || !payload?.text) {
            return { error: "sender_actor_id and text required" };
        }
        const senderUsername = this.state.members[payload.sender_actor_id] ?? payload.sender_actor_id;
        const ts = host.nowMs();
        const entry = {
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
    onMembers() {
        return {
            members: Object.keys(this.state.members),
            usernames: this.state.members,
            room_id: this.state.roomId,
        };
    }
    onStatus() {
        return {
            room_id: this.state.roomId,
            member_count: Object.keys(this.state.members).length,
        };
    }
}
class PresenceActor extends PlexSpacesActor {
    getDefaultState() {
        const selfId = host.selfId();
        const userId = selfId.includes("//") ? selfId.split("//")[0] : selfId;
        return { userId, online: false, last_seen: 0 };
    }
    onOnline(_payload) {
        this.state.online = true;
        this.state.last_seen = host.nowMs();
        host.kv.putJson(`presence:${this.state.userId}`, {
            online: true,
            last_seen: this.state.last_seen,
        });
        // Schedule an idle timeout check in 60s via the reminder facet
        host.sendAfter(60000, "timeout_check", {});
        return { success: true, online: true };
    }
    onOffline(_payload) {
        this.state.online = false;
        this.state.last_seen = host.nowMs();
        host.kv.putJson(`presence:${this.state.userId}`, {
            online: false,
            last_seen: this.state.last_seen,
        });
        return { success: true, online: false };
    }
    // Invoked by host.sendAfter — marks user offline if idle for >55s
    onTimeout_check() {
        const idleSince = host.nowMs() - this.state.last_seen;
        if (idleSince > 55000) {
            this.state.online = false;
            host.kv.putJson(`presence:${this.state.userId}`, {
                online: false,
                last_seen: this.state.last_seen,
            });
        }
        return { checked: true, idle_ms: idleSince };
    }
    onStatus() {
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
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
