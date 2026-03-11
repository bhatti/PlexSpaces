// SPDX-License-Identifier: LGPL-2.1-or-later
// AWS Durable Lambda → PlexSpaces: Serverless webhook processor (Go WASM)
//
// Exactly-once with deduplication: idempotency_key in payload; first request
// is processed and response cached; duplicate keys return cached response.
// GenServer + durability for persistent idempotency store.

package main

import (
	"encoding/json"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// WebhookProcessor implements GenServer-style actor with idempotent webhook handling.
type WebhookProcessor struct {
	plexspaces.BaseActor

	ActorID         string            `json:"actor_id"`
	Processed       map[string]string `json:"processed"`         // idempotency_key -> response JSON
	TotalProcessed  int               `json:"total_processed"`
	TotalDedupHits  int               `json:"total_dedup_hits"`
	TotalComputeMs  float64           `json:"total_compute_ms"`
	TotalCoordMs    float64           `json:"total_coord_ms"`
	CreatedAtMs    uint64            `json:"created_at_ms"`
	UpdatedAtMs    uint64            `json:"updated_at_ms"`
}

func NewWebhookProcessor() *WebhookProcessor {
	a := &WebhookProcessor{
		Processed: make(map[string]string),
	}
	a.SetSelf(a)
	return a
}

func (w *WebhookProcessor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	if config.ActorID != "" {
		w.ActorID = config.ActorID
	}
	w.CreatedAtMs = host.NowMs()
	w.UpdatedAtMs = w.CreatedAtMs
	return ""
}

func (w *WebhookProcessor) Handle(from, msgType, payloadJSON string) string {
	switch msgType {
	case "webhook":
		return w.handleWebhook(payloadJSON)
	case "status":
		return w.handleStatus(payloadJSON)
	default:
		return marshal(map[string]any{"error": "unknown operation", "op": msgType})
	}
}

func (w *WebhookProcessor) handleWebhook(payloadJSON string) string {
	t0 := host.NowMs()
	var payload struct {
		IdempotencyKey string          `json:"idempotency_key"`
		EventID        string          `json:"event_id"`
		Body           json.RawMessage `json:"body"`
	}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return marshal(map[string]any{"error": "invalid payload"})
	}
	key := payload.IdempotencyKey
	if key == "" {
		key = payload.EventID
	}
	if key == "" {
		return marshal(map[string]any{"error": "idempotency_key or event_id required"})
	}

	w.UpdatedAtMs = host.NowMs()

	if cached, ok := w.Processed[key]; ok {
		w.TotalDedupHits++
		elapsed := host.NowMs() - t0
		w.TotalCoordMs += float64(elapsed)
		return cached
	}

	// Simulate process (~5ms)
	computeMs := 5.0
	w.TotalProcessed++
	result := map[string]any{
		"ok":               true,
		"idempotency_key":  key,
		"processed_at_ms":  w.UpdatedAtMs,
		"total_processed":  w.TotalProcessed,
		"total_dedup_hits": w.TotalDedupHits,
		"total_compute_ms": w.TotalComputeMs + computeMs,
		"total_coord_ms":   w.TotalCoordMs,
	}
	w.TotalComputeMs += computeMs
	elapsed := host.NowMs() - t0
	if uint64(elapsed) > uint64(computeMs) {
		w.TotalCoordMs += float64(uint64(elapsed) - uint64(computeMs))
	}
	respJSON := marshal(result)
	w.Processed[key] = respJSON
	return respJSON
}

func (w *WebhookProcessor) handleStatus(_ string) string {
	return marshal(map[string]any{
		"actor_id":         w.ActorID,
		"total_processed": w.TotalProcessed,
		"total_dedup_hits": w.TotalDedupHits,
		"total_compute_ms": w.TotalComputeMs,
		"total_coord_ms":   w.TotalCoordMs,
		"created_at_ms":    w.CreatedAtMs,
		"updated_at_ms":    w.UpdatedAtMs,
		"keys_stored":      len(w.Processed),
	})
}

func marshal(v any) string {
	data, err := json.Marshal(v)
	if err != nil {
		return `{"error":"marshal failed"}`
	}
	return string(data)
}

func init() {
	plexspaces.Register(NewWebhookProcessor())
}

func main() {}
