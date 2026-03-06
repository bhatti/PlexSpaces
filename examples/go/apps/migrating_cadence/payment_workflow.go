// SPDX-License-Identifier: LGPL-2.1-or-later
// Payment Workflow - Cadence-style (Go WASM)
//
// Idempotent payment processing with retry: validate → authorize (retry) → capture → settle.
// Implements plexspaces.WorkflowActor for workflow_run, workflow_signal:refund/cancel, workflow_query:status|payment_id.
package main

import (
	"encoding/json"
	"hash/fnv"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// PaymentStep represents a completed step in the payment workflow.
type PaymentStep struct {
	Name          string `json:"name"`
	CompletedAtMs uint64 `json:"completed_at_ms"`
	RetryCount    int    `json:"retry_count,omitempty"`
}

// PaymentWorkflow implements WorkflowActor for idempotent payment processing with retries.
type PaymentWorkflow struct {
	plexspaces.BaseActor

	PaymentID       string        `json:"payment_id"`
	IdempotencyKey string        `json:"idempotency_key"`
	AmountCents    int64         `json:"amount_cents"`
	Status         string        `json:"status"` // pending, validated, authorized, captured, settled, refunded, failed
	Steps          []PaymentStep `json:"steps"`
	RefundRequested bool         `json:"refund_requested"`
	TotalComputeMs  float64       `json:"total_compute_ms"`
	TotalCoordMs    float64       `json:"total_coord_ms"`
	RetryCount     int           `json:"retry_count"`
	CreatedAtMs    uint64        `json:"created_at_ms"`
	UpdatedAtMs    uint64        `json:"updated_at_ms"`
}

func NewPaymentWorkflow() *PaymentWorkflow {
	a := &PaymentWorkflow{
		Status: "pending",
		Steps:  make([]PaymentStep, 0),
	}
	a.SetSelf(a)
	return a
}

func (p *PaymentWorkflow) Init(configJSON string) string {
	var config struct {
		ActorID        string `json:"actor_id"`
		PaymentID      string `json:"payment_id"`
		IdempotencyKey string `json:"idempotency_key"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	if config.PaymentID != "" {
		p.PaymentID = config.PaymentID
	}
	if config.IdempotencyKey != "" {
		p.IdempotencyKey = config.IdempotencyKey
	}
	now := host.NowMs()
	p.CreatedAtMs = now
	p.UpdatedAtMs = now
	return ""
}

func (p *PaymentWorkflow) Run(payloadJSON string) string {
	t0 := host.NowMs()
	var payload map[string]interface{}
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	if id, ok := payload["payment_id"].(string); ok && id != "" {
		p.PaymentID = id
	}
	if key, ok := payload["idempotency_key"].(string); ok && key != "" {
		p.IdempotencyKey = key
	}
	if p.IdempotencyKey == "" {
		p.IdempotencyKey = p.PaymentID
	}
	if amt, ok := payload["amount_cents"].(float64); ok {
		p.AmountCents = int64(amt)
	}
	p.UpdatedAtMs = host.NowMs()

	// Idempotency: already completed for this key
	if p.Status == "settled" || p.Status == "refunded" {
		return marshal(map[string]interface{}{
			"status":           p.Status,
			"payment_id":       p.PaymentID,
			"idempotency_key":  p.IdempotencyKey,
			"message":          "already_completed",
			"total_compute_ms": p.TotalComputeMs,
			"total_coord_ms":   p.TotalCoordMs,
		})
	}

	if p.Status != "pending" && p.Status != "validated" && p.Status != "authorized" && p.Status != "captured" {
		return marshal(map[string]interface{}{
			"status":  p.Status,
			"payment_id": p.PaymentID,
			"message": "Workflow already in terminal state",
		})
	}

	computeMs := 0.0

	// Step 1: validate (simulated work ~20ms)
	if !hasStep(p.Steps, "validate") {
		computeMs += 20
		p.Steps = append(p.Steps, PaymentStep{Name: "validate", CompletedAtMs: host.NowMs()})
		p.Status = "validated"
		p.UpdatedAtMs = host.NowMs()
	}
	if p.RefundRequested {
		return p.doRefund("refund_requested")
	}

	// Step 2: authorize (with retry: simulate failure on first attempt for some IDs; ~30ms per attempt)
	if !hasStep(p.Steps, "authorize") {
		maxRetries := 3
		for attempt := 0; attempt < maxRetries; attempt++ {
			computeMs += 30
			// Simulate transient failure: fail first attempt for ~25% of payments (by hash)
			if attempt < maxRetries-1 && simulateAuthorizeFailure(p.PaymentID, attempt) {
				p.RetryCount++
				computeMs += 10
				continue
			}
			p.Steps = append(p.Steps, PaymentStep{Name: "authorize", CompletedAtMs: host.NowMs(), RetryCount: p.RetryCount})
			p.Status = "authorized"
			p.UpdatedAtMs = host.NowMs()
			break
		}
	}
	if p.RefundRequested {
		return p.doRefund("refund_requested")
	}

	// Step 3: capture (~25ms)
	if !hasStep(p.Steps, "capture") {
		computeMs += 25
		p.Steps = append(p.Steps, PaymentStep{Name: "capture", CompletedAtMs: host.NowMs()})
		p.Status = "captured"
		p.UpdatedAtMs = host.NowMs()
	}
	if p.RefundRequested {
		return p.doRefund("refund_requested")
	}

	// Step 4: settle (~25ms)
	if !hasStep(p.Steps, "settle") {
		computeMs += 25
		p.Steps = append(p.Steps, PaymentStep{Name: "settle", CompletedAtMs: host.NowMs()})
		p.Status = "settled"
		p.UpdatedAtMs = host.NowMs()
	}

	// Wall-clock elapsed (may be 0 in WASM if host.NowMs() doesn't advance)
	totalElapsed := float64(host.NowMs() - t0)
	// Always report simulated compute; never clamp to zero when elapsed is 0
	computeReported := computeMs
	if totalElapsed < computeMs {
		totalElapsed = computeMs
	}
	coordReported := totalElapsed - computeReported
	if coordReported < 0 {
		coordReported = 0
	}
	p.TotalComputeMs += computeReported
	p.TotalCoordMs += coordReported

	return marshal(map[string]interface{}{
		"status":            p.Status,
		"payment_id":        p.PaymentID,
		"idempotency_key":   p.IdempotencyKey,
		"steps_completed":  len(p.Steps),
		"retry_count":      p.RetryCount,
		"total_compute_ms": p.TotalComputeMs,
		"total_coord_ms":   p.TotalCoordMs,
	})
}

func simulateAuthorizeFailure(paymentID string, attempt int) bool {
	h := fnv.New32a()
	h.Write([]byte(paymentID))
	return (h.Sum32()%4 == 0) && attempt == 0
}

func (p *PaymentWorkflow) Signal(name, _ string) {
	if name == "refund" || name == "cancel" {
		p.RefundRequested = true
		p.UpdatedAtMs = host.NowMs()
	}
}

func (p *PaymentWorkflow) Query(name, _ string) string {
	switch name {
	case "status":
		return marshal(map[string]interface{}{
			"payment_id":        p.PaymentID,
			"idempotency_key":  p.IdempotencyKey,
			"status":           p.Status,
			"amount_cents":     p.AmountCents,
			"steps_count":      len(p.Steps),
			"retry_count":      p.RetryCount,
			"refund_requested": p.RefundRequested,
			"total_compute_ms": p.TotalComputeMs,
			"total_coord_ms":   p.TotalCoordMs,
			"created_at_ms":    p.CreatedAtMs,
			"updated_at_ms":    p.UpdatedAtMs,
		})
	case "payment_id":
		return marshal(map[string]interface{}{
			"payment_id": p.PaymentID,
			"idempotency_key": p.IdempotencyKey,
		})
	}
	return marshal(map[string]interface{}{"error": "unknown_query", "name": name})
}

func (p *PaymentWorkflow) Handle(from, msgType, payload string) string {
	return `{"error":"use workflow_run / workflow_signal / workflow_query"}`
}

func (p *PaymentWorkflow) doRefund(reason string) string {
	p.Status = "refunded"
	p.UpdatedAtMs = host.NowMs()
	return marshal(map[string]interface{}{
		"status":            "refunded",
		"payment_id":        p.PaymentID,
		"reason":            reason,
		"steps_rolled_back": len(p.Steps),
		"total_compute_ms":  p.TotalComputeMs,
		"total_coord_ms":    p.TotalCoordMs,
	})
}

func hasStep(steps []PaymentStep, name string) bool {
	for _, s := range steps {
		if s.Name == name {
			return true
		}
	}
	return false
}

func marshal(v map[string]interface{}) string {
	b, _ := json.Marshal(v)
	return string(b)
}

func init() {
	plexspaces.Register(NewPaymentWorkflow())
}

func main() {}
