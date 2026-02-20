// guild_chat.js
// Cloudflare Workers + Durable Objects - Discord-style Guild Chat
//
// Native Cloudflare implementation of the same chat server for comparison.
// Uses Durable Objects for per-room state and per-user rate limiting.
//
// Deploy with wrangler:
//   npx wrangler deploy
//
// wrangler.toml:
//   [[durable_objects.bindings]]
//   name = "CHAT_ROOM"
//   class_name = "ChatRoom"
//
//   [[durable_objects.bindings]]
//   name = "RATE_LIMITER"
//   class_name = "RateLimiter"
//
//   [[migrations]]
//   tag = "v1"
//   new_classes = ["ChatRoom", "RateLimiter"]

// ========================================================================
// Worker entry point (fetch handler / router)
// ========================================================================

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;

    // Route: POST /rooms/:roomId/:action
    const roomMatch = path.match(/^\/rooms\/([^/]+)\/(.+)$/);
    if (roomMatch && request.method === "POST") {
      const [, roomId, action] = roomMatch;

      // Rate limit check (get Durable Object by user IP)
      if (action === "send_message") {
        const ip = request.headers.get("CF-Connecting-IP") || "unknown";
        const rateLimiterId = env.RATE_LIMITER.idFromName(ip);
        const rateLimiter = env.RATE_LIMITER.get(rateLimiterId);
        const rlResponse = await rateLimiter.fetch(
          new Request("https://internal/check_rate", {
            method: "POST",
            body: JSON.stringify({ user_id: ip }),
          })
        );
        const rlResult = await rlResponse.json();
        if (!rlResult.allowed) {
          return new Response(JSON.stringify(rlResult), {
            status: 429,
            headers: {
              "Content-Type": "application/json",
              "Retry-After": String(Math.ceil(rlResult.retry_after_ms / 1000)),
            },
          });
        }
      }

      // Get ChatRoom Durable Object by room ID
      const chatRoomId = env.CHAT_ROOM.idFromName(roomId);
      const chatRoom = env.CHAT_ROOM.get(chatRoomId);

      // Forward request to the Durable Object
      const body = await request.json();
      const doResponse = await chatRoom.fetch(
        new Request(`https://internal/${action}`, {
          method: "POST",
          body: JSON.stringify(body),
        })
      );

      return new Response(await doResponse.text(), {
        headers: { "Content-Type": "application/json" },
      });
    }

    return new Response("Not found", { status: 404 });
  },
};

// ========================================================================
// ChatRoom Durable Object
// ========================================================================

export class ChatRoom {
  constructor(state, env) {
    this.state = state;
    this.env = env;
    this.members = new Map();  // user_id -> { joinedAt, msgCount }
    this.messages = [];
    this.maxHistory = 100;
    this.messageSeq = 0;
    this.totalMessages = 0;
    this.totalJoins = 0;
    this.totalLeaves = 0;

    // blockConcurrencyWhile: restore state before handling requests
    this.state.blockConcurrencyWhile(async () => {
      const stored = await this.state.storage.get("messages");
      if (stored) {
        this.messages = stored;
        if (this.messages.length > 0) {
          this.messageSeq = this.messages[this.messages.length - 1].seq;
        }
      }
    });
  }

  async fetch(request) {
    const url = new URL(request.url);
    const action = url.pathname.slice(1); // remove leading /
    const body = await request.json().catch(() => ({}));

    switch (action) {
      case "join":
        return this.handleJoin(body);
      case "leave":
        return this.handleLeave(body);
      case "send_message":
        return this.handleSendMessage(body);
      case "get_history":
        return this.handleGetHistory(body);
      case "get_members":
        return this.handleGetMembers();
      case "stats":
        return this.handleStats();
      default:
        return jsonResponse({ error: `unknown action: ${action}` }, 400);
    }
  }

  handleJoin({ user_id }) {
    if (!user_id) return jsonResponse({ error: "user_id required" }, 400);

    if (this.members.has(user_id)) {
      return jsonResponse({
        status: "ok",
        action: "already_joined",
        user_id,
        members: this.members.size,
      });
    }

    this.members.set(user_id, { joinedAt: Date.now(), msgCount: 0 });
    this.totalJoins++;
    this.addMessage("system", `${user_id} joined the room`);

    return jsonResponse({
      status: "ok",
      action: "joined",
      user_id,
      members: this.members.size,
    });
  }

  handleLeave({ user_id }) {
    if (!this.members.has(user_id)) {
      return jsonResponse({ status: "ok", action: "not_member", user_id });
    }

    this.members.delete(user_id);
    this.totalLeaves++;
    this.addMessage("system", `${user_id} left the room`);

    return jsonResponse({
      status: "ok",
      action: "left",
      user_id,
      members: this.members.size,
    });
  }

  async handleSendMessage({ user_id, content }) {
    if (!user_id || !content) {
      return jsonResponse({ error: "user_id and content required" }, 400);
    }

    const member = this.members.get(user_id);
    if (!member) {
      return jsonResponse({ error: "not a member of this room" }, 403);
    }
    member.msgCount++;

    const msg = this.addMessage(user_id, content);

    // Persist to durable storage (transactional)
    await this.state.storage.put("messages", this.messages);

    // Fan-out count (in production, you'd broadcast via WebSocket)
    const fanOut = this.members.size - 1;
    this.totalMessages++;

    return jsonResponse({
      status: "ok",
      seq: msg.seq,
      user_id,
      fan_out: fanOut,
      members: this.members.size,
      history_size: this.messages.length,
    });
  }

  handleGetHistory({ limit = 50, after_seq = 0 }) {
    let filtered = this.messages.filter((m) => m.seq > after_seq);
    if (filtered.length > limit) {
      filtered = filtered.slice(filtered.length - limit);
    }
    return jsonResponse({
      status: "ok",
      messages: filtered,
      count: filtered.length,
      total: this.messages.length,
    });
  }

  handleGetMembers() {
    const members = [];
    for (const [userId, info] of this.members) {
      members.push({
        user_id: userId,
        joined_at: info.joinedAt,
        msg_count: info.msgCount,
      });
    }
    return jsonResponse({ status: "ok", members, count: members.length });
  }

  handleStats() {
    return jsonResponse({
      status: "ok",
      counters: {
        total_messages: this.totalMessages,
        total_joins: this.totalJoins,
        total_leaves: this.totalLeaves,
        active_members: this.members.size,
        history_size: this.messages.length,
        message_seq: this.messageSeq,
      },
      config: { max_history: this.maxHistory },
    });
  }

  addMessage(userId, content) {
    this.messageSeq++;
    const msg = {
      seq: this.messageSeq,
      user_id: userId,
      content,
      timestamp: Date.now(),
    };
    this.messages.push(msg);

    // Trim to max history
    if (this.messages.length > this.maxHistory) {
      this.messages = this.messages.slice(this.messages.length - this.maxHistory);
    }
    return msg;
  }
}

// ========================================================================
// RateLimiter Durable Object (per-IP token bucket)
// ========================================================================

export class RateLimiter {
  constructor(state, env) {
    this.state = state;
    this.maxTokens = 5;
    this.refillRateMs = 1000; // 1 token per second
    this.buckets = new Map();
    this.totalChecks = 0;
    this.totalAllowed = 0;
    this.totalDenied = 0;
  }

  async fetch(request) {
    const url = new URL(request.url);
    const action = url.pathname.slice(1);
    const body = await request.json().catch(() => ({}));

    switch (action) {
      case "check_rate":
        return this.checkRate(body);
      case "stats":
        return this.getStats();
      default:
        return jsonResponse({ error: `unknown action: ${action}` }, 400);
    }
  }

  checkRate({ user_id }) {
    if (!user_id) return jsonResponse({ error: "user_id required" }, 400);

    const now = Date.now();
    let bucket = this.buckets.get(user_id);
    if (!bucket) {
      bucket = { tokens: this.maxTokens, lastRefill: now, allowed: 0, denied: 0 };
      this.buckets.set(user_id, bucket);
    }

    // Refill tokens
    const elapsed = now - bucket.lastRefill;
    const newTokens = Math.floor(elapsed / this.refillRateMs);
    if (newTokens > 0) {
      bucket.tokens = Math.min(this.maxTokens, bucket.tokens + newTokens);
      bucket.lastRefill = now;
    }

    const allowed = bucket.tokens > 0;
    if (allowed) {
      bucket.tokens--;
      bucket.allowed++;
      this.totalAllowed++;
    } else {
      bucket.denied++;
      this.totalDenied++;
    }
    this.totalChecks++;

    const retryAfterMs = allowed ? 0 : this.refillRateMs - (elapsed % this.refillRateMs);

    return jsonResponse({
      status: "ok",
      allowed,
      user_id,
      remaining: bucket.tokens,
      limit: this.maxTokens,
      retry_after_ms: retryAfterMs,
    });
  }

  getStats() {
    const denyRate = this.totalChecks > 0
      ? (this.totalDenied / this.totalChecks) * 100
      : 0;
    return jsonResponse({
      status: "ok",
      config: { max_tokens: this.maxTokens, refill_rate_ms: this.refillRateMs },
      counters: {
        total_checks: this.totalChecks,
        total_allowed: this.totalAllowed,
        total_denied: this.totalDenied,
        deny_rate_pct: denyRate,
        active_users: this.buckets.size,
      },
    });
  }
}

// ========================================================================
// Helpers
// ========================================================================

function jsonResponse(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
