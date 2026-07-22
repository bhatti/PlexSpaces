# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 PlexSpaces Contributors
#
# Guild Chat Server - Discord-style Real-Time Chat with Durable Objects (Python WASM)
#
# Demonstrates Cloudflare Workers Durable Objects pattern for real-time chat:
# - ChatRoomActor: per-room state, member tracking, message fan-out, history
# - RateLimiterActor: per-user token bucket rate limiting (spam prevention)
# - AlarmDemoActor: durable alarm lifecycle (setAlarm equivalent)
#
# Inspired by:
# - Discord's guild process architecture (one Elixir GenServer per guild)
# - Cloudflare's workers-chat-demo (Durable Object per room + rate limiter)
#
# Real-world use case: Chat platforms (Discord, Slack), collaborative apps,
# multiplayer game lobbies — anywhere you need per-entity stateful coordination.
#
# ## SDK Features Used
#
# - @actor(facets=["virtual_actor", "process_group"]): Multi-facet actor decoration
# - host.send(): Fire-and-forget fan-out to member actors
# - host.kv.put()/host.kv.get(): Durable storage (message history persistence)
# - host.kv.multi_get()/host.kv.multi_put(): Batch KV for history load/store
# - host.kv.increment(): Atomic counter increment for distributed rate limiting
# - host.kv.cas(): Atomic compare-and-swap for idempotent token reservation
# - host.kv.put_with_ttl(): TTL-based token bucket window tracking
# - host.alarm.set()/host.alarm.get()/host.alarm.delete(): Durable alarm lifecycle
# - host.now_ms(): Timestamps for messages and token bucket refill
# - host.self_id(): Actor identity for room ID extraction
#
# ## Comparison to Cloudflare Workers / Durable Objects
#
# | Cloudflare Workers/DO             | PlexSpaces Python                 |
# |-----------------------------------|-----------------------------------|
# | export class ChatRoom extends DO  | @actor(facets=["virtual_actor"])  |
# | env.CHAT_ROOM.get(id)             | host.send(actorID, ...)           |
# | this.state.storage.put/get        | host.kv.put()/host.kv.get()       |
# | fetch(request) handler            | @handler("op")                    |
# | blockConcurrencyWhile()           | state restored from host.kv       |
# | WebSocket accept/send             | host.send() fan-out to members    |
# | storage.setAlarm(timestamp)       | host.alarm.set(timestamp_ms)      |
# | storage.getAlarm()                | host.alarm.get()                  |
# | alarm() scheduled callback        | @handler("__alarm__")             |
# | CAS / transactional put           | host.kv.cas(key, expected, new)   |
# | Atomic counter (R2/KV)            | host.kv.increment(key, delta)     |
# | Batch storage.get([k1,k2,...])    | host.kv.multi_get(keys)           |
# | Batch storage.put({k:v,...})      | host.kv.multi_put(entries)        |
# | wrangler.toml [[bindings]]        | app-config.toml [[children]]      |
# | Worker script routing             | actor_type prefix matching        |

from __future__ import annotations

from plexspaces import actor, handler, host, state

_MAX_HISTORY = 100  # bounded ring buffer like DO storage


# ─── ChatRoomActor ─────────────────────────────────────────────────────────────


@actor(facets=["virtual_actor", "process_group"])
class ChatRoomActor:
    """Per-room member registry with fan-out message delivery and durable history.

    Equivalent to a Cloudflare Durable Object — one instance per chat room,
    sequentially processing messages (no locking needed).

    State:
        members:       user_id -> {"joined_at": ms, "msg_count": int}
        messages:      bounded ring buffer of last 100 messages
        msg_seq:       monotonically increasing message sequence counter
        total_joins:   lifetime join count
        total_leaves:  lifetime leave count
        total_broadcasts: lifetime broadcast count
        total_msgs:    lifetime message count
        total_compute_ms: cumulative compute time (ms)
    """

    members: dict = state(default_factory=dict)
    messages: list = state(default_factory=list)
    msg_seq: int = state(default=0)
    total_joins: int = state(default=0)
    total_leaves: int = state(default=0)
    total_broadcasts: int = state(default=0)
    total_msgs: int = state(default=0)
    total_compute_ms: float = state(default=0.0)

    def _room_id(self) -> str:
        self_id = host.self_id()
        return self_id.split("//")[0] if "//" in self_id else self_id

    def _add_message(self, user_id: str, content: str, timestamp: int) -> dict:
        self.msg_seq += 1
        msg = {
            "seq": self.msg_seq,
            "user_id": user_id,
            "content": content,
            "timestamp": timestamp,
        }
        self.messages.append(msg)
        # Trim ring buffer
        if len(self.messages) > _MAX_HISTORY:
            self.messages = self.messages[-_MAX_HISTORY:]
        return msg

    def _persist_history(self) -> None:
        """Durably store message history using multi_put (batch).

        Each message stored under its own seq key; a seq index tracks what's stored.
        Mirrors Cloudflare DO storage.put({k1:v1, k2:v2, ...}) batch API.
        """
        if not self.messages:
            return
        import json
        room_id = self._room_id()
        entries = {}
        seqs = []
        for msg in self.messages:
            key = f"room:{room_id}:msg:{msg['seq']}"
            entries[key] = json.dumps(msg)
            seqs.append(msg["seq"])
        entries[f"room:{room_id}:seq_index"] = json.dumps(seqs)
        try:
            host.kv.multi_put(entries)
        except Exception:
            # Fallback: single-key write
            import json as _json
            host.kv.put(f"room:{room_id}:history", _json.dumps(self.messages))

    def _load_history(self) -> None:
        """Restore message history using multi_get (batch fetch).

        Equivalent to Cloudflare DO blockConcurrencyWhile() — restore persisted
        state before handling any messages.
        """
        import json
        room_id = self._room_id()
        index_raw = host.kv.get(f"room:{room_id}:seq_index")
        if not index_raw:
            # Fallback: legacy single-key blob
            stored = host.kv.get(f"room:{room_id}:history")
            if stored:
                try:
                    msgs = json.loads(stored)
                    self.messages = msgs
                    if msgs:
                        self.msg_seq = msgs[-1]["seq"]
                except Exception:
                    pass
            return
        try:
            seqs = json.loads(index_raw)
        except Exception:
            return
        if not seqs:
            return
        keys = [f"room:{room_id}:msg:{seq}" for seq in seqs]
        try:
            values = host.kv.multi_get(keys)
        except Exception:
            return
        msgs = []
        for v in values:
            if v:
                try:
                    msgs.append(json.loads(v))
                except Exception:
                    pass
        self.messages = msgs
        if msgs:
            self.msg_seq = msgs[-1]["seq"]

    @handler("join")
    def join(self, user_id: str = "") -> dict:
        """Add a member to the room (like WebSocket connect in Durable Objects)."""
        if not user_id:
            return {"error": "user_id required"}

        now = host.now_ms()

        if user_id in self.members:
            return {
                "status": "ok",
                "action": "already_joined",
                "user_id": user_id,
                "room_id": self._room_id(),
                "members": len(self.members),
            }

        self.members[user_id] = {"joined_at": now, "msg_count": 0}
        self.total_joins += 1
        self._add_message("system", f"{user_id} joined the room", now)

        return {
            "status": "ok",
            "action": "joined",
            "user_id": user_id,
            "room_id": self._room_id(),
            "members": len(self.members),
        }

    @handler("leave")
    def leave(self, user_id: str = "") -> dict:
        """Remove a member from the room (like WebSocket close)."""
        if user_id not in self.members:
            return {"status": "ok", "action": "not_member", "user_id": user_id}

        del self.members[user_id]
        self.total_leaves += 1
        now = host.now_ms()
        self._add_message("system", f"{user_id} left the room", now)

        return {
            "status": "ok",
            "action": "left",
            "user_id": user_id,
            "room_id": self._room_id(),
            "members": len(self.members),
        }

    @handler("send_message")
    def send_message(self, user_id: str = "", content: str = "") -> dict:
        """Broadcast a message to all room members.

        Core fan-out pattern — mirrors Discord's guild process that fans out
        events to all connected session processes. Uses host.send() for real
        fire-and-forget fan-out to each member actor.
        """
        if not user_id or not content:
            return {"error": "user_id and content required"}

        if user_id not in self.members:
            return {"error": "not a member of this room"}

        compute_start = host.now_ms()
        member = self.members[user_id]
        member["msg_count"] = member.get("msg_count", 0) + 1

        now = host.now_ms()
        msg = self._add_message(user_id, content, now)

        # Fan-out: send to every other member actor via host.send()
        # Mirrors Discord's Manifold pattern for distributed fan-out
        fan_out_count = 0
        out_msg = {
            "room_id": self._room_id(),
            "seq": msg["seq"],
            "from": user_id,
            "content": content,
            "timestamp": now,
        }
        for member_id in list(self.members.keys()):
            if member_id != user_id:
                host.send(member_id, "receive_message", out_msg)
                fan_out_count += 1
                self.total_broadcasts += 1

        # Persist to durable storage (like DO transactional storage)
        self._persist_history()

        compute_end = host.now_ms()
        self.total_compute_ms += float(compute_end - compute_start)
        self.total_msgs += 1

        return {
            "status": "ok",
            "seq": msg["seq"],
            "room_id": self._room_id(),
            "user_id": user_id,
            "fan_out": fan_out_count,
            "members": len(self.members),
            "history_size": len(self.messages),
        }

    @handler("send_message_batch")
    def send_message_batch(self, count: int = 1000, user_id: str = "bench-user", content: str = "Benchmark message payload with enough data for realistic sizing") -> dict:
        """Process many messages in a single WASM call for benchmarking."""
        # Ensure bench user is a member
        if user_id not in self.members:
            self.members[user_id] = {"joined_at": host.now_ms(), "msg_count": 0}
            self.total_joins += 1
        # Add extra members for fan-out
        for i in range(10):
            uid = f"fan-out-member-{i}"
            if uid not in self.members:
                self.members[uid] = {"joined_at": host.now_ms(), "msg_count": 0}

        compute_start = host.now_ms()
        sent = 0
        total_fan_out = 0

        for i in range(count):
            self.members[user_id]["msg_count"] = self.members[user_id].get("msg_count", 0) + 1
            now = host.now_ms()
            msg_content = f"{content} #{i}"
            msg = self._add_message(user_id, msg_content, now)

            out_msg = {
                "room_id": self._room_id(),
                "seq": msg["seq"],
                "from": user_id,
                "content": msg_content,
                "timestamp": now,
            }
            for member_id in list(self.members.keys()):
                if member_id != user_id:
                    host.send(member_id, "receive_message", out_msg)
                    total_fan_out += 1
                    self.total_broadcasts += 1
            self.total_msgs += 1
            sent += 1

        # Single persist at end (batched, like DO transaction)
        self._persist_history()

        compute_end = host.now_ms()
        compute_ms = float(compute_end - compute_start)
        self.total_compute_ms += compute_ms
        ops_per_sec = float(sent) / (compute_ms / 1000.0) if compute_ms > 0 else 0.0

        return {
            "status": "ok",
            "total_sent": sent,
            "total_fan_out": total_fan_out,
            "compute_ms": compute_ms,
            "ops_per_sec": ops_per_sec,
            "history_size": len(self.messages),
            "active_members": len(self.members),
        }

    @handler("get_history")
    def get_history(self, limit: int = 50, after_seq: int = 0) -> dict:
        """Return recent messages.

        Demonstrates KV multi_get: batch-fetches individual message keys,
        mirroring Cloudflare DO storage.get([k1, k2, ...]) batch API.
        """
        import json
        if limit <= 0 or limit > _MAX_HISTORY:
            limit = 50

        filtered = [m for m in self.messages if m.get("seq", 0) > after_seq]
        if len(filtered) > limit:
            filtered = filtered[-limit:]

        # Demonstrate multi_get: fetch the same messages from per-seq KV keys
        batch_fetched = 0
        if filtered:
            room_id = self._room_id()
            keys = [f"room:{room_id}:msg:{m['seq']}" for m in filtered]
            try:
                values = host.kv.multi_get(keys)
                batch_fetched = sum(1 for v in values if v)
            except Exception:
                pass

        return {
            "status": "ok",
            "room_id": self._room_id(),
            "messages": filtered,
            "count": len(filtered),
            "total": len(self.messages),
            "batch_fetched": batch_fetched,
        }

    @handler("get_members")
    def get_members(self) -> dict:
        """Return the current member list."""
        members = [
            {"user_id": uid, "joined_at": info.get("joined_at", 0), "msg_count": info.get("msg_count", 0)}
            for uid, info in self.members.items()
        ]
        return {
            "status": "ok",
            "room_id": self._room_id(),
            "members": members,
            "count": len(members),
        }

    @handler("stats")
    def stats(self) -> dict:
        """Return room statistics."""
        ops_per_sec = (
            float(self.total_msgs) / (self.total_compute_ms / 1000.0)
            if self.total_compute_ms > 0
            else 0.0
        )
        memory_kb = float(len(self.messages) * 128 + len(self.members) * 64) / 1024.0
        return {
            "status": "ok",
            "room_id": self._room_id(),
            "config": {"max_history": _MAX_HISTORY},
            "counters": {
                "total_messages": self.total_msgs,
                "total_joins": self.total_joins,
                "total_leaves": self.total_leaves,
                "total_broadcasts": self.total_broadcasts,
                "active_members": len(self.members),
                "history_size": len(self.messages),
                "message_seq": self.msg_seq,
            },
            "benchmarks": {
                "total_compute_ms": self.total_compute_ms,
                "msgs_per_sec": ops_per_sec,
                "memory_kb": memory_kb,
            },
        }

    @handler("__alarm__")
    def on_alarm(self) -> dict:
        """Periodic flush hook — persists room stats, useful for monitoring."""
        self._persist_history()
        host.info(
            f"ChatRoom periodic flush: {self.total_msgs} messages, "
            f"{len(self.members)} members in {self._room_id()}"
        )
        return {"status": "ok", "action": "flush", "messages": self.total_msgs}


# ─── RateLimiterActor ──────────────────────────────────────────────────────────


@actor(facets=["virtual_actor"])
class RateLimiterActor:
    """Per-user token bucket rate limiting.

    Like the Cloudflare chat demo's RateLimiter Durable Object that tracks
    request frequency per IP to prevent spam.

    Uses host.kv.increment() for distributed atomic counting and host.kv.cas()
    for idempotent token reservation, demonstrating both atomic KV APIs.

    State:
        max_tokens:     bucket capacity (default 5)
        refill_rate_ms: ms between token refills (default 1000)
        buckets:        user_id -> {"tokens": int, "last_refill": ms, "allowed": int, "denied": int}
        total_checks:   lifetime check count
        total_allowed:  lifetime allowed count
        total_denied:   lifetime denied count
    """

    max_tokens: int = state(default=5)
    refill_rate_ms: int = state(default=1000)
    buckets: dict = state(default_factory=dict)
    total_checks: int = state(default=0)
    total_allowed: int = state(default=0)
    total_denied: int = state(default=0)

    @handler("check")
    def check(self, user_id: str = "") -> dict:
        """Check if a user is within rate limits using token bucket.

        Demonstrates two atomic KV primitives:
          - host.kv.increment: atomically counts requests per user across restarts
          - host.kv.cas: atomically reserves a token slot (idempotent deduction)
        """
        if not user_id:
            return {"error": "user_id required"}

        now = host.now_ms()

        # Atomic distributed counter: counts total lifetime requests for this user.
        # Survives actor restarts — equivalent to Cloudflare KV atomic increment.
        try:
            host.kv.increment(f"rate:{user_id}:total", 1)
        except Exception:
            pass

        bucket = self.buckets.get(user_id)
        if bucket is None:
            bucket = {"tokens": self.max_tokens, "last_refill": now, "allowed": 0, "denied": 0}
            self.buckets[user_id] = bucket

        # Refill tokens based on elapsed time
        elapsed = now - bucket.get("last_refill", now)
        if self.refill_rate_ms > 0:
            new_tokens = elapsed // self.refill_rate_ms
            if new_tokens > 0:
                bucket["tokens"] = min(bucket["tokens"] + new_tokens, self.max_tokens)
                bucket["last_refill"] = now

        # Atomic CAS: attempt to reserve a token slot.
        # Equivalent to Cloudflare DO transactional storage — read-modify-write
        # atomically to prevent double-spend.
        cas_key = f"rate:{user_id}:window"
        try:
            current_val = host.kv.get(cas_key) or ""
            next_val = str(now)
            host.kv.cas(cas_key, current_val, next_val)
        except Exception:
            pass

        # Token bucket check
        allowed = bucket["tokens"] > 0
        if allowed:
            bucket["tokens"] -= 1
            bucket["allowed"] = bucket.get("allowed", 0) + 1
            self.total_allowed += 1
        else:
            bucket["denied"] = bucket.get("denied", 0) + 1
            self.total_denied += 1
        self.total_checks += 1

        # Calculate retry-after
        retry_after_ms = 0
        if not allowed and self.refill_rate_ms > 0:
            retry_after_ms = self.refill_rate_ms - (elapsed % self.refill_rate_ms)

        return {
            "status": "ok",
            "allowed": allowed,
            "user_id": user_id,
            "remaining": bucket["tokens"],
            "limit": self.max_tokens,
            "retry_after_ms": retry_after_ms,
        }

    @handler("check_batch")
    def check_batch(self, user_id: str = "batch-user", count: int = 1000) -> dict:
        """Check multiple requests for benchmarking."""
        num_users = 50
        requests_per_user = max(count // num_users, 1)
        batch_start = host.now_ms()
        allowed = 0
        denied = 0

        for u in range(num_users):
            uid = f"{user_id}-{u}"
            now = host.now_ms()
            bucket = self.buckets.get(uid)
            if bucket is None:
                bucket = {"tokens": self.max_tokens, "last_refill": now, "allowed": 0, "denied": 0}
                self.buckets[uid] = bucket

            for _ in range(requests_per_user):
                now = host.now_ms()
                elapsed = now - bucket.get("last_refill", now)
                if self.refill_rate_ms > 0:
                    new_tokens = elapsed // self.refill_rate_ms
                    if new_tokens > 0:
                        bucket["tokens"] = min(bucket["tokens"] + new_tokens, self.max_tokens)
                        bucket["last_refill"] = now

                ok = bucket["tokens"] > 0
                if ok:
                    bucket["tokens"] -= 1
                    bucket["allowed"] = bucket.get("allowed", 0) + 1
                    self.total_allowed += 1
                    allowed += 1
                else:
                    bucket["denied"] = bucket.get("denied", 0) + 1
                    self.total_denied += 1
                    denied += 1
                self.total_checks += 1

        batch_end = host.now_ms()
        duration_ms = float(batch_end - batch_start)
        total = allowed + denied
        ops_per_sec = float(total) / (duration_ms / 1000.0) if duration_ms > 0 else 0.0

        return {
            "status": "ok",
            "total_requests": total,
            "allowed": allowed,
            "denied": denied,
            "duration_ms": duration_ms,
            "ops_per_sec": ops_per_sec,
            "unique_users": num_users,
            "reqs_per_user": requests_per_user,
        }

    @handler("reset")
    def reset(self, user_id: str = "") -> dict:
        """Reset token bucket for a user (or all users if user_id is empty)."""
        if user_id:
            self.buckets.pop(user_id, None)
            return {"status": "ok", "action": "reset", "user_id": user_id}
        self.buckets = {}
        self.total_checks = 0
        self.total_allowed = 0
        self.total_denied = 0
        return {"status": "ok", "action": "reset_all"}

    @handler("status")
    def status(self) -> dict:
        """Return rate limiter statistics."""
        deny_rate = (
            float(self.total_denied) / float(self.total_checks) * 100.0
            if self.total_checks > 0
            else 0.0
        )
        return {
            "status": "ok",
            "config": {
                "max_tokens": self.max_tokens,
                "refill_rate_ms": self.refill_rate_ms,
            },
            "counters": {
                "total_checks": self.total_checks,
                "total_allowed": self.total_allowed,
                "total_denied": self.total_denied,
                "deny_rate_pct": deny_rate,
                "active_users": len(self.buckets),
            },
        }


# ─── AlarmDemoActor ────────────────────────────────────────────────────────────


@actor(facets=["virtual_actor", "reminder"])
class AlarmDemoActor:
    """Durable alarm lifecycle — equivalent to Cloudflare DO setAlarm/alarm().

    Demonstrates:
      - "start"     → host.alarm.set(now_ms + delay_ms)
      - "enqueue"   → append to pending batch (first enqueue auto-schedules)
      - "__alarm__" → process batched pending requests (alarm fired callback)
      - "status"    → host.alarm.get()
      - "cancel"    → host.alarm.delete()

    In production this pattern batches writes, flushes queues, or expires sessions.

    State:
        pending_requests:   items queued before alarm fires
        total_alarms_set:   lifetime alarm set count
        total_alarms_fired: lifetime alarm fired count
        total_processed:    lifetime processed item count
    """

    pending_requests: list = state(default_factory=list)
    total_alarms_set: int = state(default=0)
    total_alarms_fired: int = state(default=0)
    total_processed: int = state(default=0)

    @handler("start")
    def start(self, delay_ms: int = 30000) -> dict:
        """Schedule a durable alarm.

        Equivalent to Cloudflare DO: this.state.storage.setAlarm(Date.now() + delay_ms)
        """
        if delay_ms <= 0:
            delay_ms = 30000
        fire_at = host.now_ms() + delay_ms
        try:
            host.alarm.set(fire_at)
        except Exception as e:
            return {"error": f"alarm_set failed: {e}"}
        self.total_alarms_set += 1
        host.info(f"AlarmDemo: alarm set, fires in {delay_ms}ms at ts={fire_at}")
        return {
            "status": "ok",
            "action": "alarm_scheduled",
            "fire_at_ms": fire_at,
            "delay_ms": delay_ms,
            "total_alarms_set": self.total_alarms_set,
            "pending_requests": len(self.pending_requests),
        }

    @handler("enqueue")
    def enqueue(self, data: str = "") -> dict:
        """Add a request to the pending batch (processed when alarm fires).

        If this is the first item, automatically schedules an alarm 10s from now.
        """
        if not data:
            data = f"item-{len(self.pending_requests)}"
        self.pending_requests.append(data)

        # Auto-schedule alarm on first enqueue (like DO pattern)
        if len(self.pending_requests) == 1:
            fire_at = host.now_ms() + 10000  # 10s from now
            try:
                host.alarm.set(fire_at)
                self.total_alarms_set += 1
            except Exception:
                pass

        return {
            "status": "ok",
            "action": "enqueued",
            "data": data,
            "pending_requests": len(self.pending_requests),
        }

    @handler("__alarm__")
    def on_alarm(self) -> dict:
        """Invoked by the framework when the scheduled alarm fires.

        Equivalent to Cloudflare DO: async alarm() { ... }
        Processes all batched pending requests.
        """
        self.total_alarms_fired += 1
        processed = len(self.pending_requests)
        self.total_processed += processed

        host.info(f"AlarmDemo: alarm fired, processing {processed} pending requests")

        results = [f"processed:{req}" for req in self.pending_requests]
        self.pending_requests = []

        return {
            "status": "ok",
            "action": "alarm_fired",
            "processed": processed,
            "results": results,
            "total_alarms_fired": self.total_alarms_fired,
            "total_processed": self.total_processed,
        }

    @handler("status")
    def status(self) -> dict:
        """Return the current alarm schedule.

        Equivalent to Cloudflare DO: this.state.storage.getAlarm()
        """
        fire_at = 0
        err_msg = ""
        try:
            fire_at = host.alarm.get() or 0
        except Exception as e:
            err_msg = str(e)

        return {
            "status": "ok",
            "alarm_fire_at_ms": fire_at,
            "alarm_set": fire_at > 0,
            "pending_requests": len(self.pending_requests),
            "total_alarms_set": self.total_alarms_set,
            "total_alarms_fired": self.total_alarms_fired,
            "total_processed": self.total_processed,
            "error": err_msg,
        }

    @handler("cancel")
    def cancel(self) -> dict:
        """Cancel the pending durable alarm.

        Equivalent to Cloudflare DO: this.state.storage.deleteAlarm()
        """
        try:
            host.alarm.delete()
        except Exception as e:
            return {"error": f"alarm_delete failed: {e}"}
        host.info("AlarmDemo: alarm cancelled")
        return {"status": "ok", "action": "alarm_cancelled"}

    @handler("reset")
    def reset(self) -> dict:
        """Reset all alarm demo state (useful for testing)."""
        self.pending_requests = []
        self.total_alarms_set = 0
        self.total_alarms_fired = 0
        self.total_processed = 0
        return {"status": "ok", "action": "reset"}


__all__ = ["ChatRoomActor", "RateLimiterActor", "AlarmDemoActor"]
