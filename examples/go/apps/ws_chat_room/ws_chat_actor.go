// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WebSocket Chat Room — Go WASM actors.
//
// Port of the TypeScript ws_chat_room example to Go/TinyGo.
//
// Deployed to a PlexSpaces node and driven by browser thin-node clients via
// the WsFrame binary WebSocket protocol.
//
// Actors:
//   ChatRoomActor  — per-room member registry; fans out chat_message to all
//                    member actor_ids via host.Send(), routing each tell through
//                    WsActorTransportClient → WsRegistry → thin-node WS session.
//   PresenceActor  — per-user online/offline tracking with reminder timeout
//                    and KV presence persistence.
//
// Note on routing: host.Send(actorId, ...) is the correct fan-out primitive
// for thin-node clients. Their actor_ids (e.g. alice//ChatClient::ns@<thin-node>)
// are stored in ChatRoomActor state; the ActorRegistry routes each send to the
// appropriate WS session via WsActorTransportClient and WsRegistry.
//
// ## SDK Features Used
//
// - plexspaces.ActorRouter: Multi-actor routing (ChatRoomActor + PresenceActor)
// - plexspaces.BaseActor: Actor base with JSON state serialization
// - plexspaces.Host: Host function wrappers
// - host.Send(): Routing-transparent fan-out to member actors (incl. thin nodes)
// - host.KV().PutWithTTL(): Presence persistence with 2h TTL
// - host.KV().CAS(): Idempotent join check
// - host.Alarm().Set()/AlarmGet(): Periodic room stats flush
// - host.SendAfter(): Reminder-based idle timeout in PresenceActor

package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const maxHistory = 50

// presenceTTLSeconds is 2 hours — KV presence entries expire after this.
const presenceTTLSeconds = 2 * 60 * 60

// idleTimeoutMs is 60 seconds — PresenceActor schedules a check after this delay.
const idleTimeoutMs = 60_000

// idleThresholdMs is 55 seconds — if last_seen is older, mark user offline.
const idleThresholdMs = 55_000

// roomStatsAlarmIntervalMs is 5 minutes — ChatRoomActor schedules periodic stats flush.
const roomStatsAlarmIntervalMs = 5 * 60 * 1000

// ========================================================================
// ChatRoomActor
// ========================================================================

// HistoryEntry records a single chat message in room history.
type HistoryEntry struct {
	SenderActorID string `json:"sender_actor_id"`
	Sender        string `json:"sender"`
	Text          string `json:"text"`
	Ts            uint64 `json:"ts"`
}

// ChatRoomActor manages a single chat room: member registry, message history,
// and fan-out of events to all member actors via host.Send().
//
// Like a Cloudflare Durable Object instance per room — processes messages
// sequentially, no locks needed.
type ChatRoomActor struct {
	plexspaces.BaseActor

	// RoomID is derived from the actor's name on Init.
	RoomID string `json:"room_id"`

	// Members maps actorId → username for all connected members.
	Members map[string]string `json:"members"`

	// History is the bounded message ring buffer.
	History []HistoryEntry `json:"history"`

	// MsgSeq is the monotonic sequence counter for history entries.
	MsgSeq int64 `json:"msg_seq"`
}

var host = plexspaces.NewHost()

// NewChatRoomActor constructs a ChatRoomActor with empty state.
func NewChatRoomActor() plexspaces.Actor {
	a := &ChatRoomActor{
		Members: make(map[string]string),
		History: make([]HistoryEntry, 0),
	}
	a.SetSelf(a)
	return a
}

// Init is called once by the framework before any Handle call.
func (c *ChatRoomActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	// Extract room name from canonical actor ID: "name//Type::ns@node"
	roomID := config.ActorID
	if idx := strings.Index(roomID, "//"); idx >= 0 {
		roomID = roomID[:idx]
	}
	c.RoomID = roomID
	c.SetRuntimeMetadata(config.ActorID)

	host.Info(fmt.Sprintf("ChatRoomActor %s: initialized", c.RoomID))
	return ""
}

// Handle dispatches incoming messages.
func (c *ChatRoomActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "join":
		return c.handleJoin(payloadJSON)
	case "leave":
		return c.handleLeave(payloadJSON)
	case "send":
		return c.handleSend(payloadJSON)
	case "members":
		return c.handleMembers()
	case "status":
		return c.handleStatus()
	case "__alarm__":
		return c.handleAlarm(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleJoin adds a member to the room and fans out a member_joined event.
//
// Uses host.KVCAS for an idempotent join check: the first join with a given
// (actorId, username) pair writes the entry; subsequent identical calls are
// no-ops.  If the same username reconnects with a new actor_id (browser
// refresh), the stale entry is cleaned up first.
func (c *ChatRoomActor) handleJoin(payloadJSON string) string {
	var req struct {
		ActorID  string `json:"actor_id"`
		Username string `json:"username"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.ActorID == "" {
		return marshal(map[string]any{"error": "actor_id required"})
	}
	username := req.Username
	if username == "" {
		username = req.ActorID
	}

	// Remove stale entry for same username (reconnect with new actor_id).
	for existingActorID, existingUsername := range c.Members {
		if existingUsername == username && existingActorID != req.ActorID {
			delete(c.Members, existingActorID)
		}
	}

	// Idempotent join: use KVCAS to record this (actorId → username) binding.
	// CAS key: "room:<roomId>:member:<actorId>"
	// If the key already holds the same username the CAS will fail (current ≠ ""),
	// which is fine — we already have the member in state.
	casKey := fmt.Sprintf("room:%s:member:%s", c.RoomID, req.ActorID)
	host.KV().CAS(casKey, "", username)

	existingActorIDs := make([]string, 0, len(c.Members))
	for id := range c.Members {
		existingActorIDs = append(existingActorIDs, id)
	}
	c.Members[req.ActorID] = username

	// Build member_info snapshot for the joiner.
	memberInfo := make(map[string]string, len(c.Members))
	for id, uname := range c.Members {
		memberInfo[id] = uname
	}

	allActorIDs := make([]string, 0, len(c.Members))
	for id := range c.Members {
		allActorIDs = append(allActorIDs, id)
	}

	// Fan-out member_joined to previously existing members (not to the joiner).
	joinedEvent := map[string]any{
		"room_id":          c.RoomID,
		"members":          allActorIDs,
		"member_info":      memberInfo,
		"joined_actor_id":  req.ActorID,
		"joined_username":  username,
	}
	for _, actorID := range existingActorIDs {
		if actorID != req.ActorID {
			host.Send(actorID, "member_joined", joinedEvent)
		}
	}

	return marshal(map[string]any{
		"success":     true,
		"members":     allActorIDs,
		"member_info": memberInfo,
		"room_id":     c.RoomID,
		"history":     c.History,
	})
}

// handleLeave removes a member and fans out a member_left event.
func (c *ChatRoomActor) handleLeave(payloadJSON string) string {
	var req struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil || req.ActorID == "" {
		return marshal(map[string]any{"success": true})
	}

	delete(c.Members, req.ActorID)

	// Clean up CAS key.
	casKey := fmt.Sprintf("room:%s:member:%s", c.RoomID, req.ActorID)
	host.KV().Delete(casKey)

	allActorIDs := make([]string, 0, len(c.Members))
	for id := range c.Members {
		allActorIDs = append(allActorIDs, id)
	}

	memberInfo := make(map[string]string, len(c.Members))
	for id, uname := range c.Members {
		memberInfo[id] = uname
	}

	leftEvent := map[string]any{
		"room_id":     c.RoomID,
		"members":     allActorIDs,
		"member_info": memberInfo,
	}
	for _, actorID := range allActorIDs {
		host.Send(actorID, "member_left", leftEvent)
	}

	return marshal(map[string]any{"success": true})
}

// handleSend appends a message to history and fans out chat_message to all members.
// Fan-out uses host.Send() — routing-transparent: the ActorRegistry delivers to
// local actors directly, and to thin nodes via WsActorTransportClient → WsRegistry.
func (c *ChatRoomActor) handleSend(payloadJSON string) string {
	var req struct {
		SenderActorID string `json:"sender_actor_id"`
		Text          string `json:"text"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	if req.SenderActorID == "" || req.Text == "" {
		return marshal(map[string]any{"error": "sender_actor_id and text required"})
	}

	senderUsername, ok := c.Members[req.SenderActorID]
	if !ok {
		senderUsername = req.SenderActorID
	}
	ts := host.NowMs()

	c.MsgSeq++
	entry := HistoryEntry{
		SenderActorID: req.SenderActorID,
		Sender:        senderUsername,
		Text:          req.Text,
		Ts:            ts,
	}
	c.History = append(c.History, entry)
	if len(c.History) > maxHistory {
		c.History = c.History[len(c.History)-maxHistory:]
	}

	event := map[string]any{
		"sender":          req.SenderActorID,
		"sender_username": senderUsername,
		"text":            req.Text,
		"room_id":         c.RoomID,
		"ts":              ts,
	}
	// Fan out to all members INCLUDING sender (sender sees confirmation).
	memberIDs := make([]string, 0, len(c.Members))
	for id := range c.Members {
		memberIDs = append(memberIDs, id)
	}
	for _, actorID := range memberIDs {
		host.Send(actorID, "chat_message", event)
	}

	return marshal(map[string]any{
		"success":          true,
		"members_notified": len(memberIDs),
	})
}

// handleMembers returns the current member list.
func (c *ChatRoomActor) handleMembers() string {
	memberIDs := make([]string, 0, len(c.Members))
	for id := range c.Members {
		memberIDs = append(memberIDs, id)
	}
	// Copy members map for the response.
	usernames := make(map[string]string, len(c.Members))
	for id, uname := range c.Members {
		usernames[id] = uname
	}
	return marshal(map[string]any{
		"members":  memberIDs,
		"usernames": usernames,
		"room_id":  c.RoomID,
	})
}

// handleStatus returns room statistics and the current alarm schedule.
func (c *ChatRoomActor) handleStatus() string {
	alarmAt, _ := host.Alarm().Get()
	return marshal(map[string]any{
		"room_id":      c.RoomID,
		"member_count": len(c.Members),
		"history_size": len(c.History),
		"msg_seq":      c.MsgSeq,
		"alarm_at_ms":  alarmAt,
	})
}

// handleAlarm is called by the framework when the durable alarm fires.
// Logs periodic room statistics and reschedules the next flush alarm.
func (c *ChatRoomActor) handleAlarm(_ string) string {
	host.Info(fmt.Sprintf("ChatRoomActor %s: periodic stats flush — members=%d history=%d msg_seq=%d",
		c.RoomID, len(c.Members), len(c.History), c.MsgSeq))

	// Reschedule next stats alarm.
	if err := host.Alarm().SetIn(roomStatsAlarmIntervalMs); err != nil {
		host.Warn(fmt.Sprintf("ChatRoomActor %s: reschedule alarm failed: %v", c.RoomID, err))
	}

	return marshal(map[string]any{
		"status":       "ok",
		"action":       "stats_flushed",
		"member_count": len(c.Members),
		"history_size": len(c.History),
		"msg_seq":      c.MsgSeq,
	})
}

// ========================================================================
// PresenceActor
// ========================================================================

// PresenceActor tracks online/offline state for a single user.
// Persists presence in KV with a 2h TTL so other services can query it.
// Schedules a sendAfter idle timeout check via the reminder facet.
type PresenceActor struct {
	plexspaces.BaseActor

	// UserID is derived from the actor's name on Init.
	UserID string `json:"user_id"`

	// Online reports whether the user is currently considered online.
	Online bool `json:"online"`

	// LastSeen is the timestamp (ms) of the most recent online/activity event.
	LastSeen uint64 `json:"last_seen"`
}

// NewPresenceActor constructs a PresenceActor with empty state.
func NewPresenceActor() plexspaces.Actor {
	a := &PresenceActor{}
	a.SetSelf(a)
	return a
}

// Init extracts the user ID from the actor canonical ID.
func (p *PresenceActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	userID := config.ActorID
	if idx := strings.Index(userID, "//"); idx >= 0 {
		userID = userID[:idx]
	}
	p.UserID = userID
	p.SetRuntimeMetadata(config.ActorID)

	host.Info(fmt.Sprintf("PresenceActor %s: initialized", p.UserID))
	return ""
}

// Handle dispatches incoming messages.
func (p *PresenceActor) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "online":
		return p.handleOnline()
	case "offline":
		return p.handleOffline()
	case "timeout_check":
		return p.handleTimeoutCheck()
	case "status":
		return p.handleStatus()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleOnline marks the user online, persists to KV with TTL, and
// schedules a SendAfter idle timeout check via the reminder facet.
func (p *PresenceActor) handleOnline() string {
	p.Online = true
	p.LastSeen = host.NowMs()

	kvKey := fmt.Sprintf("presence:%s", p.UserID)
	presenceData, _ := json.Marshal(map[string]any{
		"online":    true,
		"last_seen": p.LastSeen,
	})
	// KV entry expires after 2 hours — removes stale presence automatically.
	host.KV().PutWithTTL(kvKey, string(presenceData), presenceTTLSeconds)

	// Schedule idle timeout check via the reminder facet.
	_, _ = host.Actor().SendAfter(idleTimeoutMs, "timeout_check", map[string]any{})

	return marshal(map[string]any{
		"success": true,
		"online":  true,
		"user_id": p.UserID,
	})
}

// handleOffline marks the user offline and updates the KV entry.
func (p *PresenceActor) handleOffline() string {
	p.Online = false
	p.LastSeen = host.NowMs()

	kvKey := fmt.Sprintf("presence:%s", p.UserID)
	presenceData, _ := json.Marshal(map[string]any{
		"online":    false,
		"last_seen": p.LastSeen,
	})
	host.KV().PutWithTTL(kvKey, string(presenceData), presenceTTLSeconds)

	// Cancel any pending alarm so idle checks don't fire after explicit logout.
	host.Alarm().Delete()

	return marshal(map[string]any{
		"success": true,
		"online":  false,
		"user_id": p.UserID,
	})
}

// handleTimeoutCheck is invoked by host.SendAfter after the idle window.
// Marks the user offline if they have been idle for more than idleThresholdMs.
func (p *PresenceActor) handleTimeoutCheck() string {
	idleSince := host.NowMs() - p.LastSeen
	if idleSince > idleThresholdMs {
		p.Online = false
		kvKey := fmt.Sprintf("presence:%s", p.UserID)
		presenceData, _ := json.Marshal(map[string]any{
			"online":    false,
			"last_seen": p.LastSeen,
		})
		host.KV().PutWithTTL(kvKey, string(presenceData), presenceTTLSeconds)

		host.Info(fmt.Sprintf("PresenceActor %s: marked offline after %dms idle",
			p.UserID, idleSince))
	}

	return marshal(map[string]any{
		"checked":  true,
		"idle_ms":  idleSince,
		"online":   p.Online,
		"user_id":  p.UserID,
	})
}

// handleStatus returns the current presence state.
func (p *PresenceActor) handleStatus() string {
	return marshal(map[string]any{
		"user_id":   p.UserID,
		"online":    p.Online,
		"last_seen": p.LastSeen,
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

// ========================================================================
// Registration — register actors for WASM export
// ========================================================================

// init() runs during _initialize (before main), ensuring actors are registered
// before the host calls any exported functions (init/handle/get_state/set_state).
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("ChatRoomActor", NewChatRoomActor)
	router.Route("PresenceActor", NewPresenceActor)
	plexspaces.Register(router)
}

func main() {}
