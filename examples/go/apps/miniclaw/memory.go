// SPDX-License-Identifier: AGPL-3.0-or-later
// MemoryActor — scoped KV + TupleSpace memory store.
// AuditEventActor — fire-and-forget audit trail with two-cursor polling (NanoClaw Pattern 3).
// AgentStateFSM — agent lifecycle state machine (GenFSM).
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

// ========================================================================
// MemoryActor
// ========================================================================

// MemoryActor provides scoped memory storage backed by KV (persistent) and
// TupleSpace (queryable). Supports global, agent, and session scope namespacing.
type MemoryActor struct {
	plexspaces.BaseActor
	MemoryCount int `json:"memory_count"`
}

func NewMemoryActor() plexspaces.Actor {
	a := &MemoryActor{}
	a.SetSelf(a)
	return a
}

func newMemoryActor() *MemoryActor {
	a := &MemoryActor{}
	a.SetSelf(a)
	return a
}

func (m *MemoryActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	m.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:memory"); err != nil {
		host.Warn(fmt.Sprintf("MemoryActor: failed to join svc:memory: %v", err))
	}
	host.Info(fmt.Sprintf("MemoryActor Init actor_id=%s", config.ActorID))
	return ""
}

func (m *MemoryActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "store_memory":
		return m.storeMemory(p)
	case "recall_memory":
		return m.recallMemory(p)
	case "list_memories":
		return m.listMemories(p)
	case "delete_memory":
		return m.deleteMemory(p)
	case "get_stats":
		return marshal(map[string]any{"status": "ok", "memory_count": m.MemoryCount})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (m *MemoryActor) storeMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	key := stringVal(p, "key", "")
	value := stringVal(p, "value", "")
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}

	kvKey := fmt.Sprintf("mem:%s:%s:%s", scope, scopeID, key)
	host.KVPut(kvKey, value)
	tsOK := true
	if result := host.TS().Write([]any{"memory", scope, scopeID, key, value}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("MemoryActor: TS write failed for key=%s: %s", key, result))
		tsOK = false
	}
	_ = tsOK
	m.MemoryCount++
	m.IncrCounter(host, "memories_stored")
	host.Info(fmt.Sprintf("MemoryActor: stored scope=%s key=%s", scope, key))
	return marshal(map[string]any{"status": "ok", "key": key, "scope": scope})
}

func (m *MemoryActor) recallMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	query := stringVal(p, "query", "")

	tuples := host.TS().ReadAll([]any{"memory", scope, scopeID, nil, nil})
	memories := make([]any, 0)
	for _, t := range tuples {
		if len(t) < 5 {
			continue
		}
		memKey, _ := t[3].(string)
		memVal, _ := t[4].(string)
		if query == "" ||
			strings.Contains(strings.ToLower(memKey), strings.ToLower(query)) ||
			strings.Contains(strings.ToLower(memVal), strings.ToLower(query)) {
			memories = append(memories, map[string]any{
				"key":      memKey,
				"value":    memVal,
				"scope":    scope,
				"scope_id": scopeID,
			})
		}
	}
	return marshal(map[string]any{"status": "ok", "memories": memories, "count": len(memories), "query": query})
}

func (m *MemoryActor) listMemories(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	tuples := host.TS().ReadAll([]any{"memory", scope, scopeID, nil, nil})
	memories := make([]any, 0, len(tuples))
	for _, t := range tuples {
		if len(t) < 5 {
			continue
		}
		memKey, _ := t[3].(string)
		memVal, _ := t[4].(string)
		memories = append(memories, map[string]any{"key": memKey, "value": memVal, "scope": scope})
	}
	return marshal(map[string]any{"status": "ok", "memories": memories, "count": len(memories)})
}

func (m *MemoryActor) deleteMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	key := stringVal(p, "key", "")
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	host.KVDelete(fmt.Sprintf("mem:%s:%s:%s", scope, scopeID, key))
	if m.MemoryCount > 0 {
		m.MemoryCount--
	}
	return marshal(map[string]any{"status": "ok", "key": key})
}

// ========================================================================
// AuditEventActor
// ========================================================================

// AuditEventActor records fire-and-forget audit events to TupleSpace and KV.
// Uses a monotonic Watermark (NanoClaw Pattern 3: Two-Cursor Message Processing)
// so consumers can poll with a per-consumer cursor for ordered, no-miss delivery.
// Declared with behavior_kind = "GenEvent" in app-config.toml.
type AuditEventActor struct {
	plexspaces.BaseActor
	EventsLogged  int    `json:"events_logged"`
	LastEventType string `json:"last_event_type"`
	Watermark     int64  `json:"watermark"`
}

func NewAuditEventActor() plexspaces.Actor {
	a := &AuditEventActor{}
	a.SetSelf(a)
	return a
}

func newAuditEventActor() *AuditEventActor {
	a := &AuditEventActor{}
	a.SetSelf(a)
	return a
}

func (a *AuditEventActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:audit"); err != nil {
		host.Warn(fmt.Sprintf("AuditEventActor: failed to join svc:audit: %v", err))
	}
	host.Info(fmt.Sprintf("AuditEventActor Init actor_id=%s", config.ActorID))
	return ""
}

func (a *AuditEventActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "log_event":
		return a.logEvent(p)
	case "query_events":
		return a.queryEvents(p)
	case "poll_events":
		return a.pollEvents(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"events_logged":   a.EventsLogged,
			"last_event_type": a.LastEventType,
			"watermark":       a.Watermark,
		})
	default:
		if et := stringVal(p, "event_type", ""); et != "" {
			return a.logEvent(p)
		}
		return marshal(map[string]any{"ok": true})
	}
}

func (a *AuditEventActor) logEvent(p map[string]any) string {
	eventType := stringVal(p, "event_type", "unknown")
	detail := stringVal(p, "detail", "")
	ts := p["timestamp"]
	if ts == nil {
		ts = host.NowMs()
	}

	a.Watermark++
	seq := a.Watermark

	entry := map[string]any{
		"seq":        seq,
		"event_type": eventType,
		"detail":     detail,
		"source":     stringVal(p, "source", ""),
		"timestamp":  ts,
	}
	entryJSON, err := json.Marshal(entry)
	if err != nil {
		host.Warn(fmt.Sprintf("AuditEventActor: failed to marshal audit entry for event=%s: %v", eventType, err))
		return marshal(map[string]any{"ok": false, "error": "marshal_failed"})
	}

	host.KVPut(fmt.Sprintf("audit:seq:%d", seq), string(entryJSON))
	if result := host.TS().Write([]any{"audit", eventType, ts, string(entryJSON)}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("AuditEventActor: TS write failed for event=%s: %s", eventType, result))
	}

	a.EventsLogged++
	a.LastEventType = eventType
	a.IncrCounter(host, "audit_events")
	return marshal(map[string]any{"ok": true, "seq": seq})
}

// pollEvents implements the two-cursor pattern: consumer sends its last-seen
// cursor; returns all events from (cursor+1) to current Watermark and the new
// cursor position. Consumers advance their own cursor in KV.
func (a *AuditEventActor) pollEvents(p map[string]any) string {
	consumerID := stringVal(p, "consumer_id", "default")
	limit := intVal(p, "limit", 50)

	cursorKey := "audit:cursor:" + consumerID
	cursorRaw := host.KVGet(cursorKey)
	var cursor int64
	if cursorRaw != "" {
		if n, err := strconv.ParseInt(cursorRaw, 10, 64); err == nil {
			cursor = n
		}
	}

	events := make([]any, 0)
	newCursor := cursor
	for seq := cursor + 1; seq <= a.Watermark && len(events) < limit; seq++ {
		raw := host.KVGet(fmt.Sprintf("audit:seq:%d", seq))
		if raw == "" {
			continue
		}
		var entry map[string]any
		if err := json.Unmarshal([]byte(raw), &entry); err == nil {
			events = append(events, entry)
			newCursor = seq
		}
	}

	host.KVPut(cursorKey, strconv.FormatInt(newCursor, 10))
	return marshal(map[string]any{
		"status":      "ok",
		"consumer_id": consumerID,
		"events":      events,
		"count":       len(events),
		"cursor":      newCursor,
		"watermark":   a.Watermark,
	})
}

func (a *AuditEventActor) queryEvents(p map[string]any) string {
	eventType := stringVal(p, "event_type", "")
	limit := intVal(p, "limit", 10)

	var tuples [][]any
	if eventType != "" {
		tuples = host.TS().ReadAll([]any{"audit", eventType, nil, nil})
	} else {
		tuples = host.TS().ReadAll([]any{"audit", nil, nil, nil})
	}
	if len(tuples) > limit {
		tuples = tuples[len(tuples)-limit:]
	}

	events := make([]any, 0, len(tuples))
	for _, t := range tuples {
		if len(t) < 4 {
			continue
		}
		et, _ := t[1].(string)
		ts := t[2]
		detail, _ := t[3].(string)
		events = append(events, map[string]any{"event_type": et, "timestamp": ts, "detail": detail})
	}
	return marshal(map[string]any{"status": "ok", "events": events, "count": len(events)})
}

// ========================================================================
// AgentStateFSM
// ========================================================================

// AgentStateFSM tracks the agent processing lifecycle.
// States: idle → processing → tool_executing → processing → responding → idle
// Any state can transition to error; error can recover to idle.
type AgentStateFSM struct {
	plexspaces.BaseActor
	FSMState        string `json:"fsm_state"`
	TransitionCount int    `json:"transition_count"`
}

func NewAgentStateFSM() plexspaces.Actor {
	a := &AgentStateFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func newAgentStateFSM() *AgentStateFSM {
	a := &AgentStateFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func (f *AgentStateFSM) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	f.SetRuntimeMetadata(config.ActorID)
	f.FSMState = "idle"
	if err := host.PG().Join("svc:agent_fsm"); err != nil {
		host.Warn(fmt.Sprintf("AgentStateFSM: failed to join svc:agent_fsm: %v", err))
	}
	host.Info(fmt.Sprintf("AgentStateFSM Init actor_id=%s", config.ActorID))
	return ""
}

func (f *AgentStateFSM) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "transition":
		return f.transition(p)
	case "get_state":
		return marshal(map[string]any{
			"status":           "ok",
			"state":            f.FSMState,
			"transition_count": f.TransitionCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (f *AgentStateFSM) transition(p map[string]any) string {
	to := stringVal(p, "to", "")
	if to == "" {
		return marshal(map[string]any{"error": "to state is required"})
	}

	if to == "error" || (f.FSMState == "error" && to == "idle") {
		from := f.FSMState
		f.FSMState = to
		f.TransitionCount++
		return marshal(map[string]any{"status": "ok", "from": from, "to": to, "state": f.FSMState})
	}

	allowed := validTransitions()
	validTargets, ok := allowed[f.FSMState]
	if !ok {
		return marshal(map[string]any{"error": fmt.Sprintf("unknown source state: %s", f.FSMState), "state": f.FSMState})
	}
	for _, valid := range validTargets {
		if valid == to {
			from := f.FSMState
			f.FSMState = to
			f.TransitionCount++
			f.IncrCounter(host, "fsm_transitions")
			host.Debug(fmt.Sprintf("AgentStateFSM: %s → %s", from, to))
			return marshal(map[string]any{"status": "ok", "from": from, "to": to, "state": f.FSMState})
		}
	}

	host.Warn(fmt.Sprintf("AgentStateFSM: invalid transition %s → %s", f.FSMState, to))
	return marshal(map[string]any{
		"error": fmt.Sprintf("invalid transition %s → %s", f.FSMState, to),
		"state": f.FSMState,
	})
}

func validTransitions() map[string][]string {
	return map[string][]string{
		"idle":           {"processing"},
		"processing":     {"tool_executing", "responding"},
		"tool_executing": {"processing"},
		"responding":     {"idle"},
		"error":          {"idle"},
	}
}
