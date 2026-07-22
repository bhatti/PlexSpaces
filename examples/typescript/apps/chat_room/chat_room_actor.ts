// SPDX-License-Identifier: AGPL-3.0-or-later

/**
 * Large-scale chat application example built from multiple PlexSpaces actors.
 *
 * TypeScript port of the Python chat_room example.
 * Includes all 9 actor classes in a single module:
 *   - SessionActor, PresenceActor, ConnectionFSM (sessions)
 *   - GuildActor, ChannelActor, MessageStoreActor, FanoutActor, AuditEventActor (routing)
 *   - ModerationWorkflow (workflows)
 */

import { ActorRouter, PlexSpacesActor, WorkflowActor, host } from "@plexspaces/sdk";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers (ported from helpers.py)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Extract the application namespace (e.g. "ts-chat-room-large-scale") from a
 * canonical actor ID like:  "name//ActorType::namespace@nodeId"
 */
function actorApplicationId(actorId: string): string {
  if (actorId.includes("//") && actorId.includes("::")) {
    const suffix = actorId.split("//", 2)[1];
    const qualified = suffix.split("@", 1)[0];
    const parts = qualified.split("::", 2);
    if (parts.length === 2) return parts[1];
  }
  if (actorId.includes(":") && actorId.includes("@")) {
    return actorId.split(":", 2)[1].split("@", 1)[0];
  }
  return "";
}

/**
 * Extract the instance name from a canonical actor ID.
 * "name//ActorType::namespace@nodeId" → "name"
 */
function actorInstanceName(actorId: string): string {
  const slashIdx = actorId.indexOf("//");
  if (slashIdx >= 0) return actorId.slice(0, slashIdx);
  return actorId;
}

/**
 * Build a peer canonical actor ID by inheriting namespace/node from self.
 * "actorType:name" fallback is used in non-WASM environments.
 */
function peer(actorType: string, name: string): string {
  const selfId = host.selfId();
  if (selfId && selfId.includes("//")) {
    const slashIdx = selfId.indexOf("//");
    const rest = selfId.slice(slashIdx + 2); // "ActorType::ns@node"
    const atIdx = rest.indexOf("@");
    const colonIdx = rest.indexOf("::");
    if (colonIdx >= 0 && atIdx > colonIdx) {
      const ns = rest.slice(colonIdx + 2, atIdx);
      const nodeId = rest.slice(atIdx + 1);
      return `${name}//${actorType}::${ns}@${nodeId}`;
    }
  }
  return `${actorType}:${name}`;
}

function guildActorId(guildId: string): string {
  return peer("GuildActor", guildId);
}

function channelActorId(guildId: string, channelId: string): string {
  return peer("ChannelActor", `${guildId}__${channelId}`);
}

function messageStoreActorId(guildId: string, channelId: string): string {
  return peer("MessageStoreActor", `${guildId}__${channelId}`);
}

function presenceActorId(userId: string): string {
  return peer("PresenceActor", userId);
}

function connectionFsmActorId(sessionId: string): string {
  return peer("ConnectionFSM", sessionId);
}

function fanoutActorId(): string {
  return peer("FanoutActor", "singleton");
}

function auditEventActorId(): string {
  return peer("AuditEventActor", "singleton");
}

function channelGroup(guildId: string, channelId: string): string {
  return `channel:${guildId}__${channelId}`;
}

function userSessionGroup(userId: string): string {
  return `user-session:${userId}`;
}

/**
 * Decode guild_id from a GuildActor canonical ID (instance name = guild_id).
 */
function decodeGuildId(actorId: string): string {
  return actorInstanceName(actorId);
}

/**
 * Decode (guild_id, channel_id) from a ChannelActor/MessageStoreActor canonical ID.
 * Instance name format: "guild_id__channel_id"
 */
function decodeChannelParts(actorId: string): [string, string] {
  const name = actorInstanceName(actorId);
  const sep = name.indexOf("__");
  if (sep >= 0) {
    return [name.slice(0, sep), name.slice(sep + 2)];
  }
  return [name, ""];
}

/**
 * Best-effort metrics emission — errors are swallowed.
 */
function safeMetricsAdd(applicationId: string, counters: Record<string, number>): void {
  if (!applicationId) return;
  try {
    host.incrCounters(applicationId, counters);
  } catch {
    // intentionally swallow
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// ConnectionFSM valid transitions
// ─────────────────────────────────────────────────────────────────────────────

const VALID_CONNECTION_TRANSITIONS: Record<string, Set<string>> = {
  offline: new Set(["connected"]),
  connected: new Set(["joined"]),
  joined: new Set(["idle", "disconnected"]),
  idle: new Set(["joined", "disconnected"]),
  disconnected: new Set(["connected"]),
};

// ─────────────────────────────────────────────────────────────────────────────
// State types
// ─────────────────────────────────────────────────────────────────────────────

type SessionState = {
  actor_id: string;
  application_id: string;
  session_id: string;
  user_id: string;
  guild_id: string;
  joined_channels: string[];
  delivered_events: Record<string, unknown>[];
  unread_by_channel: Record<string, number>;
  last_delivery_ms: number;
};

type PresenceState = {
  actor_id: string;
  application_id: string;
  user_id: string;
  guild_id: string;
  status: string;
  last_seen_ms: number;
  expiry_deadline_ms: number;
};

type ConnectionFSMState = {
  actor_id: string;
  application_id: string;
  session_id: string;
  fsm_state: string;
  transition_count: number;
};

type GuildState = {
  actor_id: string;
  application_id: string;
  guild_id: string;
  members: string[];
  channels: string[];
  session_index: Record<string, { user_id: string; channels: string[] }>;
};

type ChannelState = {
  actor_id: string;
  application_id: string;
  guild_id: string;
  channel_id: string;
  member_index: Record<string, { session_id: string }>;
  typing_deadlines: Record<string, number>;
  messages: Record<string, unknown>[];
  last_message_id: string;
  total_messages: number;
};

type MessageStoreState = {
  actor_id: string;
  application_id: string;
  guild_id: string;
  channel_id: string;
  messages: Record<string, unknown>[];
  next_message_seq: number;
};

type FanoutState = {
  actor_id: string;
  application_id: string;
  actor_name: string;
  deliveries: number;
};

type AuditEventState = {
  actor_id: string;
  application_id: string;
  actor_name: string;
  recent_events: Record<string, unknown>[];
};

type ModerationState = {
  actor_id: string;
  application_id: string;
  report_id: string;
  status: string;
  message_id: string;
  reason: string;
  reporter_id: string;
  resolution: string;
  signals: string[];
};

// ─────────────────────────────────────────────────────────────────────────────
// SessionActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Represents one connected client session.
 * Ported from sessions.py SessionActor.
 */
class SessionActor extends PlexSpacesActor<SessionState> {
  getDefaultState(): SessionState {
    return {
      actor_id: "",
      application_id: "",
      session_id: "",
      user_id: "",
      guild_id: "",
      joined_channels: [],
      delivered_events: [],
      unread_by_channel: {},
      last_delivery_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.session_id = actorInstanceName(actorId);
    this.state.user_id = String(args.user_id ?? "");
    this.state.guild_id = String(args.guild_id ?? "");
    const channels = args.channels;
    if (Array.isArray(channels)) {
      this.state.joined_channels = channels.map(String);
    }
  }

  onConnect(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const guildId = String(payload.guild_id ?? "");
    const channels = Array.isArray(payload.channels)
      ? (payload.channels as unknown[]).map(String)
      : [];
    const ttlMs = Number(payload.ttl_ms ?? 60000);

    this.state.user_id = userId;
    this.state.guild_id = guildId;
    this.state.joined_channels = channels;

    host.processGroups.join(userSessionGroup(userId));

    for (const channelId of channels) {
      host.processGroups.join(channelGroup(guildId, channelId));
      host.send(channelActorId(guildId, channelId), "join_member", {
        user_id: userId,
        session_id: this.state.session_id,
      });
    }

    host.send(guildActorId(guildId), "register_session", {
      user_id: userId,
      session_id: this.state.session_id,
      channels,
    });

    host.send(presenceActorId(userId), "set_presence", {
      user_id: userId,
      guild_id: guildId,
      status: "online",
      ttl_ms: ttlMs,
    });

    host.send(connectionFsmActorId(this.state.session_id), "transition", {
      to: "connected",
    });
    host.send(connectionFsmActorId(this.state.session_id), "transition", {
      to: "joined",
    });

    safeMetricsAdd(this.state.application_id, { chat_sessions_connected: 1 });

    return {
      status: "connected",
      session_id: this.state.session_id,
      user_id: this.state.user_id,
      guild_id: this.state.guild_id,
      channels: [...this.state.joined_channels],
    };
  }

  onSend_channel_message(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.user_id || !this.state.guild_id) {
      return { error: "session_not_connected" };
    }
    const channelId = String(payload.channel_id ?? "");
    const text = String(payload.text ?? "");
    return host.ask(
      channelActorId(this.state.guild_id, channelId),
      "post_message",
      {
        user_id: this.state.user_id,
        session_id: this.state.session_id,
        text,
      },
      5000,
    ) as Record<string, unknown>;
  }

  onSet_typing(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.user_id || !this.state.guild_id) {
      return { error: "session_not_connected" };
    }
    const channelId = String(payload.channel_id ?? "");
    const ttlMs = Number(payload.ttl_ms ?? 2000);
    return host.ask(
      channelActorId(this.state.guild_id, channelId),
      "mark_typing",
      { user_id: this.state.user_id, ttl_ms: ttlMs },
      5000,
    ) as Record<string, unknown>;
  }

  onDeliver_channel_event(payload: Record<string, unknown>): Record<string, unknown> {
    const guildId = String(payload.guild_id ?? "");
    const channelId = String(payload.channel_id ?? "");
    const messageId = String(payload.message_id ?? "");
    const fromUser = String(payload.from_user ?? "");
    const text = String(payload.text ?? "");
    const deliveredAtMs = Number(payload.delivered_at_ms ?? 0);
    const eventType = String(payload.event_type ?? "message");

    const event: Record<string, unknown> = {
      event_type: eventType,
      guild_id: guildId,
      channel_id: channelId,
      message_id: messageId,
      from_user: fromUser,
      text,
      delivered_at_ms: deliveredAtMs,
    };

    const recent = [...this.state.delivered_events, event];
    this.state.delivered_events = recent.slice(-50);
    this.state.last_delivery_ms = deliveredAtMs;

    if (fromUser !== this.state.user_id && eventType === "message") {
      const unread = { ...this.state.unread_by_channel };
      unread[channelId] = (unread[channelId] ?? 0) + 1;
      this.state.unread_by_channel = unread;
    }

    return { status: "delivered", session_id: this.state.session_id };
  }

  onRead_channel(payload: Record<string, unknown>): Record<string, unknown> {
    const channelId = String(payload.channel_id ?? "");
    const unread = { ...this.state.unread_by_channel };
    unread[channelId] = 0;
    this.state.unread_by_channel = unread;

    host.ask(
      connectionFsmActorId(this.state.session_id),
      "transition",
      { to: "idle" },
      5000,
    );

    return {
      status: "read",
      channel_id: channelId,
      session_id: this.state.session_id,
      remaining_unread: { ...this.state.unread_by_channel },
    };
  }

  onInbox(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      session_id: this.state.session_id,
      user_id: this.state.user_id,
      guild_id: this.state.guild_id,
      joined_channels: [...this.state.joined_channels],
      delivered_events: [...this.state.delivered_events],
      unread_by_channel: { ...this.state.unread_by_channel },
      last_delivery_ms: this.state.last_delivery_ms,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// PresenceActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Tracks user presence with reminder-style expiry.
 * Ported from sessions.py PresenceActor.
 */
class PresenceActor extends PlexSpacesActor<PresenceState> {
  getDefaultState(): PresenceState {
    return {
      actor_id: "",
      application_id: "",
      user_id: "",
      guild_id: "",
      status: "offline",
      last_seen_ms: 0,
      expiry_deadline_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    const args = (config.args as Record<string, unknown> | undefined) ?? {};
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.user_id = actorInstanceName(actorId);
    this.state.guild_id = String(args.guild_id ?? "");
  }

  onSet_presence(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const guildId = String(payload.guild_id ?? "");
    const status = String(payload.status ?? "online");
    const ttlMs = Number(payload.ttl_ms ?? 60000);

    if (userId) this.state.user_id = userId;
    if (guildId) this.state.guild_id = guildId;

    const nowMs = host.nowMs();
    this.state.status = status;
    this.state.last_seen_ms = nowMs;
    this.state.expiry_deadline_ms = nowMs + ttlMs;

    host.sendAfter(ttlMs, "expire_presence", {
      deadline_ms: this.state.expiry_deadline_ms,
    });

    safeMetricsAdd(this.state.application_id, { chat_presence_updates: 1 });

    return {
      user_id: this.state.user_id,
      guild_id: this.state.guild_id,
      status: this.state.status,
      expires_at_ms: this.state.expiry_deadline_ms,
    };
  }

  onExpire_presence(payload: Record<string, unknown>): Record<string, unknown> {
    const deadlineMs = Number(payload.deadline_ms ?? 0);
    if (deadlineMs !== this.state.expiry_deadline_ms) {
      return { status: "ignored", reason: "stale_deadline" };
    }
    this.state.status = "offline";
    safeMetricsAdd(this.state.application_id, { chat_presence_expirations: 1 });
    return { status: "expired", user_id: this.state.user_id };
  }

  onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      user_id: this.state.user_id,
      guild_id: this.state.guild_id,
      status: this.state.status,
      last_seen_ms: this.state.last_seen_ms,
      expires_at_ms: this.state.expiry_deadline_ms,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// ConnectionFSM
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Explicit session lifecycle state machine.
 * Valid transitions:
 *   offline → connected → joined → {idle, disconnected}
 *   idle → {joined, disconnected}
 *   disconnected → connected
 * Ported from sessions.py ConnectionFSM.
 */
class ConnectionFSM extends PlexSpacesActor<ConnectionFSMState> {
  getDefaultState(): ConnectionFSMState {
    return {
      actor_id: "",
      application_id: "",
      session_id: "",
      fsm_state: "offline",
      transition_count: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.session_id = actorInstanceName(actorId);
  }

  onTransition(payload: Record<string, unknown>): Record<string, unknown> {
    const to = String(payload.to ?? "");
    const allowed = VALID_CONNECTION_TRANSITIONS[this.state.fsm_state] ?? new Set<string>();

    if (!allowed.has(to)) {
      return {
        status: "ignored",
        from: this.state.fsm_state,
        to,
        allowed: [...allowed].sort(),
      };
    }

    const previous = this.state.fsm_state;
    this.state.fsm_state = to;
    this.state.transition_count += 1;

    safeMetricsAdd(this.state.application_id, { chat_connection_transitions: 1 });

    return {
      status: "ok",
      session_id: this.state.session_id,
      from: previous,
      to: this.state.fsm_state,
      transition_count: this.state.transition_count,
    };
  }

  onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      session_id: this.state.session_id,
      state: this.state.fsm_state,
      transition_count: this.state.transition_count,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// GuildActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Guild/server router that tracks members, sessions, and channels.
 * Ported from routing.py GuildActor.
 */
class GuildActor extends PlexSpacesActor<GuildState> {
  getDefaultState(): GuildState {
    return {
      actor_id: "",
      application_id: "",
      guild_id: "",
      members: [],
      channels: [],
      session_index: {},
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.guild_id = decodeGuildId(actorId);
  }

  onRegister_session(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const sessionId = String(payload.session_id ?? "");
    const channels = Array.isArray(payload.channels)
      ? (payload.channels as unknown[]).map(String)
      : [];

    const membersSet = new Set([...this.state.members, userId]);
    const sessionIndex = { ...this.state.session_index };
    sessionIndex[sessionId] = { user_id: userId, channels };

    const channelsSet = new Set([...this.state.channels, ...channels]);

    this.state.members = [...membersSet].sort();
    this.state.channels = [...channelsSet].sort();
    this.state.session_index = sessionIndex;

    safeMetricsAdd(this.state.application_id, { chat_guild_registrations: 1 });

    return {
      guild_id: this.state.guild_id,
      member_count: this.state.members.length,
      session_count: Object.keys(this.state.session_index).length,
      channels: [...this.state.channels],
    };
  }

  onCreate_channel(payload: Record<string, unknown>): Record<string, unknown> {
    const channelId = String(payload.channel_id ?? "");
    const channelsSet = new Set([...this.state.channels, channelId]);
    this.state.channels = [...channelsSet].sort();

    host.kv.put(
      `guild:${this.state.guild_id}:channels`,
      JSON.stringify(this.state.channels),
    );

    return {
      guild_id: this.state.guild_id,
      channel_id: channelId,
      channels: [...this.state.channels],
    };
  }

  onTopology(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      guild_id: this.state.guild_id,
      members: [...this.state.members],
      channels: [...this.state.channels],
      session_index: { ...this.state.session_index },
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// ChannelActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Text channel router that delegates storage and fan-out.
 * Keeps last 200 messages in memory; delegates persistence to MessageStoreActor.
 * Ported from routing.py ChannelActor.
 */
class ChannelActor extends PlexSpacesActor<ChannelState> {
  getDefaultState(): ChannelState {
    return {
      actor_id: "",
      application_id: "",
      guild_id: "",
      channel_id: "",
      member_index: {},
      typing_deadlines: {},
      messages: [],
      last_message_id: "",
      total_messages: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    [this.state.guild_id, this.state.channel_id] = decodeChannelParts(actorId);
  }

  onJoin_member(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const sessionId = String(payload.session_id ?? "");

    const memberIndex = { ...this.state.member_index };
    memberIndex[userId] = { session_id: sessionId };
    this.state.member_index = memberIndex;

    host.kv.put(
      `channel:${this.state.guild_id}:${this.state.channel_id}:members`,
      JSON.stringify(Object.keys(this.state.member_index).sort()),
    );

    return {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      member_count: Object.keys(this.state.member_index).length,
    };
  }

  onMark_typing(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const ttlMs = Number(payload.ttl_ms ?? 2000);
    const deadlineMs = host.nowMs() + ttlMs;

    const typing = { ...this.state.typing_deadlines };
    typing[userId] = deadlineMs;
    this.state.typing_deadlines = typing;

    host.sendAfter(ttlMs, "clear_typing", { user_id: userId, deadline_ms: deadlineMs });

    return {
      status: "typing",
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      user_id: userId,
      typing_users: Object.keys(this.state.typing_deadlines).sort(),
    };
  }

  onClear_typing(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const deadlineMs = Number(payload.deadline_ms ?? 0);
    const current = this.state.typing_deadlines[userId] ?? 0;

    if (current !== deadlineMs) {
      return { status: "ignored", reason: "stale_deadline" };
    }

    const typing = { ...this.state.typing_deadlines };
    delete typing[userId];
    this.state.typing_deadlines = typing;

    return { status: "cleared", user_id: userId };
  }

  onPost_message(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const text = String(payload.text ?? "");
    const sessionId = String(payload.session_id ?? "");

    if (!(userId in this.state.member_index)) {
      return { error: "user_not_in_channel", user_id: userId };
    }

    const nextSeq = this.state.total_messages + 1;
    const messageId = `${this.state.channel_id}-${nextSeq}`;
    const storedAtMs = host.nowMs();

    host.send(messageStoreActorId(this.state.guild_id, this.state.channel_id), "append_message", {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      user_id: userId,
      text,
      session_id: sessionId,
      message_id: messageId,
      stored_at_ms: storedAtMs,
    });

    const event: Record<string, unknown> = {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      message_id: messageId,
      from_user: userId,
      text,
      delivered_at_ms: storedAtMs,
      event_type: "message",
    };

    const msgs = [...this.state.messages, event];
    this.state.messages = msgs.slice(-200);

    host.send(fanoutActorId(), "deliver_channel_event", event);
    host.send(auditEventActorId(), "record_event", {
      event_type: "channel_message",
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      message_id: messageId,
      user_id: userId,
    });

    this.state.last_message_id = messageId;
    this.state.total_messages += 1;

    safeMetricsAdd(this.state.application_id, { chat_channel_messages: 1 });

    return {
      status: "ok",
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      message_id: messageId,
      recipient_count: Object.keys(this.state.member_index).length,
    };
  }

  onHistory(payload: Record<string, unknown>): Record<string, unknown> {
    const limit = Number(payload.limit ?? 50);
    const count = limit > 0 ? limit : this.state.messages.length;
    const recent = this.state.messages.slice(-count);
    return {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      messages: [...recent],
      count: recent.length,
      message_count: this.state.messages.length,
    };
  }

  onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      members: Object.keys(this.state.member_index).sort(),
      typing_users: Object.keys(this.state.typing_deadlines).sort(),
      last_message_id: this.state.last_message_id,
      total_messages: this.state.total_messages,
      channel_group: channelGroup(this.state.guild_id, this.state.channel_id),
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// MessageStoreActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Durable per-channel message storage.
 * Ported from routing.py MessageStoreActor.
 */
class MessageStoreActor extends PlexSpacesActor<MessageStoreState> {
  getDefaultState(): MessageStoreState {
    return {
      actor_id: "",
      application_id: "",
      guild_id: "",
      channel_id: "",
      messages: [],
      next_message_seq: 1,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    [this.state.guild_id, this.state.channel_id] = decodeChannelParts(actorId);
  }

  onAppend_message(payload: Record<string, unknown>): Record<string, unknown> {
    const guildId = String(payload.guild_id ?? "");
    const channelId = String(payload.channel_id ?? "");
    const userId = String(payload.user_id ?? "");
    const text = String(payload.text ?? "");
    const sessionId = String(payload.session_id ?? "");
    const messageIdArg = String(payload.message_id ?? "");
    const storedAtMsArg = Number(payload.stored_at_ms ?? 0);

    if (guildId) this.state.guild_id = guildId;
    if (channelId) this.state.channel_id = channelId;

    const resolvedMessageId =
      messageIdArg || `${this.state.channel_id}-${this.state.next_message_seq}`;
    const resolvedStoredAtMs = storedAtMsArg || host.nowMs();

    const message: Record<string, unknown> = {
      message_id: resolvedMessageId,
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      user_id: userId,
      text,
      session_id: sessionId,
      stored_at_ms: resolvedStoredAtMs,
    };

    this.state.messages = [...this.state.messages, message];
    this.state.next_message_seq = Math.max(
      this.state.next_message_seq + 1,
      this.state.messages.length + 1,
    );

    safeMetricsAdd(this.state.application_id, { chat_messages_stored: 1 });

    return {
      status: "stored",
      message_id: resolvedMessageId,
      stored_at_ms: resolvedStoredAtMs,
      message_count: this.state.messages.length,
    };
  }

  onHistory(payload: Record<string, unknown>): Record<string, unknown> {
    const limit = Number(payload.limit ?? 50);
    const count = limit > 0 ? limit : this.state.messages.length;
    const recent = this.state.messages.slice(-count);
    return {
      guild_id: this.state.guild_id,
      channel_id: this.state.channel_id,
      messages: [...recent],
      count: recent.length,
      message_count: this.state.messages.length,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// FanoutActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Offloads broadcast from the channel actor to all members in the channel group.
 * Ported from routing.py FanoutActor.
 */
class FanoutActor extends PlexSpacesActor<FanoutState> {
  getDefaultState(): FanoutState {
    return {
      actor_id: "",
      application_id: "",
      actor_name: "",
      deliveries: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.actor_name = actorInstanceName(actorId);

    // Best-effort registry registration
    try {
      host.registry.register({ objectId: actorId, objectType: "actor", objectCategory: "fanout" });
    } catch (e) {
      host.warn(`FanoutActor failed to register in object registry: ${e}`);
    }
  }

  onDeliver_channel_event(payload: Record<string, unknown>): Record<string, unknown> {
    const guildId = String(payload.guild_id ?? "");
    const channelId = String(payload.channel_id ?? "");
    const messageId = String(payload.message_id ?? "");
    const fromUser = String(payload.from_user ?? "");
    const text = String(payload.text ?? "");
    const deliveredAtMs = Number(payload.delivered_at_ms ?? 0);
    const eventType = String(payload.event_type ?? "message");

    const group = channelGroup(guildId, channelId);

    let recipients: string[] = [];
    try {
      recipients = host.processGroups.members(group);
    } catch {
      recipients = [];
    }

    try {
      host.processGroups.broadcast(group, "deliver_channel_event", {
        guild_id: guildId,
        channel_id: channelId,
        message_id: messageId,
        from_user: fromUser,
        text,
        delivered_at_ms: deliveredAtMs,
        event_type: eventType,
      });
    } catch {
      // best-effort broadcast
    }

    this.state.deliveries += 1;
    safeMetricsAdd(this.state.application_id, { chat_fanout_events: 1 });

    return {
      status: "broadcast",
      group,
      recipient_count: recipients.length,
      recipients,
      deliveries: this.state.deliveries,
    };
  }

  onStats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      deliveries: this.state.deliveries,
      actor_name: this.state.actor_name,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// AuditEventActor
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Captures append-only audit events for observability.
 * Ported from routing.py AuditEventActor.
 */
class AuditEventActor extends PlexSpacesActor<AuditEventState> {
  getDefaultState(): AuditEventState {
    return {
      actor_id: "",
      application_id: "",
      actor_name: "",
      recent_events: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.actor_name = actorInstanceName(actorId);

    // Best-effort registry registration
    try {
      host.registry.register({ objectId: actorId, objectType: "actor", objectCategory: "audit_event" });
    } catch (e) {
      host.warn(`AuditEventActor failed to register in object registry: ${e}`);
    }
  }

  onRecord_event(payload: Record<string, unknown>): Record<string, unknown> {
    const eventType = String(payload.event_type ?? "");
    const guildId = String(payload.guild_id ?? "");
    const channelId = String(payload.channel_id ?? "");
    const messageId = String(payload.message_id ?? "");
    const userId = String(payload.user_id ?? "");
    const recordedAtMs = host.nowMs();

    const event: Record<string, unknown> = {
      event_type: eventType,
      guild_id: guildId,
      channel_id: channelId,
      message_id: messageId,
      user_id: userId,
      recorded_at_ms: recordedAtMs,
    };

    const recent = [...this.state.recent_events, event];
    this.state.recent_events = recent.slice(-100);

    try {
      host.ts.write(["audit", eventType, guildId, channelId, messageId, userId]);
    } catch {
      // best-effort tuplespace write
    }

    safeMetricsAdd(this.state.application_id, { chat_audit_events: 1 });

    return {};
  }

  onStats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      actor_name: this.state.actor_name,
      event_count: this.state.recent_events.length,
      recent_events: [...this.state.recent_events],
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// ModerationWorkflow
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Durable moderation review flow for flagged messages.
 * Ported from workflows.py ModerationWorkflow.
 */
class ModerationWorkflow extends WorkflowActor<ModerationState> {
  getDefaultState(): ModerationState {
    return {
      actor_id: "",
      application_id: "",
      report_id: "",
      status: "pending",
      message_id: "",
      reason: "",
      reporter_id: "",
      resolution: "",
      signals: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const actorId = String(config.actor_id ?? "");
    this.state = this.getDefaultState();
    this.state.actor_id = actorId;
    this.state.application_id = actorApplicationId(actorId);
    this.state.report_id = actorInstanceName(actorId);
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const reportId = String(payload.report_id ?? "");
    const messageId = String(payload.message_id ?? "");
    const reporterId = String(payload.reporter_id ?? "");
    const reason = String(payload.reason ?? "");

    if (!this.state.application_id) {
      this.state.application_id = actorApplicationId(host.selfId());
    }
    if (!this.state.report_id) {
      this.state.report_id = reportId || actorInstanceName(host.selfId());
    }

    this.state.message_id = messageId;
    this.state.reporter_id = reporterId;
    this.state.reason = reason;
    this.state.status = "under_review";

    safeMetricsAdd(this.state.application_id, { chat_moderation_reports: 1 });

    return {
      report_id: this.state.report_id,
      status: this.state.status,
      message_id: this.state.message_id,
    };
  }

  signal(name: string, data: Record<string, unknown>): void {
    if (name === "review") {
      const moderatorId = String(data.moderator_id ?? "");
      const resolution = String(data.resolution ?? "");
      this.state.resolution = resolution;
      this.state.status = "reviewed";
      this.state.signals = [...this.state.signals, `review:${moderatorId}:${resolution}`];
    } else if (name === "close") {
      const resolution = String(data.resolution ?? "dismissed");
      this.state.resolution = resolution;
      this.state.status = "closed";
      this.state.signals = [...this.state.signals, `close::${resolution}`];
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "status") {
      return {
        report_id: this.state.report_id,
        status: this.state.status,
        message_id: this.state.message_id,
        reporter_id: this.state.reporter_id,
        reason: this.state.reason,
        resolution: this.state.resolution,
        signals: [...this.state.signals],
      };
    }
    return { error: `unknown query: ${name}` };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Router & WIT exports
// ─────────────────────────────────────────────────────────────────────────────

const router = new ActorRouter({
  SessionActor: () => new SessionActor(),
  PresenceActor: () => new PresenceActor(),
  ConnectionFSM: () => new ConnectionFSM(),
  GuildActor: () => new GuildActor(),
  ChannelActor: () => new ChannelActor(),
  MessageStoreActor: () => new MessageStoreActor(),
  FanoutActor: () => new FanoutActor(),
  AuditEventActor: () => new AuditEventActor(),
  ModerationWorkflow: () => new ModerationWorkflow(),
});

export const actor = {
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
