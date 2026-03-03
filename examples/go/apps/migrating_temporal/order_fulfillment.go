// SPDX-License-Identifier: LGPL-2.1-or-later
// Order Fulfillment Workflow - Temporal-style saga (Go WASM)
//
// E-commerce order fulfillment with run/signal/query (Workflow behavior).
// Implements plexspaces.WorkflowActor for workflow_run, workflow_signal:cancel, workflow_query:status.
package main

import (
	"encoding/json"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// OrderStep represents a completed step in the saga.
type OrderStep struct {
	Name          string `json:"name"`
	CompletedAtMs uint64 `json:"completed_at_ms"`
}

// OrderFulfillment implements WorkflowActor for order fulfillment saga.
type OrderFulfillment struct {
	plexspaces.BaseActor

	OrderID         string      `json:"order_id"`
	CustomerID      string      `json:"customer_id"`
	Status          string      `json:"status"` // pending, validated, inventory_reserved, payment_charged, shipped, cancelled, failed
	Steps           []OrderStep `json:"steps"`
	CancelRequested bool        `json:"cancel_requested"`
	TotalComputeMs  float64     `json:"total_compute_ms"`
	TotalCoordMs    float64     `json:"total_coord_ms"`
	CreatedAtMs     uint64      `json:"created_at_ms"`
	UpdatedAtMs     uint64      `json:"updated_at_ms"`
}

func NewOrderFulfillment() *OrderFulfillment {
	a := &OrderFulfillment{
		Status: "pending",
		Steps:  make([]OrderStep, 0),
	}
	a.SetSelf(a)
	return a
}

func (o *OrderFulfillment) Init(configJSON string) string {
	var config struct {
		ActorID    string `json:"actor_id"`
		OrderID    string `json:"order_id"`
		CustomerID string `json:"customer_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	if config.OrderID != "" {
		o.OrderID = config.OrderID
	}
	if config.CustomerID != "" {
		o.CustomerID = config.CustomerID
	}
	now := host.NowMs()
	o.CreatedAtMs = now
	o.UpdatedAtMs = now
	return ""
}

func (o *OrderFulfillment) Run(payloadJSON string) string {
	t0 := host.NowMs()
	var payload map[string]interface{}
	_ = json.Unmarshal([]byte(payloadJSON), &payload)
	if id, ok := payload["order_id"].(string); ok && id != "" {
		o.OrderID = id
	}
	if id, ok := payload["customer_id"].(string); ok && id != "" {
		o.CustomerID = id
	}
	o.UpdatedAtMs = host.NowMs()

	if o.Status != "pending" && o.Status != "validated" {
		return marshal(map[string]interface{}{
			"status":  o.Status,
			"order_id": o.OrderID,
			"message": "Workflow already completed or cancelled",
		})
	}

	computeMs := 0.0
	// Step 1: validate
	if !hasStep(o.Steps, "validate") {
		computeMs += 8
		o.Steps = append(o.Steps, OrderStep{Name: "validate", CompletedAtMs: host.NowMs()})
		o.Status = "validated"
		o.UpdatedAtMs = host.NowMs()
	}
	if o.CancelRequested {
		return o.compensate("cancel_requested")
	}
	// Step 2: reserve inventory
	if !hasStep(o.Steps, "reserve_inventory") {
		computeMs += 12
		o.Steps = append(o.Steps, OrderStep{Name: "reserve_inventory", CompletedAtMs: host.NowMs()})
		o.Status = "inventory_reserved"
		o.UpdatedAtMs = host.NowMs()
	}
	if o.CancelRequested {
		return o.compensate("cancel_requested")
	}
	// Step 3: charge payment
	if !hasStep(o.Steps, "charge_payment") {
		computeMs += 10
		o.Steps = append(o.Steps, OrderStep{Name: "charge_payment", CompletedAtMs: host.NowMs()})
		o.Status = "payment_charged"
		o.UpdatedAtMs = host.NowMs()
	}
	if o.CancelRequested {
		return o.compensate("cancel_requested")
	}
	// Step 4: ship
	if !hasStep(o.Steps, "ship") {
		computeMs += 15
		o.Steps = append(o.Steps, OrderStep{Name: "ship", CompletedAtMs: host.NowMs()})
		o.Status = "shipped"
		o.UpdatedAtMs = host.NowMs()
	}

	totalElapsed := float64(host.NowMs() - t0)
	computeReported := computeMs
	if computeReported > totalElapsed {
		computeReported = totalElapsed
	}
	coordReported := totalElapsed - computeReported
	if coordReported < 0 {
		coordReported = 0
	}
	o.TotalComputeMs += computeReported
	o.TotalCoordMs += coordReported

	return marshal(map[string]interface{}{
		"status":             o.Status,
		"order_id":           o.OrderID,
		"steps_completed":    len(o.Steps),
		"total_compute_ms":   o.TotalComputeMs,
		"total_coord_ms":     o.TotalCoordMs,
	})
}

func (o *OrderFulfillment) Signal(name, _ string) {
	if name == "cancel" {
		o.CancelRequested = true
		o.UpdatedAtMs = host.NowMs()
	}
}

func (o *OrderFulfillment) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]interface{}{
			"order_id":           o.OrderID,
			"customer_id":        o.CustomerID,
			"status":             o.Status,
			"steps_count":        len(o.Steps),
			"cancel_requested":   o.CancelRequested,
			"total_compute_ms":   o.TotalComputeMs,
			"total_coord_ms":     o.TotalCoordMs,
			"created_at_ms":      o.CreatedAtMs,
			"updated_at_ms":      o.UpdatedAtMs,
		})
	}
	return marshal(map[string]interface{}{"error": "unknown_query", "name": name})
}

func (o *OrderFulfillment) Handle(from, msgType, payload string) string {
	return `{"error":"use workflow_run / workflow_signal / workflow_query"}`
}

func (o *OrderFulfillment) compensate(reason string) string {
	o.Status = "cancelled"
	o.UpdatedAtMs = host.NowMs()
	return marshal(map[string]interface{}{
		"status":           "cancelled",
		"order_id":         o.OrderID,
		"reason":           reason,
		"steps_rolled_back": len(o.Steps),
	})
}

func hasStep(steps []OrderStep, name string) bool {
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
	plexspaces.Register(NewOrderFulfillment())
}

func main() {}
