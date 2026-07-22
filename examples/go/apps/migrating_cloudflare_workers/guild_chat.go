// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Guild Chat Server - Discord-style Real-Time Chat with Durable Objects (Go WASM)
//
// Demonstrates Cloudflare Workers Durable Objects pattern for real-time chat:
// - ChatRoom actor: per-room state, member tracking, message fan-out, history
// - RateLimiter actor: per-user token bucket rate limiting (spam prevention)
// - AlarmDemo actor: durable alarm lifecycle (setAlarm equivalent)
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
// - plexspaces.ActorRouter: Multi-actor routing (ChatRoom + RateLimiter + AlarmDemo)
// - plexspaces.BaseActor: Actor base with JSON state serialization
// - plexspaces.Host: Host function wrappers (Ask, Send, NowMs, KV, Alarms, etc.)
// - Init(): Actor initialization from framework config
// - Handle(): Message routing by msg-type
// - GetState()/SetState(): Checkpoint-based state persistence
// - host.Send(): Fire-and-forget messaging (real fan-out to member actors)
// - host.KV().Put()/KVGet(): Durable storage (message history persistence)
// - host.KV().CAS(): Atomic compare-and-swap for idempotent operations
// - host.KV().Increment(): Atomic counter increment for distributed rate limiting
// - host.KV().MultiGet()/KVMultiPut(): Batch KV for history load/store
// - host.Alarm().Set()/AlarmGet()/AlarmDelete(): Durable alarm lifecycle
//
// ## Comparison to Cloudflare Workers / Durable Objects
//
// | Cloudflare Workers/DO             | PlexSpaces Go                     |
// |-----------------------------------|-----------------------------------|
// | export class ChatRoom extends DO  | ChatRoom struct + BaseActor       |
// | env.CHAT_ROOM.get(id)             | host.Ask(actorID, ...)            |
// | this.state.storage.put/get        | host.KVPut/KVGet + GetState       |
// | fetch(request) handler            | Handle(from, msgType, payload)    |
// | blockConcurrencyWhile()           | Init() (runs before any Handle)   |
// | WebSocket accept/send             | host.Send() fan-out to members    |
// | storage.setAlarm(timestamp)       | host.Alarm().Set(timestampMs)        |
// | storage.getAlarm()                | host.Alarm().Get()                   |
// | alarm() scheduled callback        | "__alarm__" message handler       |
// | CAS / transactional put           | host.KV().CAS(key, expected, new)    |
// | Atomic counter (R2/KV)            | host.KV().Increment(key, delta)      |
// | Batch storage.get([k1,k2,...])    | host.KV().MultiGet(keys)             |
// | Batch storage.put({k:v,...})      | host.KV().MultiPut(entries)          |
// | wrangler.toml [[bindings]]        | app-config.toml [[children]]      |
// | Worker script routing             | ActorRouter prefix matching       |

package main

import (
	"encoding/json"
	"fmt"
	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ========================================================================
// ChatRoom Actor (Durable Object equivalent)
// ========================================================================

// ChatRoom manages a single chat room with members, message history, and fan-out.
// Each room is an isolated actor — like a Durable Object instance per room.
// Processes messages sequentially (single-threaded, no locks needed).
type ChatRoom struct {
	plexspaces.BaseActor

	// Room identity
	RoomID string `json:"room_id"`

	// Connected members: user_id -> MemberInfo
	Members map[string]*MemberInfo `json:"members"`

	// Message history (bounded ring buffer, like DO storage)
	Messages   []ChatMessage `json:"messages"`
	MaxHistory int           `json:"max_history"`
	MessageSeq uint64        `json:"message_seq"`

	// Room statistics
	TotalMessages   int     `json:"total_messages"`
	TotalJoins      int     `json:"total_joins"`
	TotalLeaves     int     `json:"total_leaves"`
	TotalBroadcasts int     `json:"total_broadcasts"`
	TotalComputeMs  float64 `json:"total_compute_ms"`
}

// MemberInfo tracks a connected member.
type MemberInfo struct {
	UserID   string `json:"user_id"`
	JoinedAt uint64 `json:"joined_at"`
	MsgCount int    `json:"msg_count"`
}

// ChatMessage is a single message in the room history.
type ChatMessage struct {
	Seq       uint64 `json:"seq"`
	UserID    string `json:"user_id"`
	Content   string `json:"content"`
	Timestamp uint64 `json:"timestamp"`
}

var host = plexspaces.NewHost()

func NewChatRoom() plexspaces.Actor {
	a := &ChatRoom{
		Members:    make(map[string]*MemberInfo),
		Messages:   make([]ChatMessage, 0),
		MaxHistory: 100, // Keep last 100 messages (like DO storage limit)
	}
	a.SetSelf(a)
	return a
}

func (c *ChatRoom) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	c.RoomID = config.ActorID

	if args := config.Args; args != nil {
		if v, ok := args["max_history"]; ok {
			c.MaxHistory = toInt(v)
		}
	}
	if c.MaxHistory == 0 {
		c.MaxHistory = 100
	}

	// Like Durable Object blockConcurrencyWhile() — restore persisted state.
	// Use KVMultiGet for batch history load: fetch seq index + individual message keys.
	c.loadHistoryBatch()

	host.Info(fmt.Sprintf("ChatRoom %s: max_history=%d, restored=%d messages",
		c.RoomID, c.MaxHistory, len(c.Messages)))
	return ""
}

// loadHistoryBatch restores message history using KVMultiGet (batch fetch).
// Falls back to the legacy single-key format if the index key is absent.
func (c *ChatRoom) loadHistoryBatch() {
	indexKey := "room:" + c.RoomID + ":seq_index"
	indexRaw, _ := host.KV().Get(indexKey)
	if indexRaw == "" {
		// Fallback: legacy single-key history blob
		stored, _ := host.KV().Get("room:" + c.RoomID + ":history")
		if stored != "" {
			var msgs []ChatMessage
			if err := json.Unmarshal([]byte(stored), &msgs); err == nil {
				c.Messages = msgs
				if len(msgs) > 0 {
					c.MessageSeq = msgs[len(msgs)-1].Seq
				}
			}
		}
		return
	}

	// Parse the seq index (list of seq numbers stored)
	var seqs []uint64
	if err := json.Unmarshal([]byte(indexRaw), &seqs); err != nil || len(seqs) == 0 {
		return
	}

	// Build keys for batch fetch
	keys := make([]string, len(seqs))
	for i, seq := range seqs {
		keys[i] = fmt.Sprintf("room:%s:msg:%d", c.RoomID, seq)
	}

	values, err := host.KV().MultiGet(keys)
	if err != nil {
		host.Info(fmt.Sprintf("ChatRoom %s: KVMultiGet error: %v", c.RoomID, err))
		return
	}

	msgs := make([]ChatMessage, 0, len(seqs))
	for i, v := range values {
		if v == "" {
			continue
		}
		var msg ChatMessage
		if err := json.Unmarshal([]byte(v), &msg); err == nil {
			msgs = append(msgs, msg)
		} else {
			host.Info(fmt.Sprintf("ChatRoom %s: skip key %s: %v", c.RoomID, keys[i], err))
		}
	}
	c.Messages = msgs
	if len(msgs) > 0 {
		c.MessageSeq = msgs[len(msgs)-1].Seq
	}
}

func (c *ChatRoom) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "join":
		return c.handleJoin(payloadJSON)
	case "leave":
		return c.handleLeave(payloadJSON)
	case "send_message":
		return c.handleSendMessage(payloadJSON)
	case "send_message_batch":
		return c.handleSendMessageBatch(payloadJSON)
	case "get_history":
		return c.handleGetHistory(payloadJSON)
	case "get_members":
		return c.handleGetMembers()
	case "stats":
		return c.handleStats()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleJoin adds a member to the room (like WebSocket connect in Durable Objects).
func (c *ChatRoom) handleJoin(payloadJSON string) string {
	var req struct {
		UserID string `json:"user_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.UserID == "" {
		return marshal(map[string]any{"error": "user_id required"})
	}

	now := host.NowMs()

	// Check if already a member
	if _, exists := c.Members[req.UserID]; exists {
		return marshal(map[string]any{
			"status":  "ok",
			"action":  "already_joined",
			"user_id": req.UserID,
			"room_id": c.RoomID,
			"members": len(c.Members),
		})
	}

	c.Members[req.UserID] = &MemberInfo{
		UserID:   req.UserID,
		JoinedAt: now,
	}
	c.TotalJoins++

	// Add system message to history
	c.addMessage("system", fmt.Sprintf("%s joined the room", req.UserID), now)

	return marshal(map[string]any{
		"status":  "ok",
		"action":  "joined",
		"user_id": req.UserID,
		"room_id": c.RoomID,
		"members": len(c.Members),
	})
}

// handleLeave removes a member from the room (like WebSocket close).
func (c *ChatRoom) handleLeave(payloadJSON string) string {
	var req struct {
		UserID string `json:"user_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}

	if _, exists := c.Members[req.UserID]; !exists {
		return marshal(map[string]any{
			"status":  "ok",
			"action":  "not_member",
			"user_id": req.UserID,
		})
	}

	delete(c.Members, req.UserID)
	c.TotalLeaves++

	now := host.NowMs()
	c.addMessage("system", fmt.Sprintf("%s left the room", req.UserID), now)

	return marshal(map[string]any{
		"status":  "ok",
		"action":  "left",
		"user_id": req.UserID,
		"room_id": c.RoomID,
		"members": len(c.Members),
	})
}

// handleSendMessage broadcasts a message to all room members.
// This is the core fan-out pattern — like Discord's guild process
// that fans out events to all connected session processes.
// Uses host.Send() for real fire-and-forget fan-out to each member actor.
func (c *ChatRoom) handleSendMessage(payloadJSON string) string {
	var req struct {
		UserID  string `json:"user_id"`
		Content string `json:"content"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.UserID == "" || req.Content == "" {
		return marshal(map[string]any{"error": "user_id and content required"})
	}

	computeStart := host.NowMs()

	// Verify sender is a member
	member, isMember := c.Members[req.UserID]
	if !isMember {
		return marshal(map[string]any{"error": "not a member of this room"})
	}
	member.MsgCount++

	now := host.NowMs()

	// Add to history (like Durable Object storage.put)
	msg := c.addMessage(req.UserID, req.Content, now)

	// Build the broadcast payload once
	outMsg := marshal(map[string]any{
		"room_id":   c.RoomID,
		"seq":       msg.Seq,
		"from":      req.UserID,
		"content":   req.Content,
		"timestamp": now,
	})

	// Real fan-out: send to every other member actor via host.Send().
	// Each member is treated as a session actor — mirrors Discord's Manifold
	// pattern where the guild process fans out to all connected session processes.
	fanOutCount := 0
	for memberID := range c.Members {
		if memberID != req.UserID {
			host.Send(memberID, "receive_message", outMsg)
			fanOutCount++
			c.TotalBroadcasts++
		}
	}

	// Persist to durable storage (like DO transactional storage)
	c.persistHistory()

	computeEnd := host.NowMs()
	c.TotalComputeMs += float64(computeEnd - computeStart)
	c.TotalMessages++

	return marshal(map[string]any{
		"status":       "ok",
		"seq":          msg.Seq,
		"room_id":      c.RoomID,
		"user_id":      req.UserID,
		"fan_out":      fanOutCount,
		"members":      len(c.Members),
		"history_size": len(c.Messages),
	})
}

// handleSendMessageBatch processes many messages in a single WASM call for benchmarking.
// This measures pure computation (JSON parsing, member lookup, history management,
// fan-out counting) without per-message HTTP coordination overhead.
func (c *ChatRoom) handleSendMessageBatch(payloadJSON string) string {
	var req struct {
		Count   int    `json:"count"`
		UserID  string `json:"user_id"`
		Content string `json:"content"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.Count <= 0 {
		req.Count = 1000
	}
	if req.UserID == "" {
		req.UserID = "bench-user"
	}
	if req.Content == "" {
		req.Content = "Benchmark message payload with enough data for realistic sizing"
	}

	// Ensure bench user is a member
	if _, exists := c.Members[req.UserID]; !exists {
		c.Members[req.UserID] = &MemberInfo{
			UserID:   req.UserID,
			JoinedAt: host.NowMs(),
		}
		c.TotalJoins++
	}
	// Add some extra members for fan-out
	for i := 0; i < 10; i++ {
		uid := fmt.Sprintf("fan-out-member-%d", i)
		if _, exists := c.Members[uid]; !exists {
			c.Members[uid] = &MemberInfo{UserID: uid, JoinedAt: host.NowMs()}
		}
	}

	computeStart := host.NowMs()

	sent := 0
	totalFanOut := 0
	for i := 0; i < req.Count; i++ {
		member := c.Members[req.UserID]
		member.MsgCount++

		now := host.NowMs()
		msg := c.addMessage(req.UserID, fmt.Sprintf("%s #%d", req.Content, i), now)

		outMsg := marshal(map[string]any{
			"room_id":   c.RoomID,
			"seq":       msg.Seq,
			"from":      req.UserID,
			"content":   fmt.Sprintf("%s #%d", req.Content, i),
			"timestamp": now,
		})

		// Fan-out via host.Send() to each member actor
		for memberID := range c.Members {
			if memberID != req.UserID {
				host.Send(memberID, "receive_message", outMsg)
				totalFanOut++
				c.TotalBroadcasts++
			}
		}
		c.TotalMessages++
		sent++
	}

	// Single persist at end (batched, like DO transaction)
	c.persistHistory()

	computeEnd := host.NowMs()
	computeMs := float64(computeEnd - computeStart)
	c.TotalComputeMs += computeMs

	opsPerSec := 0.0
	if computeMs > 0 {
		opsPerSec = float64(sent) / (computeMs / 1000.0)
	}

	return marshal(map[string]any{
		"status":         "ok",
		"total_sent":     sent,
		"total_fan_out":  totalFanOut,
		"compute_ms":     computeMs,
		"ops_per_sec":    opsPerSec,
		"history_size":   len(c.Messages),
		"active_members": len(c.Members),
	})
}

// handleGetHistory returns recent messages (like DO storage.list).
// Uses KVMultiGet to batch-fetch individual message keys when they are
// stored per-seq, demonstrating the batch KV API.
func (c *ChatRoom) handleGetHistory(payloadJSON string) string {
	var req struct {
		Limit    int    `json:"limit"`
		AfterSeq uint64 `json:"after_seq"`
	}
	if payloadJSON != "" && payloadJSON != "{}" {
		json.Unmarshal([]byte(payloadJSON), &req)
	}
	if req.Limit <= 0 || req.Limit > c.MaxHistory {
		req.Limit = 50
	}

	var filtered []ChatMessage
	for _, msg := range c.Messages {
		if msg.Seq > req.AfterSeq {
			filtered = append(filtered, msg)
		}
	}

	// Return last N messages
	start := 0
	if len(filtered) > req.Limit {
		start = len(filtered) - req.Limit
	}
	result := filtered[start:]

	// Demonstrate KVMultiGet: fetch the same messages from per-seq KV keys.
	// This shows how batch KV maps directly to Cloudflare DO storage.get([k1,k2,...]).
	var batchFetched int
	if len(result) > 0 {
		keys := make([]string, len(result))
		for i, msg := range result {
			keys[i] = fmt.Sprintf("room:%s:msg:%d", c.RoomID, msg.Seq)
		}
		values, err := host.KV().MultiGet(keys)
		if err == nil {
			for _, v := range values {
				if v != "" {
					batchFetched++
				}
			}
		}
	}

	return marshal(map[string]any{
		"status":        "ok",
		"room_id":       c.RoomID,
		"messages":      result,
		"count":         len(result),
		"total":         len(c.Messages),
		"batch_fetched": batchFetched,
	})
}

// handleGetMembers returns the current member list.
func (c *ChatRoom) handleGetMembers() string {
	members := make([]map[string]any, 0, len(c.Members))
	for _, m := range c.Members {
		members = append(members, map[string]any{
			"user_id":   m.UserID,
			"joined_at": m.JoinedAt,
			"msg_count": m.MsgCount,
		})
	}
	return marshal(map[string]any{
		"status":  "ok",
		"room_id": c.RoomID,
		"members": members,
		"count":   len(members),
	})
}

// handleStats returns room statistics.
func (c *ChatRoom) handleStats() string {
	totalTime := c.TotalComputeMs
	opsPerSec := 0.0
	if totalTime > 0 {
		opsPerSec = float64(c.TotalMessages) / (totalTime / 1000.0)
	}

	// Memory estimate
	memoryKB := float64(len(c.Messages)*128+len(c.Members)*64) / 1024.0

	return marshal(map[string]any{
		"status":  "ok",
		"room_id": c.RoomID,
		"config": map[string]any{
			"max_history": c.MaxHistory,
		},
		"counters": map[string]any{
			"total_messages":   c.TotalMessages,
			"total_joins":      c.TotalJoins,
			"total_leaves":     c.TotalLeaves,
			"total_broadcasts": c.TotalBroadcasts,
			"active_members":   len(c.Members),
			"history_size":     len(c.Messages),
			"message_seq":      c.MessageSeq,
		},
		"benchmarks": map[string]any{
			"total_compute_ms": c.TotalComputeMs,
			"msgs_per_sec":     opsPerSec,
			"memory_kb":        memoryKB,
		},
	})
}

// addMessage appends a message to the history ring buffer.
func (c *ChatRoom) addMessage(userID, content string, timestamp uint64) ChatMessage {
	c.MessageSeq++
	msg := ChatMessage{
		Seq:       c.MessageSeq,
		UserID:    userID,
		Content:   content,
		Timestamp: timestamp,
	}

	c.Messages = append(c.Messages, msg)

	// Trim to max history (ring buffer behavior)
	if len(c.Messages) > c.MaxHistory {
		c.Messages = c.Messages[len(c.Messages)-c.MaxHistory:]
	}

	return msg
}

// persistHistory durably stores message history using KVMultiPut (batch write).
// Stores each message under its own key ("room:{id}:msg:{seq}") plus a seq index.
// This mirrors Cloudflare DO storage.put({k1:v1, k2:v2, ...}) batch API.
func (c *ChatRoom) persistHistory() {
	if len(c.Messages) == 0 {
		return
	}

	entries := make(map[string]string, len(c.Messages)+1)

	// Store each message individually keyed by seq
	seqs := make([]uint64, 0, len(c.Messages))
	for _, msg := range c.Messages {
		data, err := json.Marshal(msg)
		if err == nil {
			entries[fmt.Sprintf("room:%s:msg:%d", c.RoomID, msg.Seq)] = string(data)
			seqs = append(seqs, msg.Seq)
		}
	}

	// Store seq index so Init can reconstruct the key list on restore
	if idxData, err := json.Marshal(seqs); err == nil {
		entries["room:"+c.RoomID+":seq_index"] = string(idxData)
	}

	if err := host.KV().MultiPut(entries); err != nil {
		// Fallback: legacy single-key write
		data, merr := json.Marshal(c.Messages)
		if merr == nil {
			host.KV().Put("room:"+c.RoomID+":history", string(data))
		}
	}
}

// ========================================================================
// RateLimiter Actor (Durable Object for per-user rate limiting)
// ========================================================================

// RateLimiter implements per-user token bucket rate limiting.
// Like the Cloudflare chat demo's RateLimiter Durable Object that
// tracks request frequency per IP to prevent spam.
//
// Uses host.KVIncrement for distributed atomic counting and host.KVCAS
// for idempotent token reservation, demonstrating both atomic KV APIs.
type RateLimiter struct {
	plexspaces.BaseActor

	// Token bucket config
	MaxTokens    int    `json:"max_tokens"`
	RefillRateMs uint64 `json:"refill_rate_ms"` // ms between token refills

	// Per-user buckets: user_id -> bucket
	Buckets map[string]*TokenBucket `json:"buckets"`

	// Stats
	TotalChecks  int `json:"total_checks"`
	TotalAllowed int `json:"total_allowed"`
	TotalDenied  int `json:"total_denied"`
}

// TokenBucket tracks tokens for a single user.
type TokenBucket struct {
	Tokens     int    `json:"tokens"`
	LastRefill uint64 `json:"last_refill"`
	Allowed    int    `json:"allowed"`
	Denied     int    `json:"denied"`
}

func NewRateLimiter() plexspaces.Actor {
	a := &RateLimiter{
		MaxTokens:    5,    // 5 messages
		RefillRateMs: 1000, // 1 token per second
		Buckets:      make(map[string]*TokenBucket),
	}
	a.SetSelf(a)
	return a
}

func (r *RateLimiter) Init(configJSON string) string {
	var config struct {
		ActorID string         `json:"actor_id"`
		Args    map[string]any `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	r.SetRuntimeMetadata(config.ActorID)

	if args := config.Args; args != nil {
		if v, ok := args["max_tokens"]; ok {
			r.MaxTokens = toInt(v)
		}
		if v, ok := args["refill_rate_ms"]; ok {
			r.RefillRateMs = toUint64(v)
		}
	}
	if r.MaxTokens == 0 {
		r.MaxTokens = 5
	}
	if r.RefillRateMs == 0 {
		r.RefillRateMs = 1000
	}

	host.Info(fmt.Sprintf("RateLimiter %s: max_tokens=%d, refill_rate=%dms",
		r.ActorID(), r.MaxTokens, r.RefillRateMs))
	return ""
}

func (r *RateLimiter) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "check_rate":
		return r.checkRate(payloadJSON)
	case "check_rate_batch":
		return r.checkRateBatch(payloadJSON)
	case "stats":
		return r.getStats()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// checkRate checks if a user is within rate limits using token bucket.
// Demonstrates two atomic KV primitives:
//   - host.KVIncrement: atomically counts requests per user across restarts
//   - host.KVCAS: atomically reserves a token slot (idempotent deduction)
func (r *RateLimiter) checkRate(payloadJSON string) string {
	var req struct {
		UserID string `json:"user_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.UserID == "" {
		return marshal(map[string]any{"error": "user_id required"})
	}

	now := host.NowMs()

	// Atomic distributed counter: counts total lifetime requests for this user.
	// Survives actor restarts — unlike the in-process bucket which resets on restart.
	// Equivalent to Cloudflare KV atomic increment for distributed rate limiting.
	_ = host.KV().Increment("rate:"+req.UserID+":total", 1)

	bucket, exists := r.Buckets[req.UserID]
	if !exists {
		bucket = &TokenBucket{
			Tokens:     r.MaxTokens,
			LastRefill: now,
		}
		r.Buckets[req.UserID] = bucket
	}

	// Refill tokens based on elapsed time
	elapsed := now - bucket.LastRefill
	if r.RefillRateMs > 0 {
		newTokens := int(elapsed / r.RefillRateMs)
		if newTokens > 0 {
			bucket.Tokens += newTokens
			if bucket.Tokens > r.MaxTokens {
				bucket.Tokens = r.MaxTokens
			}
			bucket.LastRefill = now
		}
	}

	// Atomic CAS: attempt to reserve a token slot.
	// Demonstrates host.KVCAS — equivalent to Cloudflare DO transactional storage
	// where you read-modify-write atomically to prevent double-spend.
	casKey := fmt.Sprintf("rate:%s:window", req.UserID)
	currentVal, _ := host.KV().Get(casKey)
	nextVal := fmt.Sprintf("%d", now)
	_, _ = host.KV().CAS(casKey, currentVal, nextVal)

	// Check if tokens available (in-process token bucket)
	allowed := bucket.Tokens > 0
	if allowed {
		bucket.Tokens--
		bucket.Allowed++
		r.TotalAllowed++
	} else {
		bucket.Denied++
		r.TotalDenied++
	}
	r.TotalChecks++

	// Calculate retry-after
	retryAfterMs := uint64(0)
	if !allowed && r.RefillRateMs > 0 {
		retryAfterMs = r.RefillRateMs - (elapsed % r.RefillRateMs)
	}

	return marshal(map[string]any{
		"status":         "ok",
		"allowed":        allowed,
		"user_id":        req.UserID,
		"remaining":      bucket.Tokens,
		"limit":          r.MaxTokens,
		"retry_after_ms": retryAfterMs,
	})
}

// checkRateBatch checks multiple requests for benchmarking.
func (r *RateLimiter) checkRateBatch(payloadJSON string) string {
	var req struct {
		UserID string `json:"user_id"`
		Count  int    `json:"count"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.UserID == "" {
		req.UserID = "batch-user"
	}
	if req.Count <= 0 {
		req.Count = 1000
	}

	batchStart := host.NowMs()
	allowed := 0
	denied := 0

	numUsers := 50
	requestsPerUser := req.Count / numUsers
	if requestsPerUser < 1 {
		requestsPerUser = 1
	}

	for u := 0; u < numUsers; u++ {
		userID := fmt.Sprintf("%s-%d", req.UserID, u)
		for i := 0; i < requestsPerUser; i++ {
			now := host.NowMs()

			bucket, exists := r.Buckets[userID]
			if !exists {
				bucket = &TokenBucket{
					Tokens:     r.MaxTokens,
					LastRefill: now,
				}
				r.Buckets[userID] = bucket
			}

			// Refill
			elapsed := now - bucket.LastRefill
			if r.RefillRateMs > 0 {
				newTokens := int(elapsed / r.RefillRateMs)
				if newTokens > 0 {
					bucket.Tokens += newTokens
					if bucket.Tokens > r.MaxTokens {
						bucket.Tokens = r.MaxTokens
					}
					bucket.LastRefill = now
				}
			}

			ok := bucket.Tokens > 0
			if ok {
				bucket.Tokens--
				bucket.Allowed++
				r.TotalAllowed++
				allowed++
			} else {
				bucket.Denied++
				r.TotalDenied++
				denied++
			}
			r.TotalChecks++
		}
	}

	batchEnd := host.NowMs()
	durationMs := float64(batchEnd - batchStart)

	total := allowed + denied
	opsPerSec := 0.0
	if durationMs > 0 {
		opsPerSec = float64(total) / (durationMs / 1000.0)
	}

	return marshal(map[string]any{
		"status":         "ok",
		"total_requests": total,
		"allowed":        allowed,
		"denied":         denied,
		"duration_ms":    durationMs,
		"ops_per_sec":    opsPerSec,
		"unique_users":   numUsers,
		"reqs_per_user":  requestsPerUser,
	})
}

// getStats returns rate limiter statistics.
func (r *RateLimiter) getStats() string {
	denyRate := 0.0
	if r.TotalChecks > 0 {
		denyRate = float64(r.TotalDenied) / float64(r.TotalChecks) * 100
	}
	return marshal(map[string]any{
		"status": "ok",
		"config": map[string]any{
			"max_tokens":     r.MaxTokens,
			"refill_rate_ms": r.RefillRateMs,
		},
		"counters": map[string]any{
			"total_checks":  r.TotalChecks,
			"total_allowed": r.TotalAllowed,
			"total_denied":  r.TotalDenied,
			"deny_rate_pct": denyRate,
			"active_users":  len(r.Buckets),
		},
	})
}

// ========================================================================
// AlarmDemo Actor (Durable Object setAlarm equivalent)
// ========================================================================

// AlarmDemo demonstrates the durable alarm lifecycle.
// Equivalent to Cloudflare Durable Object this.state.storage.setAlarm():
//   - "start"    → host.Alarm().Set(nowMs + 30s)   — schedule alarm 30s from now
//   - "__alarm__" → process batched requests      — alarm fired callback
//   - "status"   → host.Alarm().Get()               — when does the alarm fire?
//   - "cancel"   → host.Alarm().Delete()            — cancel the pending alarm
//
// In production this pattern batches writes, flushes queues, or expires sessions.
type AlarmDemo struct {
	plexspaces.BaseActor

	// Pending requests collected before the alarm fires
	PendingRequests []string `json:"pending_requests"`

	// Lifecycle counters
	TotalAlarmsSet   int `json:"total_alarms_set"`
	TotalAlarmsFired int `json:"total_alarms_fired"`
	TotalProcessed   int `json:"total_processed"`
}

func NewAlarmDemo() plexspaces.Actor {
	a := &AlarmDemo{
		PendingRequests: make([]string, 0),
	}
	a.SetSelf(a)
	return a
}

func (a *AlarmDemo) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	host.Info(fmt.Sprintf("AlarmDemo %s: initialized", a.ActorID()))
	return ""
}

func (a *AlarmDemo) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "start":
		return a.handleStart(payloadJSON)
	case "__alarm__":
		return a.handleAlarm(payloadJSON)
	case "status":
		return a.handleStatus()
	case "cancel":
		return a.handleCancel()
	case "enqueue":
		return a.handleEnqueue(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleStart schedules a durable alarm 30 seconds from now.
// Equivalent to Cloudflare DO: this.state.storage.setAlarm(Date.now() + 30_000)
func (a *AlarmDemo) handleStart(payloadJSON string) string {
	var req struct {
		DelayMs uint64 `json:"delay_ms"`
	}
	// Default delay: 30 seconds
	req.DelayMs = 30000
	if payloadJSON != "" && payloadJSON != "{}" {
		json.Unmarshal([]byte(payloadJSON), &req)
	}
	if req.DelayMs == 0 {
		req.DelayMs = 30000
	}

	fireAt := host.NowMs() + req.DelayMs
	if err := host.Alarm().Set(fireAt); err != nil {
		return marshal(map[string]any{"error": "alarm_set failed: " + err.Error()})
	}
	a.TotalAlarmsSet++

	host.Info(fmt.Sprintf("AlarmDemo %s: alarm set, fires in %dms at ts=%d",
		a.ActorID(), req.DelayMs, fireAt))

	return marshal(map[string]any{
		"status":            "ok",
		"action":            "alarm_scheduled",
		"fire_at_ms":        fireAt,
		"delay_ms":          req.DelayMs,
		"total_alarms_set":  a.TotalAlarmsSet,
		"pending_requests":  len(a.PendingRequests),
	})
}

// handleAlarm is invoked by the framework when the scheduled alarm fires.
// Equivalent to Cloudflare DO: async alarm() { ... }
// Processes all batched pending requests and optionally reschedules.
func (a *AlarmDemo) handleAlarm(payloadJSON string) string {
	a.TotalAlarmsFired++
	processed := len(a.PendingRequests)
	a.TotalProcessed += processed

	host.Info(fmt.Sprintf("AlarmDemo %s: alarm fired, processing %d pending requests",
		a.ActorID(), processed))

	// Process the batch
	results := make([]string, 0, processed)
	for _, req := range a.PendingRequests {
		results = append(results, fmt.Sprintf("processed:%s", req))
	}
	a.PendingRequests = make([]string, 0)

	return marshal(map[string]any{
		"status":              "ok",
		"action":              "alarm_fired",
		"processed":           processed,
		"results":             results,
		"total_alarms_fired":  a.TotalAlarmsFired,
		"total_processed":     a.TotalProcessed,
	})
}

// handleStatus returns the current alarm schedule.
// Equivalent to Cloudflare DO: this.state.storage.getAlarm()
func (a *AlarmDemo) handleStatus() string {
	fireAt, err := host.Alarm().Get()
	errMsg := ""
	if err != nil {
		errMsg = err.Error()
	}

	return marshal(map[string]any{
		"status":             "ok",
		"alarm_fire_at_ms":   fireAt,
		"alarm_set":          fireAt > 0,
		"pending_requests":   len(a.PendingRequests),
		"total_alarms_set":   a.TotalAlarmsSet,
		"total_alarms_fired": a.TotalAlarmsFired,
		"total_processed":    a.TotalProcessed,
		"error":              errMsg,
	})
}

// handleCancel cancels the pending durable alarm.
// Equivalent to Cloudflare DO: this.state.storage.deleteAlarm()
func (a *AlarmDemo) handleCancel() string {
	if err := host.Alarm().Delete(); err != nil {
		return marshal(map[string]any{"error": "alarm_delete failed: " + err.Error()})
	}

	host.Info(fmt.Sprintf("AlarmDemo %s: alarm cancelled", a.ActorID()))

	return marshal(map[string]any{
		"status": "ok",
		"action": "alarm_cancelled",
	})
}

// handleEnqueue adds a request to the pending batch (processed when alarm fires).
func (a *AlarmDemo) handleEnqueue(payloadJSON string) string {
	var req struct {
		Data string `json:"data"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil || req.Data == "" {
		req.Data = fmt.Sprintf("item-%d", len(a.PendingRequests))
	}

	a.PendingRequests = append(a.PendingRequests, req.Data)

	return marshal(map[string]any{
		"status":           "ok",
		"action":           "enqueued",
		"data":             req.Data,
		"pending_requests": len(a.PendingRequests),
	})
}

// ========================================================================
// Helpers
// ========================================================================

func marshal(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `{"error":"marshal failed"}`
	}
	return string(data)
}

func toUint64(v any) uint64 {
	switch n := v.(type) {
	case float64:
		return uint64(n)
	case string:
		var result uint64
		fmt.Sscanf(n, "%d", &result)
		return result
	default:
		return 0
	}
}

func toInt(v any) int {
	switch n := v.(type) {
	case float64:
		return int(n)
	case string:
		var result int
		fmt.Sscanf(n, "%d", &result)
		return result
	default:
		return 0
	}
}

// ========================================================================
// Registration - Register actors for WASM export
// ========================================================================

// init() runs during _initialize (before main), ensuring the actors are
// registered before the host calls any exported functions like init/handle.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("ChatRoom", NewChatRoom)
	router.Route("RateLimiter", NewRateLimiter)
	router.Route("AlarmDemo", NewAlarmDemo)
	plexspaces.Register(router)
}

func main() {}
