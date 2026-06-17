// SPDX-License-Identifier: AGPL-3.0-or-later
// MemoryActor — tiered memory storage (core → reachable → deep).
// Demonstrates: KV (core memories), TupleSpace (reachable layer with pattern query),
// BlobStorage (deep/archived memories), PutWithTTL (reachable decay).
// AuditEventActor — fire-and-forget audit trail with two-cursor polling.
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// MemoryActor provides three-tier memory:
//
//   - Core:      Always in KV; never expires. Used for critical facts.
//   - Reachable: KV with TTL + TupleSpace for pattern search. Recent memories.
//   - Deep:      BlobStorage for large/archived content.
type MemoryActor struct {
	plexspaces.BaseActor
	CoreCount      int    `json:"core_count"`
	ReachableCount int    `json:"reachable_count"`
	DeepCount      int    `json:"deep_count"`
	ReachableTTL   uint64 `json:"reachable_ttl_ms"`
}

func NewMemoryActor() plexspaces.Actor {
	a := &MemoryActor{ReachableTTL: 3600000} // 1 hour default
	a.SetSelf(a)
	return a
}

func newMemoryActor() *MemoryActor {
	a := &MemoryActor{ReachableTTL: 3600000}
	a.SetSelf(a)
	return a
}

func (m *MemoryActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	m.SetRuntimeMetadata(config.ActorID)
	if ttl := config.Args["reachable_ttl_ms"]; ttl != "" {
		var n uint64
		if _, err := fmt.Sscan(ttl, &n); err == nil && n > 0 {
			m.ReachableTTL = n
		}
	}
	if err := host.PG().Join("svc:memory"); err != nil {
		host.Warn(fmt.Sprintf("MemoryActor: failed to join svc:memory: %v", err))
	}
	host.Info(fmt.Sprintf("MemoryActor Init actor_id=%s reachable_ttl=%d", config.ActorID, m.ReachableTTL))
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
	case "archive_memory":
		return m.archiveMemory(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"core_count":      m.CoreCount,
			"reachable_count": m.ReachableCount,
			"deep_count":      m.DeepCount,
			"reachable_ttl":   m.ReachableTTL,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (m *MemoryActor) storeMemory(p map[string]any) string {
	tier := stringVal(p, "tier", "reachable") // core | reachable | deep
	key := stringVal(p, "key", "")
	value := stringVal(p, "value", "")
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")

	if key == "" || value == "" {
		return marshal(map[string]any{"error": "key and value are required"})
	}

	switch tier {
	case "core":
		kvKey := fmt.Sprintf("mem:core:%s:%s:%s", scope, scopeID, key)
		host.KVPut(kvKey, value)
		// Also index in TupleSpace for querying
		_ = host.TS().Write([]any{"memory", "core", scope, scopeID, key, value})
		m.CoreCount++

	case "deep":
		blobKey := "deep:" + scope + ":" + scopeID + ":" + key
		storedID := host.BlobUpload(blobKey, value, "text/plain")
		if plexspaces.IsHostError(storedID) {
			host.Warn(fmt.Sprintf("MemoryActor: blob upload failed: %s", storedID))
			host.KVPut(fmt.Sprintf("mem:deep:%s:%s:%s", scope, scopeID, key), value)
		} else {
			host.KVPut("mem:deep_blob:"+scope+":"+scopeID+":"+key, storedID)
		}
		_ = host.TS().Write([]any{"memory", "deep", scope, scopeID, key, value[:min(200, len(value))]})
		m.DeepCount++

	default: // reachable
		kvKey := fmt.Sprintf("mem:reachable:%s:%s:%s", scope, scopeID, key)
		host.KVPut(kvKey, value) // TTL-like eviction handled by skill maintenance tick
		// TupleSpace entry for pattern-based recall (write without TTL; eviction is handled by KV)
		_ = host.TS().Write([]any{"memory", "reachable", scope, scopeID, key, value})
		m.ReachableCount++
	}

	m.IncrCounter(host, "memories_stored")
	fireAudit("memory_stored", fmt.Sprintf("tier=%s scope=%s key=%s", tier, scope, key))
	return marshal(map[string]any{"status": "ok", "key": key, "tier": tier, "scope": scope})
}

func (m *MemoryActor) recallMemory(p map[string]any) string {
	query := stringVal(p, "query", "")
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	tier := stringVal(p, "tier", "") // empty = search all tiers

	queryLower := strings.ToLower(query)
	memories := make([]any, 0)

	searchTiers := []string{"core", "reachable", "deep"}
	if tier != "" {
		searchTiers = []string{tier}
	}

	for _, t := range searchTiers {
		tuples := host.TS().ReadAll([]any{"memory", t, scope, scopeID, nil, nil})
		for _, tup := range tuples {
			if len(tup) < 6 {
				continue
			}
			memKey, _ := tup[4].(string)
			memVal, _ := tup[5].(string)
			if query == "" ||
				strings.Contains(strings.ToLower(memKey), queryLower) ||
				strings.Contains(strings.ToLower(memVal), queryLower) {
				memories = append(memories, map[string]any{
					"key":      memKey,
					"value":    memVal,
					"tier":     t,
					"scope":    scope,
					"scope_id": scopeID,
				})
			}
		}
	}

	m.IncrCounter(host, "memory_recalls")
	return marshal(map[string]any{
		"status":   "ok",
		"memories": memories,
		"count":    len(memories),
		"query":    query,
		"scope":    scope,
	})
}

func (m *MemoryActor) listMemories(p map[string]any) string {
	tier := stringVal(p, "tier", "")
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")

	tiers := []string{"core", "reachable", "deep"}
	if tier != "" {
		tiers = []string{tier}
	}

	all := make([]any, 0)
	for _, t := range tiers {
		tuples := host.TS().ReadAll([]any{"memory", t, scope, scopeID, nil, nil})
		for _, tup := range tuples {
			if len(tup) < 6 {
				continue
			}
			all = append(all, map[string]any{
				"key":   tup[4],
				"value": tup[5],
				"tier":  t,
				"scope": scope,
			})
		}
	}
	return marshal(map[string]any{"status": "ok", "memories": all, "count": len(all)})
}

func (m *MemoryActor) deleteMemory(p map[string]any) string {
	tier := stringVal(p, "tier", "reachable")
	key := stringVal(p, "key", "")
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	host.KVDelete(fmt.Sprintf("mem:%s:%s:%s:%s", tier, scope, scopeID, key))
	host.KVDelete(fmt.Sprintf("mem:deep_blob:%s:%s:%s", scope, scopeID, key))
	switch tier {
	case "core":
		if m.CoreCount > 0 {
			m.CoreCount--
		}
	case "deep":
		if m.DeepCount > 0 {
			m.DeepCount--
		}
	default:
		if m.ReachableCount > 0 {
			m.ReachableCount--
		}
	}
	return marshal(map[string]any{"status": "ok", "key": key, "tier": tier})
}

// archiveMemory promotes a reachable memory to deep storage.
func (m *MemoryActor) archiveMemory(p map[string]any) string {
	key := stringVal(p, "key", "")
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	// Read current value from reachable tier
	value := host.KVGet(fmt.Sprintf("mem:reachable:%s:%s:%s", scope, scopeID, key))
	if value == "" {
		value = host.KVGet(fmt.Sprintf("mem:core:%s:%s:%s", scope, scopeID, key))
	}
	if value == "" {
		return marshal(map[string]any{"error": "memory not found", "key": key})
	}
	// Store in deep
	result := m.storeMemory(map[string]any{
		"tier":     "deep",
		"key":      key,
		"value":    value,
		"scope":    scope,
		"scope_id": scopeID,
	})
	// Delete from reachable
	host.KVDelete(fmt.Sprintf("mem:reachable:%s:%s:%s", scope, scopeID, key))
	return result
}

// ========================================================================
// AuditEventActor
// ========================================================================

// AuditEventActor records fire-and-forget audit events to TupleSpace and KV.
// Uses a monotonic Watermark so consumers can poll with a per-consumer cursor.
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
		"seq": seq, "event_type": eventType,
		"detail": detail, "source": stringVal(p, "source", ""),
		"timestamp": ts,
	}
	entryJSON, err := json.Marshal(entry)
	if err != nil {
		return marshal(map[string]any{"ok": false, "error": "marshal_failed"})
	}

	host.KVPut(fmt.Sprintf("audit:seq:%d", seq), string(entryJSON))
	_ = host.TS().Write([]any{"audit", eventType, ts, string(entryJSON)})

	a.EventsLogged++
	a.LastEventType = eventType
	a.IncrCounter(host, "audit_events")
	return marshal(map[string]any{"ok": true, "seq": seq})
}

func (a *AuditEventActor) pollEvents(p map[string]any) string {
	consumerID := stringVal(p, "consumer_id", "default")
	limit := intVal(p, "limit", 50)

	cursorKey := "audit:cursor:" + consumerID
	cursorRaw := host.KVGet(cursorKey)
	var cursor int64
	if cursorRaw != "" {
		if n, err := fmt.Sscan(cursorRaw, &cursor); n == 1 && err == nil {
			_ = n
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

	host.KVPut(cursorKey, fmt.Sprintf("%d", newCursor))
	return marshal(map[string]any{
		"status": "ok", "consumer_id": consumerID,
		"events": events, "count": len(events),
		"cursor": newCursor, "watermark": a.Watermark,
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
