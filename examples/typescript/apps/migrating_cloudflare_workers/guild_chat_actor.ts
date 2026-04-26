// Guild Chat Server - Discord-style Real-Time Chat with Durable Objects (TypeScript WASM)
//
// Demonstrates Cloudflare Workers Durable Objects pattern for real-time chat:
// - ChatRoom actor: per-room state, member tracking, message fan-out, history
// - RateLimiter actor: per-user token bucket rate limiting (spam prevention)
//
// Inspired by:
// - Discord's guild process architecture (one Elixir GenServer per guild)
// - Cloudflare's workers-chat-demo (Durable Object per room + rate limiter)
//
// Real-world use case: Chat platforms (Discord, Slack), collaborative apps,
// multiplayer game lobbies — anywhere you need per-entity stateful coordination.
//
// ## SDK Features Used
//
// - ActorRouter: Multi-actor routing (ChatRoom + RateLimiter)
// - PlexSpacesActor<T>: Actor base with typed state + JSON serialization
// - Host: Host function wrappers (ask, send, nowMs, kvPut, kvGet, etc.)
// - onInit(): Actor initialization from framework config
// - on<Op>(): Message handlers dispatched by payload.op
// - getState()/setState(): Checkpoint-based state persistence
// - host.kvPut()/kvGet(): Durable storage (message history persistence)
//
// ## Comparison to Cloudflare Workers / Durable Objects
//
// | Cloudflare Workers/DO             | PlexSpaces TypeScript             |
// |-----------------------------------|-----------------------------------|
// | export class ChatRoom extends DO  | ChatRoomActor extends PlexSpacesActor |
// | env.CHAT_ROOM.get(id)             | host.ask(actorID, ...)            |
// | this.state.storage.put/get        | host.kvPut/kvGet + getState       |
// | fetch(request) handler            | on<Op>(payload) handlers          |
// | blockConcurrencyWhile()           | onInit() (runs before any handle) |
// | WebSocket accept/send             | host.send() fan-out to members    |
// | alarm() scheduled callback        | host.sendAfter() timer            |
// | wrangler.toml [[bindings]]        | app-config.toml [[children]]      |
// | Worker script routing             | ActorRouter prefix matching       |

import { PlexSpacesActor, ActorRouter, host } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

interface MemberInfo {
  user_id: string;
  joined_at: number;
  msg_count: number;
}

interface ChatMessage {
  seq: number;
  user_id: string;
  content: string;
  timestamp: number;
}

interface ChatRoomState {
  [key: string]: unknown;
  room_id: string;
  members: Record<string, MemberInfo>;
  messages: ChatMessage[];
  max_history: number;
  message_seq: number;
  total_messages: number;
  total_joins: number;
  total_leaves: number;
  total_broadcasts: number;
  total_compute_ms: number;
}

interface TokenBucket {
  tokens: number;
  last_refill: number;
  allowed: number;
  denied: number;
}

interface RateLimiterState {
  [key: string]: unknown;
  actor_id: string;
  max_tokens: number;
  refill_rate_ms: number;
  buckets: Record<string, TokenBucket>;
  total_checks: number;
  total_allowed: number;
  total_denied: number;
}

// ========================================================================
// ChatRoom Actor (Durable Object equivalent)
// ========================================================================

class ChatRoomActor extends PlexSpacesActor<ChatRoomState> {
  getDefaultState(): ChatRoomState {
    return {
      room_id: "",
      members: {},
      messages: [],
      max_history: 100,
      message_seq: 0,
      total_messages: 0,
      total_joins: 0,
      total_leaves: 0,
      total_broadcasts: 0,
      total_compute_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.room_id = String(config.actor_id ?? "");

    const args = config.args as Record<string, unknown> | undefined;
    if (args) {
      if (args.max_history) {
        this.state.max_history = Number(args.max_history);
      }
    }
    if (this.state.max_history <= 0) {
      this.state.max_history = 100;
    }

    // Like Durable Object blockConcurrencyWhile() — restore persisted state
    try {
      const stored = host.kvGet("room:" + this.state.room_id + ":history");
      if (stored) {
        const msgs: ChatMessage[] = JSON.parse(stored);
        if (Array.isArray(msgs)) {
          this.state.messages = msgs;
          if (msgs.length > 0) {
            this.state.message_seq = msgs[msgs.length - 1].seq;
          }
        }
      }
    } catch {
      // No persisted state yet — start fresh
    }

    host.info(
      `ChatRoom ${this.state.room_id}: max_history=${this.state.max_history}, restored=${this.state.messages.length} messages`
    );
  }

  // --- Handlers ---

  onJoin(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    if (!userId) return { error: "user_id required" };

    const now = host.nowMs();

    if (this.state.members[userId]) {
      return {
        status: "ok",
        action: "already_joined",
        user_id: userId,
        room_id: this.state.room_id,
        members: Object.keys(this.state.members).length,
      };
    }

    this.state.members[userId] = {
      user_id: userId,
      joined_at: now,
      msg_count: 0,
    };
    this.state.total_joins++;

    this.addMessage("system", `${userId} joined the room`, now);

    return {
      status: "ok",
      action: "joined",
      user_id: userId,
      room_id: this.state.room_id,
      members: Object.keys(this.state.members).length,
    };
  }

  onLeave(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    if (!this.state.members[userId]) {
      return { status: "ok", action: "not_member", user_id: userId };
    }

    delete this.state.members[userId];
    this.state.total_leaves++;

    const now = host.nowMs();
    this.addMessage("system", `${userId} left the room`, now);

    return {
      status: "ok",
      action: "left",
      user_id: userId,
      room_id: this.state.room_id,
      members: Object.keys(this.state.members).length,
    };
  }

  onSend_message(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    const content = String(payload.content ?? "");
    if (!userId || !content) return { error: "user_id and content required" };

    const computeStart = host.nowMs();

    const member = this.state.members[userId];
    if (!member) return { error: "not a member of this room" };
    member.msg_count++;

    const now = host.nowMs();
    const msg = this.addMessage(userId, content, now);

    // Fan-out: count members who would receive the broadcast
    let fanOutCount = 0;
    for (const memberId of Object.keys(this.state.members)) {
      if (memberId !== userId) {
        fanOutCount++;
        this.state.total_broadcasts++;
      }
    }

    // Persist to durable storage (like DO transactional storage)
    this.persistHistory();

    const computeEnd = host.nowMs();
    this.state.total_compute_ms += computeEnd - computeStart;
    this.state.total_messages++;

    return {
      status: "ok",
      seq: msg.seq,
      room_id: this.state.room_id,
      user_id: userId,
      fan_out: fanOutCount,
      members: Object.keys(this.state.members).length,
      history_size: this.state.messages.length,
    };
  }

  onSend_message_batch(
    payload: Record<string, unknown>
  ): Record<string, unknown> {
    let count = Number(payload.count ?? 1000);
    if (count <= 0) count = 1000;
    const userId = String(payload.user_id ?? "bench-user");
    const content = String(
      payload.content ??
        "Benchmark message payload with enough data for realistic sizing"
    );

    // Ensure bench user is a member
    if (!this.state.members[userId]) {
      this.state.members[userId] = {
        user_id: userId,
        joined_at: host.nowMs(),
        msg_count: 0,
      };
      this.state.total_joins++;
    }
    // Add extra members for fan-out
    for (let i = 0; i < 10; i++) {
      const uid = `fan-out-member-${i}`;
      if (!this.state.members[uid]) {
        this.state.members[uid] = {
          user_id: uid,
          joined_at: host.nowMs(),
          msg_count: 0,
        };
      }
    }

    const computeStart = host.nowMs();

    let sent = 0;
    let totalFanOut = 0;
    for (let i = 0; i < count; i++) {
      this.state.members[userId].msg_count++;
      const now = host.nowMs();
      this.addMessage(userId, `${content} #${i}`, now);

      for (const memberId of Object.keys(this.state.members)) {
        if (memberId !== userId) {
          totalFanOut++;
          this.state.total_broadcasts++;
        }
      }
      this.state.total_messages++;
      sent++;
    }

    // Single persist at end (batched, like DO transaction)
    this.persistHistory();

    const computeEnd = host.nowMs();
    const computeMs = computeEnd - computeStart;
    this.state.total_compute_ms += computeMs;

    const opsPerSec = computeMs > 0 ? sent / (computeMs / 1000.0) : 0;

    return {
      status: "ok",
      total_sent: sent,
      total_fan_out: totalFanOut,
      compute_ms: computeMs,
      ops_per_sec: opsPerSec,
      history_size: this.state.messages.length,
      active_members: Object.keys(this.state.members).length,
    };
  }

  onGet_history(payload: Record<string, unknown>): Record<string, unknown> {
    let limit = Number(payload.limit ?? 50);
    const afterSeq = Number(payload.after_seq ?? 0);

    if (limit <= 0 || limit > this.state.max_history) limit = 50;

    let filtered = this.state.messages.filter((m) => m.seq > afterSeq);
    if (filtered.length > limit) {
      filtered = filtered.slice(filtered.length - limit);
    }

    return {
      status: "ok",
      room_id: this.state.room_id,
      messages: filtered,
      count: filtered.length,
      total: this.state.messages.length,
    };
  }

  onGet_members(): Record<string, unknown> {
    const members = Object.values(this.state.members).map((m) => ({
      user_id: m.user_id,
      joined_at: m.joined_at,
      msg_count: m.msg_count,
    }));
    return {
      status: "ok",
      room_id: this.state.room_id,
      members,
      count: members.length,
    };
  }

  onStats(): Record<string, unknown> {
    const totalTime = this.state.total_compute_ms;
    const opsPerSec =
      totalTime > 0
        ? this.state.total_messages / (totalTime / 1000.0)
        : 0;

    const memoryKB =
      (this.state.messages.length * 128 +
        Object.keys(this.state.members).length * 64) /
      1024.0;

    return {
      status: "ok",
      room_id: this.state.room_id,
      config: { max_history: this.state.max_history },
      counters: {
        total_messages: this.state.total_messages,
        total_joins: this.state.total_joins,
        total_leaves: this.state.total_leaves,
        total_broadcasts: this.state.total_broadcasts,
        active_members: Object.keys(this.state.members).length,
        history_size: this.state.messages.length,
        message_seq: this.state.message_seq,
      },
      benchmarks: {
        total_compute_ms: this.state.total_compute_ms,
        msgs_per_sec: opsPerSec,
        memory_kb: memoryKB,
      },
    };
  }

  // --- Internal helpers ---

  private addMessage(
    userId: string,
    content: string,
    timestamp: number
  ): ChatMessage {
    this.state.message_seq++;
    const msg: ChatMessage = {
      seq: this.state.message_seq,
      user_id: userId,
      content,
      timestamp,
    };

    this.state.messages.push(msg);

    // Trim to max history (ring buffer behavior)
    if (this.state.messages.length > this.state.max_history) {
      this.state.messages = this.state.messages.slice(
        this.state.messages.length - this.state.max_history
      );
    }

    return msg;
  }

  private persistHistory(): void {
    try {
      host.kvPut(
        "room:" + this.state.room_id + ":history",
        JSON.stringify(this.state.messages)
      );
    } catch {
      // KV not available — state will still be preserved via getState/setState
    }
  }
}

// ========================================================================
// RateLimiter Actor (Durable Object for per-user rate limiting)
// ========================================================================

class RateLimiterActor extends PlexSpacesActor<RateLimiterState> {
  getDefaultState(): RateLimiterState {
    return {
      actor_id: "",
      max_tokens: 5,
      refill_rate_ms: 1000,
      buckets: {},
      total_checks: 0,
      total_allowed: 0,
      total_denied: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actor_id = String(config.actor_id ?? "");

    const args = config.args as Record<string, unknown> | undefined;
    if (args) {
      if (args.max_tokens) this.state.max_tokens = Number(args.max_tokens);
      if (args.refill_rate_ms)
        this.state.refill_rate_ms = Number(args.refill_rate_ms);
    }
    if (this.state.max_tokens <= 0) this.state.max_tokens = 5;
    if (this.state.refill_rate_ms <= 0) this.state.refill_rate_ms = 1000;

    host.info(
      `RateLimiter ${this.state.actor_id}: max_tokens=${this.state.max_tokens}, refill_rate=${this.state.refill_rate_ms}ms`
    );
  }

  onCheck_rate(payload: Record<string, unknown>): Record<string, unknown> {
    const userId = String(payload.user_id ?? "");
    if (!userId) return { error: "user_id required" };

    const now = host.nowMs();

    let bucket = this.state.buckets[userId];
    if (!bucket) {
      bucket = {
        tokens: this.state.max_tokens,
        last_refill: now,
        allowed: 0,
        denied: 0,
      };
      this.state.buckets[userId] = bucket;
    }

    // Refill tokens based on elapsed time
    const elapsed = now - bucket.last_refill;
    if (this.state.refill_rate_ms > 0) {
      const newTokens = Math.floor(elapsed / this.state.refill_rate_ms);
      if (newTokens > 0) {
        bucket.tokens = Math.min(
          this.state.max_tokens,
          bucket.tokens + newTokens
        );
        bucket.last_refill = now;
      }
    }

    // Check if tokens available
    const allowed = bucket.tokens > 0;
    if (allowed) {
      bucket.tokens--;
      bucket.allowed++;
      this.state.total_allowed++;
    } else {
      bucket.denied++;
      this.state.total_denied++;
    }
    this.state.total_checks++;

    // Calculate retry-after
    let retryAfterMs = 0;
    if (!allowed && this.state.refill_rate_ms > 0) {
      retryAfterMs =
        this.state.refill_rate_ms - (elapsed % this.state.refill_rate_ms);
    }

    return {
      status: "ok",
      allowed,
      user_id: userId,
      remaining: bucket.tokens,
      limit: this.state.max_tokens,
      retry_after_ms: retryAfterMs,
    };
  }

  onCheck_rate_batch(payload: Record<string, unknown>): Record<string, unknown> {
    let userId = String(payload.user_id ?? "batch-user");
    let count = Number(payload.count ?? 1000);
    if (count <= 0) count = 1000;

    const batchStart = host.nowMs();
    let allowed = 0;
    let denied = 0;

    const numUsers = 50;
    const requestsPerUser = Math.max(1, Math.floor(count / numUsers));

    for (let u = 0; u < numUsers; u++) {
      const uid = `${userId}-${u}`;
      for (let i = 0; i < requestsPerUser; i++) {
        const now = host.nowMs();

        let bucket = this.state.buckets[uid];
        if (!bucket) {
          bucket = {
            tokens: this.state.max_tokens,
            last_refill: now,
            allowed: 0,
            denied: 0,
          };
          this.state.buckets[uid] = bucket;
        }

        // Refill
        const elapsed = now - bucket.last_refill;
        if (this.state.refill_rate_ms > 0) {
          const newTokens = Math.floor(elapsed / this.state.refill_rate_ms);
          if (newTokens > 0) {
            bucket.tokens = Math.min(
              this.state.max_tokens,
              bucket.tokens + newTokens
            );
            bucket.last_refill = now;
          }
        }

        const ok = bucket.tokens > 0;
        if (ok) {
          bucket.tokens--;
          bucket.allowed++;
          this.state.total_allowed++;
          allowed++;
        } else {
          bucket.denied++;
          this.state.total_denied++;
          denied++;
        }
        this.state.total_checks++;
      }
    }

    const batchEnd = host.nowMs();
    const durationMs = batchEnd - batchStart;

    const total = allowed + denied;
    const opsPerSec = durationMs > 0 ? total / (durationMs / 1000.0) : 0;

    return {
      status: "ok",
      total_requests: total,
      allowed,
      denied,
      duration_ms: durationMs,
      ops_per_sec: opsPerSec,
      unique_users: numUsers,
      reqs_per_user: requestsPerUser,
    };
  }

  onStats(): Record<string, unknown> {
    const denyRate =
      this.state.total_checks > 0
        ? (this.state.total_denied / this.state.total_checks) * 100
        : 0;
    return {
      status: "ok",
      config: {
        max_tokens: this.state.max_tokens,
        refill_rate_ms: this.state.refill_rate_ms,
      },
      counters: {
        total_checks: this.state.total_checks,
        total_allowed: this.state.total_allowed,
        total_denied: this.state.total_denied,
        deny_rate_pct: denyRate,
        active_users: Object.keys(this.state.buckets).length,
      },
    };
  }
}

// ========================================================================
// Main - Register multi-actor router for WASM export
// ========================================================================

const router = new ActorRouter({
  "chat-room": () => new ChatRoomActor(),
  "rate-limiter": () => new RateLimiterActor(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.init(configJson),
  handle: (
    from: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView
  ) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.setState(stateJson),
};
