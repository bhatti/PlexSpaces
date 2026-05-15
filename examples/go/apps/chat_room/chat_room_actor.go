// SPDX-License-Identifier: AGPL-3.0-or-later
// Chat Room (Go WASM)
//
// A large-scale chat example that demonstrates durable virtual actors, process
// groups, FSM actors, workflow actors, timers, and shared services.  This is a
// faithful Go port of the Python chat_room example.

package main

import (
	"encoding/json"
	"strings"

	"github.com/bhatti/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ---------------------------------------------------------------------------
// Helpers – peer ID building (mirrors helpers.py)
// ---------------------------------------------------------------------------

func peer(actorType, name string) string {
	selfID := host.SelfID()
	if selfID == "" {
		return actorType + ":" + name
	}
	parsed, err := plexspaces.ParseActorID(selfID)
	if err != nil {
		return actorType + ":" + name
	}
	return parsed.WithTypeAndName(actorType, name).String()
}

func guildActorID(guildID string) string {
	return peer("GuildActor", guildID)
}

func channelActorID(guildID, channelID string) string {
	return peer("ChannelActor", guildID+"__"+channelID)
}

func messageStoreActorID(guildID, channelID string) string {
	return peer("MessageStoreActor", guildID+"__"+channelID)
}

func presenceActorID(userID string) string {
	return peer("PresenceActor", userID)
}

func connectionFSMActorID(sessionID string) string {
	return peer("ConnectionFSM", sessionID)
}

func fanoutActorID() string {
	return peer("FanoutActor", "singleton")
}

func auditEventActorID() string {
	return peer("AuditEventActor", "singleton")
}

func actorApplicationID(actorID string) string {
	parsed, err := plexspaces.ParseActorID(actorID)
	if err != nil {
		return ""
	}
	return parsed.Namespace
}

func actorInstanceName(actorID string) string {
	parsed, err := plexspaces.ParseActorID(actorID)
	if err != nil {
		return actorID
	}
	if parsed.Name != "" {
		return parsed.Name
	}
	return actorID
}

func decodeGuildID(actorID string) string {
	return actorInstanceName(actorID)
}

func decodeChannelParts(actorID string) (string, string) {
	name := actorInstanceName(actorID)
	if idx := strings.Index(name, "__"); idx >= 0 {
		return name[:idx], name[idx+2:]
	}
	return name, ""
}

func channelGroup(guildID, channelID string) string {
	return "channel:" + guildID + "__" + channelID
}

func userSessionGroup(userID string) string {
	return "user-session:" + userID
}

// safeMetricsAdd emits metrics best-effort (ignores errors).
func safeMetricsAdd(appID string, counters map[string]int) {
	if appID == "" {
		return
	}
	cm := make(map[string]any, len(counters))
	for k, v := range counters {
		cm[k] = v
	}
	_, _ = host.ApplicationMetricsAdd(appID, map[string]any{
		"message_count":   1,
		"counter_metrics": cm,
	})
}

// ---------------------------------------------------------------------------
// Generic JSON helpers
// ---------------------------------------------------------------------------

type appConfig struct {
	ActorID string         `json:"actor_id"`
	Args    map[string]any `json:"args"`
}

func parsePayload(payloadJSON string) map[string]any {
	if payloadJSON == "" {
		return map[string]any{}
	}
	var m map[string]any
	if err := json.Unmarshal([]byte(payloadJSON), &m); err != nil {
		return map[string]any{}
	}
	return m
}

func payloadString(payload map[string]any, key string) string {
	if v, ok := payload[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return ""
}

func payloadInt(payload map[string]any, key string, fallback int) int {
	if v, ok := payload[key]; ok {
		switch t := v.(type) {
		case float64:
			return int(t)
		case int:
			return t
		}
	}
	return fallback
}

func payloadInt64(payload map[string]any, key string, fallback int64) int64 {
	if v, ok := payload[key]; ok {
		switch t := v.(type) {
		case float64:
			return int64(t)
		case int64:
			return t
		case int:
			return int64(t)
		}
	}
	return fallback
}

func configString(args map[string]any, key string) string {
	if args == nil {
		return ""
	}
	if v, ok := args[key]; ok {
		if s, ok := v.(string); ok {
			return s
		}
	}
	return ""
}

func marshal(value map[string]any) string {
	data, _ := json.Marshal(value)
	return string(data)
}

func marshalAny(value any) string {
	data, _ := json.Marshal(value)
	return string(data)
}

// sliceUnique merges existing + add and returns sorted unique strings.
func sliceUnique(existing []string, add ...string) []string {
	seen := make(map[string]struct{}, len(existing)+len(add))
	for _, v := range existing {
		seen[v] = struct{}{}
	}
	for _, v := range add {
		seen[v] = struct{}{}
	}
	out := make([]string, 0, len(seen))
	for v := range seen {
		out = append(out, v)
	}
	for i := 0; i < len(out); i++ {
		for j := i + 1; j < len(out); j++ {
			if out[i] > out[j] {
				out[i], out[j] = out[j], out[i]
			}
		}
	}
	return out
}

// lastNMaps returns the last n elements of a slice of maps.
func lastNMaps(s []map[string]any, n int) []map[string]any {
	if n <= 0 || len(s) == 0 {
		return s
	}
	if len(s) <= n {
		return s
	}
	return s[len(s)-n:]
}

// intToStr converts an int to its decimal string representation.
func intToStr(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	buf := make([]byte, 0, 12)
	for n > 0 {
		buf = append(buf, byte('0'+n%10))
		n /= 10
	}
	if neg {
		buf = append(buf, '-')
	}
	// reverse
	for i, j := 0, len(buf)-1; i < j; i, j = i+1, j-1 {
		buf[i], buf[j] = buf[j], buf[i]
	}
	return string(buf)
}

// payloadStringSlice extracts a []string from a payload key.
func payloadStringSlice(payload map[string]any, key string) []string {
	raw, ok := payload[key]
	if !ok {
		return nil
	}
	arr, ok := raw.([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(arr))
	for _, v := range arr {
		if s, ok := v.(string); ok {
			out = append(out, s)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// SessionActor
// ---------------------------------------------------------------------------

type sessionActor struct {
	plexspaces.BaseActor

	ApplicationID   string           `json:"application_id"`
	SessionID       string           `json:"session_id"`
	UserID          string           `json:"user_id"`
	GuildID         string           `json:"guild_id"`
	JoinedChannels  []string         `json:"joined_channels"`
	DeliveredEvents []map[string]any `json:"delivered_events"`
	UnreadByChannel map[string]int   `json:"unread_by_channel"`
	LastDeliveryMs  int64            `json:"last_delivery_ms"`
}

func newSessionActor() *sessionActor {
	a := &sessionActor{
		JoinedChannels:  []string{},
		DeliveredEvents: []map[string]any{},
		UnreadByChannel: map[string]int{},
	}
	a.SetSelf(a)
	return a
}

func (a *sessionActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.SessionID = actorInstanceName(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.JoinedChannels == nil {
		a.JoinedChannels = []string{}
	}
	if a.DeliveredEvents == nil {
		a.DeliveredEvents = []map[string]any{}
	}
	if a.UnreadByChannel == nil {
		a.UnreadByChannel = map[string]int{}
	}
	if uid := configString(cfg.Args, "user_id"); uid != "" {
		a.UserID = uid
	}
	if gid := configString(cfg.Args, "guild_id"); gid != "" {
		a.GuildID = gid
	}
	return ""
}

func (a *sessionActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "connect":
		return a.handleConnect(payload)
	case "send_channel_message":
		return a.handleSendChannelMessage(payload)
	case "set_typing":
		return a.handleSetTyping(payload)
	case "deliver_channel_event":
		return a.handleDeliverChannelEvent(payload)
	case "read_channel":
		return a.handleReadChannel(payload)
	case "inbox":
		return a.handleInbox()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *sessionActor) handleConnect(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	guildID := payloadString(payload, "guild_id")
	if userID == "" || guildID == "" {
		return marshal(map[string]any{"error": "user_id and guild_id are required"})
	}
	a.UserID = userID
	a.GuildID = guildID
	ttlMs := payloadInt(payload, "ttl_ms", 60000)

	channels := payloadStringSlice(payload, "channels")
	if channels == nil {
		channels = []string{}
	}
	a.JoinedChannels = channels

	// Join user session process group.
	_ = host.PG().Join(userSessionGroup(userID))

	// Join each channel's process group and send join_member.
	for _, chID := range channels {
		_ = host.PG().Join(channelGroup(guildID, chID))
		_ = host.Send(channelActorID(guildID, chID), "join_member", map[string]any{
			"user_id":    userID,
			"session_id": a.SessionID,
		})
	}

	// Register with guild.
	_ = host.Send(guildActorID(guildID), "register_session", map[string]any{
		"user_id":    userID,
		"session_id": a.SessionID,
		"channels":   channels,
	})

	// Set presence.
	_ = host.Send(presenceActorID(userID), "set_presence", map[string]any{
		"user_id":  userID,
		"guild_id": guildID,
		"status":   "online",
		"ttl_ms":   ttlMs,
	})

	// Advance FSM: offline→connected→joined.
	_ = host.Send(connectionFSMActorID(a.SessionID), "transition", map[string]any{"to": "connected"})
	_ = host.Send(connectionFSMActorID(a.SessionID), "transition", map[string]any{"to": "joined"})

	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_sessions_connected": 1})
	return marshal(map[string]any{
		"status":     "connected",
		"session_id": a.SessionID,
		"user_id":    a.UserID,
		"guild_id":   a.GuildID,
		"channels":   a.JoinedChannels,
	})
}

func (a *sessionActor) handleSendChannelMessage(payload map[string]any) string {
	if a.UserID == "" || a.GuildID == "" {
		return marshal(map[string]any{"error": "session_not_connected"})
	}
	channelID := payloadString(payload, "channel_id")
	text := payloadString(payload, "text")
	if channelID == "" {
		return marshal(map[string]any{"error": "channel_id is required"})
	}
	result, err := host.Ask(channelActorID(a.GuildID, channelID), "post_message", map[string]any{
		"user_id":    a.UserID,
		"session_id": a.SessionID,
		"text":       text,
	}, 5000)
	if err != nil {
		return marshal(map[string]any{"error": err.Error()})
	}
	return marshalAny(result)
}

func (a *sessionActor) handleSetTyping(payload map[string]any) string {
	if a.UserID == "" || a.GuildID == "" {
		return marshal(map[string]any{"error": "session_not_connected"})
	}
	channelID := payloadString(payload, "channel_id")
	ttlMs := payloadInt(payload, "ttl_ms", 2000)
	if channelID == "" {
		return marshal(map[string]any{"error": "channel_id is required"})
	}
	result, err := host.Ask(channelActorID(a.GuildID, channelID), "mark_typing", map[string]any{
		"user_id": a.UserID,
		"ttl_ms":  ttlMs,
	}, 5000)
	if err != nil {
		return marshal(map[string]any{"error": err.Error()})
	}
	return marshalAny(result)
}

func (a *sessionActor) handleDeliverChannelEvent(payload map[string]any) string {
	evtType := payloadString(payload, "event_type")
	if evtType == "" {
		evtType = "message"
	}
	event := map[string]any{
		"event_type":      evtType,
		"guild_id":        payloadString(payload, "guild_id"),
		"channel_id":      payloadString(payload, "channel_id"),
		"message_id":      payloadString(payload, "message_id"),
		"from_user":       payloadString(payload, "from_user"),
		"text":            payloadString(payload, "text"),
		"delivered_at_ms": payloadInt64(payload, "delivered_at_ms", 0),
	}
	a.DeliveredEvents = append(a.DeliveredEvents, event)
	if len(a.DeliveredEvents) > 50 {
		a.DeliveredEvents = a.DeliveredEvents[len(a.DeliveredEvents)-50:]
	}
	a.LastDeliveryMs = payloadInt64(payload, "delivered_at_ms", 0)

	fromUser := payloadString(payload, "from_user")
	channelID := payloadString(payload, "channel_id")
	if fromUser != a.UserID && evtType == "message" {
		a.UnreadByChannel[channelID]++
	}
	return marshal(map[string]any{"status": "delivered", "session_id": a.SessionID})
}

func (a *sessionActor) handleReadChannel(payload map[string]any) string {
	channelID := payloadString(payload, "channel_id")
	if channelID == "" {
		return marshal(map[string]any{"error": "channel_id is required"})
	}
	a.UnreadByChannel[channelID] = 0

	_, _ = host.Ask(connectionFSMActorID(a.SessionID), "transition", map[string]any{"to": "idle"}, 5000)

	remaining := make(map[string]any, len(a.UnreadByChannel))
	for k, v := range a.UnreadByChannel {
		remaining[k] = v
	}
	return marshal(map[string]any{
		"status":           "read",
		"channel_id":       channelID,
		"session_id":       a.SessionID,
		"remaining_unread": remaining,
	})
}

func (a *sessionActor) handleInbox() string {
	unread := make(map[string]any, len(a.UnreadByChannel))
	for k, v := range a.UnreadByChannel {
		unread[k] = v
	}
	events := make([]any, len(a.DeliveredEvents))
	for i, e := range a.DeliveredEvents {
		events[i] = e
	}
	return marshal(map[string]any{
		"session_id":        a.SessionID,
		"user_id":           a.UserID,
		"guild_id":          a.GuildID,
		"joined_channels":   a.JoinedChannels,
		"delivered_events":  events,
		"unread_by_channel": unread,
		"last_delivery_ms":  a.LastDeliveryMs,
	})
}

// ---------------------------------------------------------------------------
// PresenceActor
// ---------------------------------------------------------------------------

type presenceActor struct {
	plexspaces.BaseActor

	ApplicationID    string `json:"application_id"`
	UserID           string `json:"user_id"`
	GuildID          string `json:"guild_id"`
	Status           string `json:"status"`
	LastSeenMs       int64  `json:"last_seen_ms"`
	ExpiryDeadlineMs int64  `json:"expiry_deadline_ms"`
}

func newPresenceActor() *presenceActor {
	a := &presenceActor{Status: "offline"}
	a.SetSelf(a)
	return a
}

func (a *presenceActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.UserID = actorInstanceName(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.Status == "" {
		a.Status = "offline"
	}
	if gid := configString(cfg.Args, "guild_id"); gid != "" {
		a.GuildID = gid
	}
	return ""
}

func (a *presenceActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "set_presence":
		return a.handleSetPresence(payload)
	case "expire_presence":
		return a.handleExpirePresence(payload)
	case "status":
		return a.handleStatus()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *presenceActor) handleSetPresence(payload map[string]any) string {
	if uid := payloadString(payload, "user_id"); uid != "" {
		a.UserID = uid
	}
	if gid := payloadString(payload, "guild_id"); gid != "" {
		a.GuildID = gid
	}
	status := payloadString(payload, "status")
	if status == "" {
		status = "online"
	}
	ttlMs := payloadInt(payload, "ttl_ms", 60000)

	nowMs := int64(host.NowMs())
	a.Status = status
	a.LastSeenMs = nowMs
	a.ExpiryDeadlineMs = nowMs + int64(ttlMs)

	_ = host.SendAfter(uint64(ttlMs), "expire_presence", map[string]any{
		"deadline_ms": a.ExpiryDeadlineMs,
	})

	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_presence_updates": 1})
	return marshal(map[string]any{
		"user_id":       a.UserID,
		"guild_id":      a.GuildID,
		"status":        a.Status,
		"expires_at_ms": a.ExpiryDeadlineMs,
	})
}

func (a *presenceActor) handleExpirePresence(payload map[string]any) string {
	deadlineMs := payloadInt64(payload, "deadline_ms", 0)
	if deadlineMs != a.ExpiryDeadlineMs {
		return marshal(map[string]any{"status": "ignored", "reason": "stale_deadline"})
	}
	a.Status = "offline"
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_presence_expirations": 1})
	return marshal(map[string]any{"status": "expired", "user_id": a.UserID})
}

func (a *presenceActor) handleStatus() string {
	return marshal(map[string]any{
		"user_id":       a.UserID,
		"guild_id":      a.GuildID,
		"status":        a.Status,
		"last_seen_ms":  a.LastSeenMs,
		"expires_at_ms": a.ExpiryDeadlineMs,
	})
}

// ---------------------------------------------------------------------------
// ConnectionFSM
// ---------------------------------------------------------------------------

var validConnectionTransitions = map[string]map[string]bool{
	"offline":      {"connected": true},
	"connected":    {"joined": true},
	"joined":       {"idle": true, "disconnected": true},
	"idle":         {"joined": true, "disconnected": true},
	"disconnected": {"connected": true},
}

type connectionFSM struct {
	plexspaces.BaseActor

	SessionID       string `json:"session_id"`
	ApplicationID   string `json:"application_id"`
	FSMState        string `json:"fsm_state"`
	TransitionCount int    `json:"transition_count"`
}

func newConnectionFSM() *connectionFSM {
	a := &connectionFSM{FSMState: "offline"}
	a.SetSelf(a)
	return a
}

func (a *connectionFSM) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.SessionID = actorInstanceName(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.FSMState == "" {
		a.FSMState = "offline"
	}
	return ""
}

func (a *connectionFSM) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "transition":
		return a.handleTransition(payload)
	case "status":
		return a.handleStatus()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *connectionFSM) handleTransition(payload map[string]any) string {
	to := payloadString(payload, "to")
	if to == "" {
		return marshal(map[string]any{"error": "to is required"})
	}
	allowed := validConnectionTransitions[a.FSMState]
	if !allowed[to] {
		allowedList := make([]string, 0, len(allowed))
		for k := range allowed {
			allowedList = append(allowedList, k)
		}
		return marshal(map[string]any{
			"status":  "ignored",
			"from":    a.FSMState,
			"to":      to,
			"allowed": allowedList,
		})
	}
	previous := a.FSMState
	a.FSMState = to
	a.TransitionCount++
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_connection_transitions": 1})
	return marshal(map[string]any{
		"status":           "ok",
		"session_id":       a.SessionID,
		"from":             previous,
		"to":               a.FSMState,
		"transition_count": a.TransitionCount,
	})
}

func (a *connectionFSM) handleStatus() string {
	return marshal(map[string]any{
		"session_id":       a.SessionID,
		"state":            a.FSMState,
		"transition_count": a.TransitionCount,
	})
}

// ---------------------------------------------------------------------------
// GuildActor
// ---------------------------------------------------------------------------

type guildActor struct {
	plexspaces.BaseActor

	ApplicationID string         `json:"application_id"`
	GuildID       string         `json:"guild_id"`
	Members       []string       `json:"members"`
	Channels      []string       `json:"channels"`
	SessionIndex  map[string]any `json:"session_index"`
}

func newGuildActor() *guildActor {
	a := &guildActor{
		Members:      []string{},
		Channels:     []string{},
		SessionIndex: map[string]any{},
	}
	a.SetSelf(a)
	return a
}

func (a *guildActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.GuildID = decodeGuildID(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.Members == nil {
		a.Members = []string{}
	}
	if a.Channels == nil {
		a.Channels = []string{}
	}
	if a.SessionIndex == nil {
		a.SessionIndex = map[string]any{}
	}
	return ""
}

func (a *guildActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "register_session":
		return a.handleRegisterSession(payload)
	case "create_channel":
		return a.handleCreateChannel(payload)
	case "topology":
		return a.handleTopology()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *guildActor) handleRegisterSession(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	sessionID := payloadString(payload, "session_id")
	channels := payloadStringSlice(payload, "channels")
	if channels == nil {
		channels = []string{}
	}
	a.Members = sliceUnique(a.Members, userID)
	a.Channels = sliceUnique(a.Channels, channels...)
	a.SessionIndex[sessionID] = map[string]any{
		"user_id":  userID,
		"channels": channels,
	}
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_guild_registrations": 1})
	return marshal(map[string]any{
		"guild_id":      a.GuildID,
		"member_count":  len(a.Members),
		"session_count": len(a.SessionIndex),
		"channels":      a.Channels,
	})
}

func (a *guildActor) handleCreateChannel(payload map[string]any) string {
	channelID := payloadString(payload, "channel_id")
	if channelID == "" {
		return marshal(map[string]any{"error": "channel_id is required"})
	}
	a.Channels = sliceUnique(a.Channels, channelID)
	kvVal, _ := json.Marshal(a.Channels)
	_ = host.KVPut("guild:"+a.GuildID+":channels", string(kvVal))
	return marshal(map[string]any{
		"guild_id":   a.GuildID,
		"channel_id": channelID,
		"channels":   a.Channels,
	})
}

func (a *guildActor) handleTopology() string {
	return marshal(map[string]any{
		"guild_id":      a.GuildID,
		"members":       a.Members,
		"channels":      a.Channels,
		"session_index": a.SessionIndex,
	})
}

// ---------------------------------------------------------------------------
// ChannelActor
// ---------------------------------------------------------------------------

type channelActor struct {
	plexspaces.BaseActor

	ApplicationID   string           `json:"application_id"`
	GuildID         string           `json:"guild_id"`
	ChannelID       string           `json:"channel_id"`
	MemberIndex     map[string]any   `json:"member_index"`
	TypingDeadlines map[string]int64 `json:"typing_deadlines"`
	Messages        []map[string]any `json:"messages"`
	LastMessageID   string           `json:"last_message_id"`
	TotalMessages   int              `json:"total_messages"`
}

func newChannelActor() *channelActor {
	a := &channelActor{
		MemberIndex:     map[string]any{},
		TypingDeadlines: map[string]int64{},
		Messages:        []map[string]any{},
	}
	a.SetSelf(a)
	return a
}

func (a *channelActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.GuildID, a.ChannelID = decodeChannelParts(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.MemberIndex == nil {
		a.MemberIndex = map[string]any{}
	}
	if a.TypingDeadlines == nil {
		a.TypingDeadlines = map[string]int64{}
	}
	if a.Messages == nil {
		a.Messages = []map[string]any{}
	}
	return ""
}

func (a *channelActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "join_member":
		return a.handleJoinMember(payload)
	case "mark_typing":
		return a.handleMarkTyping(payload)
	case "clear_typing":
		return a.handleClearTyping(payload)
	case "post_message":
		return a.handlePostMessage(payload)
	case "history":
		return a.handleHistory(payload)
	case "status":
		return a.handleStatus()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *channelActor) handleJoinMember(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	sessionID := payloadString(payload, "session_id")
	if userID == "" {
		return marshal(map[string]any{"error": "user_id is required"})
	}
	a.MemberIndex[userID] = map[string]any{"session_id": sessionID}
	members := make([]string, 0, len(a.MemberIndex))
	for k := range a.MemberIndex {
		members = append(members, k)
	}
	membersJSON, _ := json.Marshal(members)
	_ = host.KVPut("channel:"+a.GuildID+":"+a.ChannelID+":members", string(membersJSON))
	return marshal(map[string]any{
		"guild_id":     a.GuildID,
		"channel_id":   a.ChannelID,
		"member_count": len(a.MemberIndex),
	})
}

func (a *channelActor) handleMarkTyping(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	ttlMs := payloadInt(payload, "ttl_ms", 2000)
	if userID == "" {
		return marshal(map[string]any{"error": "user_id is required"})
	}
	deadlineMs := int64(host.NowMs()) + int64(ttlMs)
	a.TypingDeadlines[userID] = deadlineMs
	_ = host.SendAfter(uint64(ttlMs), "clear_typing", map[string]any{
		"user_id":     userID,
		"deadline_ms": deadlineMs,
	})
	typingUsers := make([]string, 0, len(a.TypingDeadlines))
	for k := range a.TypingDeadlines {
		typingUsers = append(typingUsers, k)
	}
	return marshal(map[string]any{
		"status":       "typing",
		"guild_id":     a.GuildID,
		"channel_id":   a.ChannelID,
		"user_id":      userID,
		"typing_users": typingUsers,
	})
}

func (a *channelActor) handleClearTyping(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	deadlineMs := payloadInt64(payload, "deadline_ms", 0)
	current, ok := a.TypingDeadlines[userID]
	if !ok || current != deadlineMs {
		return marshal(map[string]any{"status": "ignored", "reason": "stale_deadline"})
	}
	delete(a.TypingDeadlines, userID)
	return marshal(map[string]any{"status": "cleared", "user_id": userID})
}

func (a *channelActor) handlePostMessage(payload map[string]any) string {
	userID := payloadString(payload, "user_id")
	text := payloadString(payload, "text")
	sessionID := payloadString(payload, "session_id")

	if userID == "" {
		return marshal(map[string]any{"error": "user_id is required"})
	}
	if _, ok := a.MemberIndex[userID]; !ok {
		return marshal(map[string]any{"error": "user_not_in_channel", "user_id": userID})
	}

	nextSeq := a.TotalMessages + 1
	messageID := a.ChannelID + "-" + intToStr(nextSeq)
	storedAtMs := int64(host.NowMs())

	// Store in MessageStoreActor.
	_ = host.Send(messageStoreActorID(a.GuildID, a.ChannelID), "append_message", map[string]any{
		"guild_id":     a.GuildID,
		"channel_id":   a.ChannelID,
		"user_id":      userID,
		"text":         text,
		"session_id":   sessionID,
		"message_id":   messageID,
		"stored_at_ms": storedAtMs,
	})

	event := map[string]any{
		"guild_id":        a.GuildID,
		"channel_id":      a.ChannelID,
		"message_id":      messageID,
		"from_user":       userID,
		"text":            text,
		"delivered_at_ms": storedAtMs,
		"event_type":      "message",
	}

	// Keep last 200 messages in-memory.
	a.Messages = append(a.Messages, event)
	if len(a.Messages) > 200 {
		a.Messages = a.Messages[len(a.Messages)-200:]
	}

	// Fan out to channel group via FanoutActor.
	_ = host.Send(fanoutActorID(), "deliver_channel_event", event)

	// Record audit event.
	_ = host.Send(auditEventActorID(), "record_event", map[string]any{
		"event_type": "channel_message",
		"guild_id":   a.GuildID,
		"channel_id": a.ChannelID,
		"message_id": messageID,
		"user_id":    userID,
	})

	a.LastMessageID = messageID
	a.TotalMessages++
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_channel_messages": 1})
	return marshal(map[string]any{
		"status":          "ok",
		"guild_id":        a.GuildID,
		"channel_id":      a.ChannelID,
		"message_id":      messageID,
		"recipient_count": len(a.MemberIndex),
	})
}

func (a *channelActor) handleHistory(payload map[string]any) string {
	limit := payloadInt(payload, "limit", 50)
	if limit <= 0 {
		limit = 50
	}
	recent := lastNMaps(a.Messages, limit)
	msgs := make([]any, len(recent))
	for i, m := range recent {
		msgs[i] = m
	}
	return marshal(map[string]any{
		"guild_id":      a.GuildID,
		"channel_id":    a.ChannelID,
		"messages":      msgs,
		"count":         len(recent),
		"message_count": len(a.Messages),
	})
}

func (a *channelActor) handleStatus() string {
	members := make([]string, 0, len(a.MemberIndex))
	for k := range a.MemberIndex {
		members = append(members, k)
	}
	typingUsers := make([]string, 0, len(a.TypingDeadlines))
	for k := range a.TypingDeadlines {
		typingUsers = append(typingUsers, k)
	}
	return marshal(map[string]any{
		"guild_id":        a.GuildID,
		"channel_id":      a.ChannelID,
		"members":         members,
		"typing_users":    typingUsers,
		"last_message_id": a.LastMessageID,
		"total_messages":  a.TotalMessages,
		"channel_group":   channelGroup(a.GuildID, a.ChannelID),
	})
}

// ---------------------------------------------------------------------------
// MessageStoreActor
// ---------------------------------------------------------------------------

type messageStoreActor struct {
	plexspaces.BaseActor

	ApplicationID  string           `json:"application_id"`
	GuildID        string           `json:"guild_id"`
	ChannelID      string           `json:"channel_id"`
	Messages       []map[string]any `json:"messages"`
	NextMessageSeq int              `json:"next_message_seq"`
}

func newMessageStoreActor() *messageStoreActor {
	a := &messageStoreActor{
		Messages:       []map[string]any{},
		NextMessageSeq: 1,
	}
	a.SetSelf(a)
	return a
}

func (a *messageStoreActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.GuildID, a.ChannelID = decodeChannelParts(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.Messages == nil {
		a.Messages = []map[string]any{}
	}
	if a.NextMessageSeq < 1 {
		a.NextMessageSeq = 1
	}
	return ""
}

func (a *messageStoreActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "append_message":
		return a.handleAppendMessage(payload)
	case "history":
		return a.handleHistory(payload)
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *messageStoreActor) handleAppendMessage(payload map[string]any) string {
	if gid := payloadString(payload, "guild_id"); gid != "" {
		a.GuildID = gid
	}
	if cid := payloadString(payload, "channel_id"); cid != "" {
		a.ChannelID = cid
	}
	messageID := payloadString(payload, "message_id")
	if messageID == "" {
		messageID = a.ChannelID + "-" + intToStr(a.NextMessageSeq)
	}
	storedAtMs := payloadInt64(payload, "stored_at_ms", 0)
	if storedAtMs == 0 {
		storedAtMs = int64(host.NowMs())
	}
	message := map[string]any{
		"message_id":   messageID,
		"guild_id":     a.GuildID,
		"channel_id":   a.ChannelID,
		"user_id":      payloadString(payload, "user_id"),
		"text":         payloadString(payload, "text"),
		"session_id":   payloadString(payload, "session_id"),
		"stored_at_ms": storedAtMs,
	}
	a.Messages = append(a.Messages, message)
	newSeq := len(a.Messages) + 1
	if a.NextMessageSeq < newSeq {
		a.NextMessageSeq = newSeq
	} else {
		a.NextMessageSeq++
	}
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_messages_stored": 1})
	return marshal(map[string]any{
		"status":        "stored",
		"message_id":    messageID,
		"stored_at_ms":  storedAtMs,
		"message_count": len(a.Messages),
	})
}

func (a *messageStoreActor) handleHistory(payload map[string]any) string {
	limit := payloadInt(payload, "limit", 50)
	if limit <= 0 {
		limit = 50
	}
	recent := lastNMaps(a.Messages, limit)
	msgs := make([]any, len(recent))
	for i, m := range recent {
		msgs[i] = m
	}
	return marshal(map[string]any{
		"guild_id":      a.GuildID,
		"channel_id":    a.ChannelID,
		"messages":      msgs,
		"count":         len(recent),
		"message_count": len(a.Messages),
	})
}

// ---------------------------------------------------------------------------
// FanoutActor
// ---------------------------------------------------------------------------

type fanoutActor struct {
	plexspaces.BaseActor

	ApplicationID string `json:"application_id"`
	ActorName     string `json:"actor_name"`
	Deliveries    int    `json:"deliveries"`
}

func newFanoutActor() *fanoutActor {
	a := &fanoutActor{}
	a.SetSelf(a)
	return a
}

func (a *fanoutActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.ActorName = actorInstanceName(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	// Best-effort registry registration — ignore errors.
	_ = host.Registry().Register(plexspaces.ObjectRegistration{ObjectID: cfg.ActorID, ObjectType: "actor", ObjectCategory: "fanout"})
	return ""
}

func (a *fanoutActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "deliver_channel_event":
		return a.handleDeliverChannelEvent(payload)
	case "stats":
		return marshal(map[string]any{"deliveries": a.Deliveries, "actor_name": a.ActorName})
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *fanoutActor) handleDeliverChannelEvent(payload map[string]any) string {
	guildID := payloadString(payload, "guild_id")
	channelID := payloadString(payload, "channel_id")
	group := channelGroup(guildID, channelID)

	members, _ := host.PG().Members(group)
	_ = host.PG().Broadcast(group, "deliver_channel_event", payload)
	a.Deliveries++
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_fanout_events": 1})
	return marshal(map[string]any{
		"status":          "broadcast",
		"group":           group,
		"recipient_count": len(members),
		"recipients":      members,
		"deliveries":      a.Deliveries,
	})
}

// ---------------------------------------------------------------------------
// AuditEventActor
// ---------------------------------------------------------------------------

type auditEventActor struct {
	plexspaces.BaseActor

	ApplicationID string           `json:"application_id"`
	ActorName     string           `json:"actor_name"`
	RecentEvents  []map[string]any `json:"recent_events"`
}

func newAuditEventActor() *auditEventActor {
	a := &auditEventActor{RecentEvents: []map[string]any{}}
	a.SetSelf(a)
	return a
}

func (a *auditEventActor) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.ActorName = actorInstanceName(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.RecentEvents == nil {
		a.RecentEvents = []map[string]any{}
	}
	// Best-effort registry registration — ignore errors.
	_ = host.Registry().Register(plexspaces.ObjectRegistration{ObjectID: cfg.ActorID, ObjectType: "actor", ObjectCategory: "audit_event"})
	return ""
}

func (a *auditEventActor) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "record_event":
		return a.handleRecordEvent(payload)
	case "stats":
		return a.handleStats()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

func (a *auditEventActor) handleRecordEvent(payload map[string]any) string {
	event := map[string]any{
		"event_type":     payloadString(payload, "event_type"),
		"guild_id":       payloadString(payload, "guild_id"),
		"channel_id":     payloadString(payload, "channel_id"),
		"message_id":     payloadString(payload, "message_id"),
		"user_id":        payloadString(payload, "user_id"),
		"recorded_at_ms": host.NowMs(),
	}
	a.RecentEvents = append(a.RecentEvents, event)
	if len(a.RecentEvents) > 100 {
		a.RecentEvents = a.RecentEvents[len(a.RecentEvents)-100:]
	}
	_ = host.TS().Write([]any{
		"audit",
		payloadString(payload, "event_type"),
		payloadString(payload, "guild_id"),
		payloadString(payload, "channel_id"),
		payloadString(payload, "message_id"),
		payloadString(payload, "user_id"),
	})
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_audit_events": 1})
	return "{}"
}

func (a *auditEventActor) handleStats() string {
	events := make([]any, len(a.RecentEvents))
	for i, e := range a.RecentEvents {
		events[i] = e
	}
	return marshal(map[string]any{
		"actor_name":    a.ActorName,
		"event_count":   len(a.RecentEvents),
		"recent_events": events,
	})
}

// ---------------------------------------------------------------------------
// ModerationWorkflow  (implements Actor + WorkflowActor)
// ---------------------------------------------------------------------------

type moderationWorkflow struct {
	plexspaces.BaseActor

	ApplicationID string   `json:"application_id"`
	ReportID      string   `json:"report_id"`
	Status        string   `json:"status"`
	MessageID     string   `json:"message_id"`
	Reason        string   `json:"reason"`
	ReporterID    string   `json:"reporter_id"`
	Resolution    string   `json:"resolution"`
	Signals       []string `json:"signals"`
}

func newModerationWorkflow() *moderationWorkflow {
	a := &moderationWorkflow{
		Status:  "pending",
		Signals: []string{},
	}
	a.SetSelf(a)
	return a
}

func (a *moderationWorkflow) Init(configJSON string) string {
	var cfg appConfig
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	a.ApplicationID = actorApplicationID(cfg.ActorID)
	a.SetRuntimeMetadata(cfg.ActorID)
	if a.Status == "" {
		a.Status = "pending"
	}
	if a.Signals == nil {
		a.Signals = []string{}
	}
	return ""
}

// Handle covers query-via-message and direct fire-and-forget operations.
func (a *moderationWorkflow) Handle(_ string, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch msgType {
	case "status":
		return a.currentStatus()
	case "review":
		a.applyReview(payloadString(payload, "moderator_id"), payloadString(payload, "resolution"))
		return "{}"
	case "close":
		resolution := payloadString(payload, "resolution")
		if resolution == "" {
			resolution = "dismissed"
		}
		a.applyClose(resolution)
		return "{}"
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// Run implements WorkflowActor — called when the workflow starts.
func (a *moderationWorkflow) Run(payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	if a.ApplicationID == "" {
		a.ApplicationID = actorApplicationID(host.SelfID())
	}
	reportID := payloadString(payload, "report_id")
	if a.ReportID == "" {
		if reportID != "" {
			a.ReportID = reportID
		} else {
			a.ReportID = actorInstanceName(host.SelfID())
		}
	}
	a.MessageID = payloadString(payload, "message_id")
	a.ReporterID = payloadString(payload, "reporter_id")
	a.Reason = payloadString(payload, "reason")
	a.Status = "under_review"
	safeMetricsAdd(a.ApplicationID, map[string]int{"chat_moderation_reports": 1})
	return marshal(map[string]any{
		"report_id":  a.ReportID,
		"status":     a.Status,
		"message_id": a.MessageID,
	})
}

// Signal implements WorkflowActor — handles "review" and "close" signals.
func (a *moderationWorkflow) Signal(name, payloadJSON string) {
	payload := parsePayload(payloadJSON)
	switch name {
	case "review":
		a.applyReview(payloadString(payload, "moderator_id"), payloadString(payload, "resolution"))
	case "close":
		resolution := payloadString(payload, "resolution")
		if resolution == "" {
			resolution = "dismissed"
		}
		a.applyClose(resolution)
	}
}

// Query implements WorkflowActor — answers "status" queries.
func (a *moderationWorkflow) Query(name, _ string) string {
	if name != "status" {
		return marshal(map[string]any{"error": "unknown query: " + name})
	}
	return a.currentStatus()
}

func (a *moderationWorkflow) applyReview(moderatorID, resolution string) {
	a.Resolution = resolution
	a.Status = "reviewed"
	a.Signals = append(a.Signals, "review:"+moderatorID+":"+resolution)
}

func (a *moderationWorkflow) applyClose(resolution string) {
	a.Resolution = resolution
	a.Status = "closed"
	a.Signals = append(a.Signals, "close::"+resolution)
}

func (a *moderationWorkflow) currentStatus() string {
	return marshal(map[string]any{
		"report_id":   a.ReportID,
		"status":      a.Status,
		"message_id":  a.MessageID,
		"reporter_id": a.ReporterID,
		"reason":      a.Reason,
		"resolution":  a.Resolution,
		"signals":     a.Signals,
	})
}

// ---------------------------------------------------------------------------
// Router registration + main
// ---------------------------------------------------------------------------

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("SessionActor", func() plexspaces.Actor { return newSessionActor() })
	router.Route("GuildActor", func() plexspaces.Actor { return newGuildActor() })
	router.Route("ChannelActor", func() plexspaces.Actor { return newChannelActor() })
	router.Route("PresenceActor", func() plexspaces.Actor { return newPresenceActor() })
	router.Route("MessageStoreActor", func() plexspaces.Actor { return newMessageStoreActor() })
	router.Route("FanoutActor", func() plexspaces.Actor { return newFanoutActor() })
	router.Route("AuditEventActor", func() plexspaces.Actor { return newAuditEventActor() })
	router.Route("ConnectionFSM", func() plexspaces.Actor { return newConnectionFSM() })
	router.Route("ModerationWorkflow", func() plexspaces.Actor { return newModerationWorkflow() })
	plexspaces.Register(router)
}

func main() {}
