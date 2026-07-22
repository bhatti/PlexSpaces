// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Alarms API Example — Go WASM actor
//
// Demonstrates the Cloudflare Durable Objects alarm() pattern: a RequestQueue
// actor that batches incoming requests and processes them 10 seconds after the
// first write, using a durable alarm that survives actor deactivation.
//
// ## Cloudflare DO vs PlexSpaces Go
//
// | Cloudflare DO                             | PlexSpaces Go                          |
// |-------------------------------------------|----------------------------------------|
// | export class RequestQueue extends DO      | RequestQueue struct + BaseActor        |
// | this.ctx.storage.get('count')             | host.KV().Get("count")                 |
// | this.ctx.storage.put('count', n)          | host.KV().Put("count", val)            |
// | this.ctx.storage.setAlarm(Date.now()+10s) | host.Alarm().Set(nowMs + 10_000)       |
// | this.ctx.storage.getAlarm()               | host.Alarm().Get()                     |
// | async alarm() { ... }                     | case "__alarm__":                      |
// | new Response(JSON.stringify(result))      | return marshal(map[string]any{...})    |
// | wrangler.toml [[durable_objects]]         | app-config.toml [[supervisor.children]] |

package main

import (
	"encoding/json"
	"fmt"
	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ============================================================================
// RequestQueue Actor (Durable Object alarm() equivalent)
// ============================================================================

// QueueItem represents a single enqueued request.
type QueueItem struct {
	ID         int         `json:"id"`
	Data       interface{} `json:"data"`
	EnqueuedAt uint64      `json:"enqueued_at"`
}

// RequestQueue batches incoming items and processes them when the durable alarm fires.
// Each instance is an isolated actor — like a Durable Object per queue.
type RequestQueue struct {
	plexspaces.BaseActor

	// Queued items pending batch processing
	Items []QueueItem `json:"items"`

	// Current queue depth (mirrors len(Items) but durable via KV)
	Count int `json:"count"`

	// Lifecycle counters
	TotalProcessed   int `json:"total_processed"`
	TotalAlarmFires  int `json:"total_alarm_fires"`
}

// host is the package-level host function accessor (created once, reused across calls).
var host = plexspaces.NewHost()

// NewRequestQueue creates a new RequestQueue actor instance.
func NewRequestQueue() plexspaces.Actor {
	q := &RequestQueue{
		Items: make([]QueueItem, 0),
	}
	q.SetSelf(q)
	return q
}

// Init is called by the framework before the first Handle. Stores actor identity.
func (q *RequestQueue) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	q.SetRuntimeMetadata(config.ActorID)
	host.Info(fmt.Sprintf("RequestQueue %s: initialized", q.ActorID()))
	return ""
}

// Handle dispatches incoming messages to the appropriate handler.
func (q *RequestQueue) Handle(fromActor, msgType, payloadJSON string) string {
	switch msgType {
	case "enqueue":
		return q.handleEnqueue(payloadJSON)
	case "status":
		return q.handleStatus()
	case "reset":
		return q.handleReset()
	case "__alarm__":
		return q.handleAlarm()
	default:
		return marshal(map[string]any{"error": "unknown operation: " + msgType})
	}
}

// handleEnqueue adds an item to the queue.
// Sets a durable alarm on the FIRST item (10 seconds from now).
// Equivalent to Cloudflare DO:
//
//	if (count === 0) { await this.ctx.storage.setAlarm(Date.now() + 10_000) }
func (q *RequestQueue) handleEnqueue(payloadJSON string) string {
	var req struct {
		Item interface{} `json:"item"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &req); err != nil {
		req.Item = payloadJSON
	}
	if req.Item == nil {
		req.Item = fmt.Sprintf("item-%d", q.Count+1)
	}

	wasEmpty := q.Count == 0
	item := QueueItem{
		ID:         q.Count + 1,
		Data:       req.Item,
		EnqueuedAt: host.NowMs(),
	}
	q.Items = append(q.Items, item)
	q.Count++

	alarmSet := false
	if wasEmpty {
		// First item: schedule alarm 10 seconds from now.
		// Equivalent to: await this.ctx.storage.setAlarm(Date.now() + 10_000)
		fireAt := host.NowMs() + 10_000
		if err := host.Alarm().Set(fireAt); err != nil {
			return marshal(map[string]any{"error": "alarm_set failed: " + err.Error()})
		}
		alarmSet = true
		host.Info(fmt.Sprintf("RequestQueue %s: first item, alarm set for 10s at ts=%d",
			q.ActorID(), fireAt))
	}

	return marshal(map[string]any{
		"status":    "ok",
		"queued":    q.Count,
		"item_id":   item.ID,
		"alarm_set": alarmSet,
	})
}

// handleStatus returns the current queue depth and next alarm timestamp.
// Equivalent to Cloudflare DO: this.ctx.storage.getAlarm()
func (q *RequestQueue) handleStatus() string {
	fireAt, err := host.Alarm().Get()
	errMsg := ""
	if err != nil {
		errMsg = err.Error()
	}
	return marshal(map[string]any{
		"status":            "ok",
		"count":             q.Count,
		"alarm_at":          fireAt,
		"alarm_set":         fireAt > 0,
		"total_processed":   q.TotalProcessed,
		"total_alarm_fires": q.TotalAlarmFires,
		"error":             errMsg,
	})
}

// handleReset clears the queue and cancels any pending alarm (for test repeatability).
func (q *RequestQueue) handleReset() string {
	q.Items = make([]QueueItem, 0)
	q.Count = 0
	_ = host.Alarm().Delete()
	host.Info(fmt.Sprintf("RequestQueue %s: queue reset", q.ActorID()))
	return marshal(map[string]any{
		"status": "ok",
		"reset":  true,
	})
}

// handleAlarm is invoked by the framework when the scheduled alarm fires.
// Equivalent to Cloudflare DO: async alarm() { ... }
// Processes all batched items and clears the queue.
func (q *RequestQueue) handleAlarm() string {
	processed := q.Count
	q.TotalAlarmFires++
	q.TotalProcessed += processed

	host.Info(fmt.Sprintf("RequestQueue %s: alarm fired, processing %d items",
		q.ActorID(), processed))

	for _, item := range q.Items {
		host.Info(fmt.Sprintf("RequestQueue %s: processing item %d: %v",
			q.ActorID(), item.ID, item.Data))
	}

	// Clear the queue
	q.Items = make([]QueueItem, 0)
	q.Count = 0

	return marshal(map[string]any{
		"status":            "ok",
		"processed":         processed,
		"total_processed":   q.TotalProcessed,
		"total_alarm_fires": q.TotalAlarmFires,
	})
}

// ============================================================================
// Helpers
// ============================================================================

func marshal(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `{"error":"marshal failed"}`
	}
	return string(data)
}

// ============================================================================
// Registration
// ============================================================================

// init registers actors before the host calls any exported functions.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("RequestQueueActor", NewRequestQueue)
	plexspaces.Register(router)
}

func main() {}
