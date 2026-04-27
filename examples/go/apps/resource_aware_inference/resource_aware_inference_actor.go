// SPDX-License-Identifier: AGPL-3.0-or-later
// Resource-Aware Inference (Go WASM)
//
// Demonstrates Resource-Based Affinity (Pattern 4) and Resource-Aware
// Optimization (Pattern 17):
//   - Label-based routing to GPU/CPU inference workers
//   - Cost-aware model selection based on prompt complexity and budget
//   - Per-tenant token budget tracking and enforcement
//
// Actors:
//   - model_registry:      Catalog of available models with tier/cost/resource specs
//   - inference_worker:    Simulated inference with metrics (small/medium/large tiers)
//   - budget_manager:      Per-tenant USD budget tracking and enforcement
//   - routing_workflow:    Orchestrates budget-check → model-select → infer → deduct (Workflow)
//   - inference_event:     Fire-and-forget cost/usage event logger (GenEvent)
//   - budget_fsm:          Per-tenant budget state machine (active/warning/throttled/exhausted) (GenFSM)
package main

import (
	"encoding/json"
	"fmt"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ============================================================================
// Helpers
// ============================================================================

func marshal(v map[string]any) string {
	data, _ := json.Marshal(v)
	return string(data)
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

func stringVal(m map[string]any, key, fallback string) string {
	if v, ok := m[key]; ok {
		if s, ok := v.(string); ok && s != "" {
			return s
		}
	}
	return fallback
}

func intVal(m map[string]any, key string, fallback int) int {
	if v, ok := m[key]; ok {
		switch n := v.(type) {
		case float64:
			return int(n)
		case int:
			return n
		case int64:
			return int(n)
		}
	}
	return fallback
}

func floatVal(m map[string]any, key string, fallback float64) float64 {
	if v, ok := m[key]; ok {
		switch n := v.(type) {
		case float64:
			return n
		case int:
			return float64(n)
		case int64:
			return float64(n)
		}
	}
	return fallback
}

// tsRegisterService registers the current actor as the canonical instance for
// serviceType using a write-once TupleSpace entry. Only the first caller wins;
// subsequent callers (e.g. virtual_actor instances created during the
// re-instantiation window) find the entry and skip the write.
//
// TupleSpace is shared across all actors in the application, unlike KV which
// is scoped per-actor-instance. This makes registration durable against the
// wasmtime#8943 re-instantiation pattern.
func tsRegisterService(serviceType, actorID string) {
	if _, ok := host.TS().Read([]any{"svc", serviceType, nil}); !ok {
		host.TS().Write([]any{"svc", serviceType, actorID})
	}
}

// tsDiscoverService returns the canonical actor ID registered for serviceType,
// or an error if no actor has registered yet.
func tsDiscoverService(serviceType string) (string, error) {
	tup, ok := host.TS().Read([]any{"svc", serviceType, nil})
	if !ok || len(tup) < 3 {
		return "", fmt.Errorf("service %q not found in service registry", serviceType)
	}
	id, _ := tup[2].(string)
	if id == "" {
		return "", fmt.Errorf("service %q has empty actor id", serviceType)
	}
	return id, nil
}

// minInt returns the smaller of two ints.
func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ============================================================================
// ModelSpec — shared model description
// ============================================================================

type ModelSpec struct {
	Name           string  `json:"name"`
	Tier           string  `json:"tier"`
	CostPer1KTokens float64 `json:"cost_per_1k_tokens"`
	RequiresGPU    bool    `json:"requires_gpu"`
	MinMemoryGB    int     `json:"min_memory_gb"`
	AvgLatencyMs   int     `json:"avg_latency_ms"`
}

// ============================================================================
// ModelRegistryActor
// ============================================================================

// ModelRegistryActor maintains a catalog of inference models with resource and
// cost metadata. It selects the most appropriate model for a given request
// based on complexity, budget, and GPU preference.
type ModelRegistryActor struct {
	plexspaces.BaseActor
	ModelCount int `json:"model_count"`
}

func NewModelRegistryActor() plexspaces.Actor {
	a := &ModelRegistryActor{}
	a.SetSelf(a)
	return a
}

func (r *ModelRegistryActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	r.SetRuntimeMetadata(config.ActorID)

	// Seed model catalog in KV
	models := []ModelSpec{
		{
			Name:           "gpt-nano",
			Tier:           "small",
			CostPer1KTokens: 0.001,
			RequiresGPU:    false,
			MinMemoryGB:    2,
			AvgLatencyMs:   10,
		},
		{
			Name:           "gpt-base",
			Tier:           "medium",
			CostPer1KTokens: 0.01,
			RequiresGPU:    false,
			MinMemoryGB:    8,
			AvgLatencyMs:   25,
		},
		{
			Name:           "gpt-large",
			Tier:           "large",
			CostPer1KTokens: 0.05,
			RequiresGPU:    true,
			MinMemoryGB:    32,
			AvgLatencyMs:   80,
		},
	}

	for _, m := range models {
		specJSON, _ := json.Marshal(m)
		if result := host.KVPut("model:"+m.Name, string(specJSON)); result != "" {
			return "ERROR: failed to seed model " + m.Name + ": " + result
		}
	}
	r.ModelCount = len(models)
	// Register in shared service registry so routing_workflow can find this instance.
	tsRegisterService("model_registry", config.ActorID)
	host.Info(fmt.Sprintf("ModelRegistryActor Init actor_id=%s seeded %d models", config.ActorID, len(models)))
	return ""
}

func (r *ModelRegistryActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "register_model":
		return r.registerModel(p)
	case "select_model":
		return r.selectModel(p)
	case "list_models":
		return r.listModels()
	case "get_model":
		return r.getModel(p)
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (r *ModelRegistryActor) registerModel(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "name is required"})
	}
	specJSON, _ := json.Marshal(p)
	if result := host.KVPut("model:"+name, string(specJSON)); result != "" {
		return marshal(map[string]any{"error": "kv_put failed: " + result})
	}
	r.ModelCount++
	return marshal(map[string]any{"status": "ok", "name": name})
}

func (r *ModelRegistryActor) selectModel(p map[string]any) string {
	complexity := floatVal(p, "complexity", 0.5)
	budgetRemaining := floatVal(p, "budget_remaining", 1.0)
	preferGPU, _ := p["prefer_gpu"].(bool)

	// Determine desired tier from complexity
	desiredTier := "medium"
	switch {
	case complexity < 0.3:
		desiredTier = "small"
	case complexity > 0.7:
		if preferGPU {
			desiredTier = "large"
		} else {
			desiredTier = "medium"
		}
	}

	// Tier priority order: try desired tier first, then fall back to cheaper tiers
	tierOrder := tierFallbackOrder(desiredTier)

	for _, tier := range tierOrder {
		spec := r.specForTier(tier)
		if spec == nil {
			continue
		}
		// Estimate cost for 100 tokens (a typical small request)
		estimatedCost := 100.0 * spec.CostPer1KTokens / 1000.0
		if estimatedCost <= budgetRemaining {
			spec.Name = tierToDefaultModel(tier)
			specJSON, _ := json.Marshal(spec)
			var out map[string]any
			_ = json.Unmarshal(specJSON, &out)
			out["status"] = "ok"
			return marshal(out)
		}
	}

	return marshal(map[string]any{"error": "no_model_within_budget", "budget_remaining": budgetRemaining})
}

// tierFallbackOrder returns the ordered list of tiers to try, starting from
// desired and falling back to cheaper options.
func tierFallbackOrder(desired string) []string {
	switch desired {
	case "large":
		return []string{"large", "medium", "small"}
	case "medium":
		return []string{"medium", "small"}
	default:
		return []string{"small"}
	}
}

func tierToDefaultModel(tier string) string {
	switch tier {
	case "large":
		return "gpt-large"
	case "medium":
		return "gpt-base"
	default:
		return "gpt-nano"
	}
}

// specForTier loads the ModelSpec for a given tier from KV.
func (r *ModelRegistryActor) specForTier(tier string) *ModelSpec {
	name := tierToDefaultModel(tier)
	raw := host.KVGet("model:" + name)
	if raw == "" {
		return nil
	}
	var spec ModelSpec
	if err := json.Unmarshal([]byte(raw), &spec); err != nil {
		return nil
	}
	return &spec
}

func (r *ModelRegistryActor) listModels() string {
	keysJSON := host.KVList("model:")
	var keys []string
	_ = json.Unmarshal([]byte(keysJSON), &keys)

	models := make([]any, 0, len(keys))
	for _, key := range keys {
		raw := host.KVGet(key)
		if raw == "" {
			continue
		}
		var spec map[string]any
		if err := json.Unmarshal([]byte(raw), &spec); err == nil {
			models = append(models, spec)
		}
	}
	return marshal(map[string]any{"status": "ok", "models": models, "count": len(models)})
}

func (r *ModelRegistryActor) getModel(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "name is required"})
	}
	raw := host.KVGet("model:" + name)
	if raw == "" {
		return marshal(map[string]any{"error": "model not found", "name": name})
	}
	var spec map[string]any
	if err := json.Unmarshal([]byte(raw), &spec); err != nil {
		return marshal(map[string]any{"error": "invalid model JSON: " + err.Error()})
	}
	spec["status"] = "ok"
	return marshal(spec)
}

// ============================================================================
// InferenceActor
// ============================================================================

// InferenceActor simulates a model inference worker. It is instantiated once
// per model tier (small/medium/large), tracks per-instance metrics, and records
// per-tenant token usage in the shared KV store.
type InferenceActor struct {
	plexspaces.BaseActor
	ModelName       string  `json:"model_name"`
	ModelTier       string  `json:"model_tier"`
	RequestsHandled int     `json:"requests_handled"`
	TotalTokens     int     `json:"total_tokens"`
	TotalCostUSD    float64 `json:"total_cost_usd"`
	GPUCapable      bool    `json:"gpu_capable"`
	MemoryGB        int     `json:"memory_gb"`
}

func NewInferenceActor() plexspaces.Actor {
	a := &InferenceActor{}
	a.SetSelf(a)
	return a
}

func (w *InferenceActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	w.SetRuntimeMetadata(config.ActorID)
	w.ModelName = config.Args["model_name"]
	w.ModelTier = config.Args["model_tier"]
	w.GPUCapable = config.Args["gpu_capable"] == "true"
	if gb := config.Args["memory_gb"]; gb != "" {
		fmt.Sscanf(gb, "%d", &w.MemoryGB)
	}
	// Register in shared service registry so routing_workflow can discover this tier.
	if w.ModelTier != "" {
		tsRegisterService("inference_worker_"+w.ModelTier, config.ActorID)
	}
	host.Info(fmt.Sprintf("InferenceActor Init actor_id=%s model=%s tier=%s gpu=%v mem=%dGB",
		config.ActorID, w.ModelName, w.ModelTier, w.GPUCapable, w.MemoryGB))
	return ""
}

func (w *InferenceActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "infer":
		return w.infer(p)
	case "get_metrics":
		return w.getMetrics()
	case "reset":
		return w.reset()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (w *InferenceActor) infer(p map[string]any) string {
	prompt := stringVal(p, "prompt", "")
	if prompt == "" {
		return marshal(map[string]any{"error": "prompt is required"})
	}
	maxTokens := intVal(p, "max_tokens", 100)
	if maxTokens <= 0 {
		maxTokens = 100
	}
	tenantID := stringVal(p, "tenant_id", "default")

	// Estimate token count: rough heuristic len(prompt)/4 + max_tokens
	tokenCount := len(prompt)/4 + maxTokens

	// Cost lookup by tier
	costPer1K := tierCostPer1K(w.ModelTier)
	cost := float64(tokenCount) * costPer1K / 1000.0

	// Simulate latency via a busy-wait proportional to tier
	latencyTarget := tierLatencyMs(w.ModelTier)
	startMs := host.NowMs()
	for host.NowMs()-startMs < uint64(latencyTarget) {
		// spin to simulate compute work
	}
	latencyMs := int(host.NowMs() - startMs)

	// Update instance counters
	w.RequestsHandled++
	w.TotalTokens += tokenCount
	w.TotalCostUSD += cost

	// Persist per-tenant usage in KV
	usageTokensKey := fmt.Sprintf("usage:%s:tokens", tenantID)
	usageCostKey := fmt.Sprintf("usage:%s:cost", tenantID)

	existingTokens := 0
	if raw := host.KVGet(usageTokensKey); raw != "" {
		fmt.Sscanf(raw, "%d", &existingTokens)
	}
	existingCost := 0.0
	if raw := host.KVGet(usageCostKey); raw != "" {
		fmt.Sscanf(raw, "%f", &existingCost)
	}

	host.KVPut(usageTokensKey, fmt.Sprintf("%d", existingTokens+tokenCount))
	host.KVPut(usageCostKey, fmt.Sprintf("%f", existingCost+cost))

	// Report metrics
	_, _ = host.ApplicationMetricsAdd(w.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"inference_requests": 1,
			"tokens_generated":   tokenCount,
		},
		"latency_totals_ms": map[string]any{
			"inference": latencyMs,
		},
		"latency_max_ms": map[string]any{
			"inference": latencyMs,
		},
		"latency_samples": map[string]any{
			"inference": 1,
		},
	})

	// Truncate prompt preview
	promptPreview := prompt
	if len(promptPreview) > 50 {
		promptPreview = promptPreview[:50]
	}

	return marshal(map[string]any{
		"status":      "ok",
		"result":      "Generated response for: " + promptPreview,
		"tokens_used": tokenCount,
		"cost_usd":    cost,
		"model":       w.ModelName,
		"tier":        w.ModelTier,
		"gpu_used":    w.GPUCapable,
		"latency_ms":  latencyMs,
		"tenant_id":   tenantID,
	})
}

func (w *InferenceActor) getMetrics() string {
	return marshal(map[string]any{
		"status":           "ok",
		"model_name":       w.ModelName,
		"model_tier":       w.ModelTier,
		"requests_handled": w.RequestsHandled,
		"total_tokens":     w.TotalTokens,
		"total_cost_usd":   w.TotalCostUSD,
		"gpu_capable":      w.GPUCapable,
		"memory_gb":        w.MemoryGB,
	})
}

func (w *InferenceActor) reset() string {
	w.RequestsHandled = 0
	w.TotalTokens = 0
	w.TotalCostUSD = 0
	return marshal(map[string]any{"status": "ok", "model": w.ModelName})
}

func tierCostPer1K(tier string) float64 {
	switch tier {
	case "large":
		return 0.05
	case "medium":
		return 0.01
	default:
		return 0.001
	}
}

func tierLatencyMs(tier string) int {
	switch tier {
	case "large":
		return 40
	case "medium":
		return 15
	default:
		return 5
	}
}

// ============================================================================
// BudgetManagerActor
// ============================================================================

// BudgetManagerActor enforces per-tenant USD spending limits. Budgets and
// cumulative usage are stored in the shared KV store so all nodes see a
// consistent view.
type BudgetManagerActor struct {
	plexspaces.BaseActor
	TenantCount int `json:"tenant_count"`
}

func NewBudgetManagerActor() plexspaces.Actor {
	a := &BudgetManagerActor{}
	a.SetSelf(a)
	return a
}

func (b *BudgetManagerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	b.SetRuntimeMetadata(config.ActorID)
	// Register in shared service registry. Write-once: only the first (supervisor-
	// spawned) instance claims the slot; virtual_actor instances created during the
	// re-instantiation window find the entry and skip the write.
	tsRegisterService("budget_manager", config.ActorID)
	host.Info(fmt.Sprintf("BudgetManagerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (b *BudgetManagerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "set_budget":
		return b.setBudget(p)
	case "check_budget":
		return b.checkBudget(p)
	case "deduct":
		return b.deduct(p)
	case "get_report":
		return b.getReport()
	case "reset_tenant":
		return b.resetTenant(p)
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// tsReadBudgetFloat reads a float value from a TupleSpace entry keyed by
// (prefix, tenantID). Returns 0.0 if the entry does not exist.
// Uses TupleSpace instead of per-actor KV so all BudgetManagerActor
// instances (supervisor-spawned and virtual) share the same state.
func (b *BudgetManagerActor) tsReadBudgetFloat(prefix, tenantID string) float64 {
	tup, ok := host.TS().Read([]any{prefix, tenantID, nil})
	if !ok || len(tup) < 3 {
		return 0
	}
	var v float64
	fmt.Sscanf(fmt.Sprint(tup[2]), "%f", &v)
	return v
}

// tsWriteBudgetFloat atomically updates a (prefix, tenantID) TupleSpace entry.
func (b *BudgetManagerActor) tsWriteBudgetFloat(prefix, tenantID string, value float64) {
	host.TS().Take([]any{prefix, tenantID, nil})
	host.TS().Write([]any{prefix, tenantID, fmt.Sprintf("%f", value)})
}

func (b *BudgetManagerActor) setBudget(p map[string]any) string {
	tenantID := stringVal(p, "tenant_id", "")
	if tenantID == "" {
		return marshal(map[string]any{"error": "tenant_id is required"})
	}
	budgetUSD := floatVal(p, "budget_usd", 0)
	// Store in shared TupleSpace so all BudgetManagerActor instances see same data.
	b.tsWriteBudgetFloat("budget", tenantID, budgetUSD)
	b.TenantCount++
	return marshal(map[string]any{"status": "ok", "tenant_id": tenantID, "budget_usd": budgetUSD})
}

func (b *BudgetManagerActor) checkBudget(p map[string]any) string {
	tenantID := stringVal(p, "tenant_id", "")
	if tenantID == "" {
		return marshal(map[string]any{"error": "tenant_id is required"})
	}
	estimatedCost := floatVal(p, "estimated_cost", 0)

	budgetUSD := b.tsReadBudgetFloat("budget", tenantID)
	usedCost := b.tsReadBudgetFloat("usage_cost", tenantID)
	remainingUSD := budgetUSD - usedCost
	allowed := remainingUSD >= estimatedCost

	return marshal(map[string]any{
		"status":        "ok",
		"tenant_id":     tenantID,
		"allowed":       allowed,
		"remaining_usd": remainingUSD,
		"requested_usd": estimatedCost,
		"budget_usd":    budgetUSD,
		"used_usd":      usedCost,
	})
}

func (b *BudgetManagerActor) deduct(p map[string]any) string {
	tenantID := stringVal(p, "tenant_id", "")
	if tenantID == "" {
		return marshal(map[string]any{"error": "tenant_id is required"})
	}
	cost := floatVal(p, "cost", 0)

	budgetUSD := b.tsReadBudgetFloat("budget", tenantID)
	usedCost := b.tsReadBudgetFloat("usage_cost", tenantID) + cost
	b.tsWriteBudgetFloat("usage_cost", tenantID, usedCost)

	remainingUSD := budgetUSD - usedCost
	return marshal(map[string]any{
		"status":        "ok",
		"tenant_id":     tenantID,
		"deducted_usd":  cost,
		"remaining_usd": remainingUSD,
	})
}

func (b *BudgetManagerActor) getReport() string {
	// ReadAll "budget" entries from shared TupleSpace.
	budgetTuples := host.TS().ReadAll([]any{"budget", nil, nil})

	report := make([]any, 0, len(budgetTuples))
	for _, tup := range budgetTuples {
		if len(tup) < 3 {
			continue
		}
		tenantID, _ := tup[1].(string)
		if tenantID == "" {
			continue
		}
		budgetUSD := b.tsReadBudgetFloat("budget", tenantID)
		usedCost := b.tsReadBudgetFloat("usage_cost", tenantID)
		usedTokens := 0
		if tokenTup, ok := host.TS().Read([]any{"usage_tokens", tenantID, nil}); ok && len(tokenTup) >= 3 {
			fmt.Sscanf(fmt.Sprint(tokenTup[2]), "%d", &usedTokens)
		}

		report = append(report, map[string]any{
			"tenant_id":     tenantID,
			"budget_usd":    budgetUSD,
			"used_usd":      usedCost,
			"remaining_usd": budgetUSD - usedCost,
			"tokens_used":   usedTokens,
		})
	}
	return marshal(map[string]any{"status": "ok", "report": report, "tenant_count": len(report)})
}

func (b *BudgetManagerActor) resetTenant(p map[string]any) string {
	tenantID := stringVal(p, "tenant_id", "")
	if tenantID == "" {
		return marshal(map[string]any{"error": "tenant_id is required"})
	}
	b.tsWriteBudgetFloat("usage_cost", tenantID, 0)
	b.tsWriteBudgetFloat("usage_tokens", tenantID, 0)
	return marshal(map[string]any{"status": "ok", "tenant_id": tenantID})
}

// ============================================================================
// RoutingWorkflow (WorkflowActor)
// ============================================================================

// RoutingWorkflow orchestrates the full resource-aware inference pipeline:
//  1. Check tenant budget via BudgetManagerActor
//  2. Estimate prompt complexity
//  3. Select model tier via ModelRegistryActor
//  4. Route request to the appropriate InferenceActor
//  5. Deduct cost from tenant budget
type RoutingWorkflow struct {
	plexspaces.BaseActor
	Status        string  `json:"status"`
	RequestID     string  `json:"request_id"`
	SelectedModel string  `json:"selected_model"`
	TotalCost     float64 `json:"total_cost"`
}

func NewRoutingWorkflow() plexspaces.Actor {
	a := &RoutingWorkflow{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (rw *RoutingWorkflow) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	_ = json.Unmarshal([]byte(configJSON), &config)
	rw.SetRuntimeMetadata(config.ActorID)
	rw.Status = "idle"
	host.Info(fmt.Sprintf("RoutingWorkflow Init actor_id=%s", config.ActorID))
	return ""
}

func (rw *RoutingWorkflow) Handle(from, msgType, payload string) string {
	return `{"error":"use workflow_run / workflow_signal / workflow_query for routing_workflow"}`
}

func (rw *RoutingWorkflow) Run(payloadJSON string) string {
	p := parsePayload(payloadJSON)
	prompt := stringVal(p, "prompt", "")
	if prompt == "" {
		return marshal(map[string]any{"error": "prompt is required"})
	}
	tenantID := stringVal(p, "tenant_id", "default")
	preferGPU, _ := p["prefer_gpu"].(bool)
	maxBudgetUSD := floatVal(p, "max_budget_usd", 1.0)
	requestID := fmt.Sprintf("req-%d", host.NowMs())

	rw.Status = "running"
	rw.RequestID = requestID

	// Discover canonical IDs via shared TupleSpace service registry.
	// Supervisor-spawned actors register write-once in Init; this ensures we
	// always reach the instance whose KV/state was set up at deploy time.
	budgetManagerID, err := tsDiscoverService("budget_manager")
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{"error": "budget_manager not found: " + err.Error(), "request_id": requestID})
	}
	modelRegistryID, err := tsDiscoverService("model_registry")
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{"error": "model_registry not found: " + err.Error(), "request_id": requestID})
	}

	// Step 1: Estimate cost for the request and check budget
	complexity := promptComplexity(prompt)
	// Use a conservative cost estimate (medium tier, 200 tokens) for the budget check
	estimatedCost := 200.0 * tierCostPer1K("medium") / 1000.0

	budgetResp, err := host.Ask(budgetManagerID, "check_budget", map[string]any{
		"tenant_id":      tenantID,
		"estimated_cost": estimatedCost,
	}, 10000)
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{
			"error":      "budget_check_failed: " + err.Error(),
			"tenant_id":  tenantID,
			"request_id": requestID,
		})
	}

	budgetMap, _ := budgetResp.(map[string]any)
	allowed, _ := budgetMap["allowed"].(bool)
	remainingUSD := floatVal(budgetMap, "remaining_usd", 0)

	if !allowed {
		rw.Status = "rejected"
		return marshal(map[string]any{
			"error":         "budget_exceeded",
			"tenant_id":     tenantID,
			"request_id":    requestID,
			"remaining_usd": remainingUSD,
			"max_budget_usd": maxBudgetUSD,
		})
	}

	// Step 2: Select model based on complexity, remaining budget and GPU preference
	modelResp, err := host.Ask(modelRegistryID, "select_model", map[string]any{
		"complexity":       complexity,
		"budget_remaining": remainingUSD,
		"prefer_gpu":       preferGPU,
	}, 10000)
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{
			"error":      "model_selection_failed: " + err.Error(),
			"tenant_id":  tenantID,
			"request_id": requestID,
		})
	}

	modelMap, _ := modelResp.(map[string]any)
	selectedModel := stringVal(modelMap, "name", "gpt-nano")
	selectedTier := stringVal(modelMap, "tier", "small")
	rw.SelectedModel = selectedModel

	// Step 3: Route to the appropriate inference worker (TS discovery by tier)
	workerRole := tierToWorkerRole(selectedTier)
	workerID, err := tsDiscoverService(workerRole)
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{
			"error":      "inference_worker not found: " + err.Error(),
			"tenant_id":  tenantID,
			"request_id": requestID,
			"model":      selectedModel,
		})
	}

	inferResp, err := host.Ask(workerID, "infer", map[string]any{
		"prompt":     prompt,
		"max_tokens": 100,
		"tenant_id":  tenantID,
	}, 30000)
	if err != nil {
		rw.Status = "failed"
		return marshal(map[string]any{
			"error":      "inference_failed: " + err.Error(),
			"tenant_id":  tenantID,
			"request_id": requestID,
			"model":      selectedModel,
		})
	}

	inferMap, _ := inferResp.(map[string]any)
	actualCost := floatVal(inferMap, "cost_usd", estimatedCost)
	rw.TotalCost += actualCost

	// Step 4: Deduct actual cost from budget
	_, _ = host.Ask(budgetManagerID, "deduct", map[string]any{
		"tenant_id": tenantID,
		"cost":      actualCost,
	}, 10000)

	rw.Status = "completed"

	_, _ = host.ApplicationMetricsAdd(rw.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"routing_workflow_runs":      1,
			"routing_workflow_completed": 1,
		},
	})

	return marshal(map[string]any{
		"status":         "ok",
		"request_id":     requestID,
		"tenant_id":      tenantID,
		"model_selected": selectedModel,
		"model_tier":     selectedTier,
		"complexity":     complexity,
		"result":         inferMap,
		"actual_cost_usd": actualCost,
		"remaining_usd":  remainingUSD - actualCost,
	})
}

func (rw *RoutingWorkflow) Signal(name, payloadJSON string) {
	switch name {
	case "update_budget":
		// Budget updates are handled directly by BudgetManagerActor;
		// signal is here for external callers to trigger a re-check.
		host.Info(fmt.Sprintf("RoutingWorkflow Signal update_budget request_id=%s", rw.RequestID))
	}
}

func (rw *RoutingWorkflow) Query(name, _ string) string {
	switch name {
	case "cost_report":
		budgetManagerID, _ := tsDiscoverService("budget_manager")
		resp, err := host.Ask(budgetManagerID, "get_report", map[string]any{}, 10000)
		if err != nil {
			return marshal(map[string]any{"error": "cost_report failed: " + err.Error()})
		}
		respMap, _ := resp.(map[string]any)
		respMap["workflow_total_cost_usd"] = rw.TotalCost
		return marshal(respMap)
	default:
		return marshal(map[string]any{"error": "unknown_query", "name": name})
	}
}

// promptComplexity estimates a 0.0-1.0 complexity score from prompt length.
func promptComplexity(prompt string) float64 {
	l := len(prompt)
	switch {
	case l < 50:
		return 0.2
	case l < 200:
		return 0.5
	default:
		return 0.8
	}
}

// tierToWorkerRole maps a model tier to the actor role name for that worker.
func tierToWorkerRole(tier string) string {
	switch tier {
	case "large":
		return "inference_worker_large"
	case "medium":
		return "inference_worker_medium"
	default:
		return "inference_worker_small"
	}
}

// ============================================================================
// InferenceEventActor (GenEvent — cost/usage events)
// ============================================================================

// InferenceEventActor receives fire-and-forget inference completion events
// carrying cost and tier information. Declared with behavior_kind = "GenEvent".
type InferenceEventActor struct {
	plexspaces.BaseActor
	TotalEventsEmitted int     `json:"total_events_emitted"`
	TotalCostTracked   float64 `json:"total_cost_tracked"`
}

func NewInferenceEventActor() plexspaces.Actor {
	a := &InferenceEventActor{}
	a.SetSelf(a)
	return a
}

func (e *InferenceEventActor) Init(configJSON string) string {
	var cfg struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	e.SetRuntimeMetadata(cfg.ActorID)
	_ = host.PG().Join("inference-events")
	return ""
}

func (e *InferenceEventActor) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch stringVal(payload, "op", "") {
	case "inference_completed":
		e.TotalEventsEmitted++
		var cost float64
		fmt.Sscanf(fmt.Sprintf("%v", payload["cost_usd"]), "%f", &cost)
		e.TotalCostTracked += cost
		tenant := stringVal(payload, "tenant_id", "default")
		tier := stringVal(payload, "tier", "unknown")
		_, _ = host.ApplicationMetricsAdd(e.ApplicationID(), map[string]any{
			"message_count": 1,
			"counter_metrics": map[string]any{
				"inference_events":             1,
				"tier_" + tier + "_calls":      1,
				"tenant_" + tenant + "_calls":  1,
			},
		})
		return marshal(map[string]any{"ok": true})
	case "get_stats":
		return marshal(map[string]any{
			"events_emitted": e.TotalEventsEmitted,
			"total_cost_usd": e.TotalCostTracked,
		})
	}
	return marshal(map[string]any{"ok": true})
}

// ============================================================================
// BudgetFSM (GenFSM — per-tenant budget state machine)
// ============================================================================

// BudgetFSM tracks a per-tenant budget as a state machine.
// States: active | warning | throttled | exhausted
// Declared with behavior_kind = "GenFSM" in app-config.toml.
type BudgetFSM struct {
	plexspaces.BaseActor
	FSMState string  `json:"fsm_state"`
	TenantID string  `json:"tenant_id"`
	SpentUSD float64 `json:"spent_usd"`
	LimitUSD float64 `json:"limit_usd"`
}

func NewBudgetFSM() plexspaces.Actor {
	a := &BudgetFSM{FSMState: "active", LimitUSD: 10.0}
	a.SetSelf(a)
	return a
}

func (f *BudgetFSM) Init(configJSON string) string {
	var cfg struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &cfg); err != nil {
		return "ERROR: " + err.Error()
	}
	f.SetRuntimeMetadata(cfg.ActorID)
	f.TenantID = cfg.Args["tenant_id"]
	if v := cfg.Args["limit_usd"]; v != "" {
		fmt.Sscanf(v, "%f", &f.LimitUSD)
	}
	return ""
}

func (f *BudgetFSM) Handle(fromActor, msgType, payloadJSON string) string {
	payload := parsePayload(payloadJSON)
	switch stringVal(payload, "op", "") {
	case "deduct":
		var amount float64
		fmt.Sscanf(fmt.Sprintf("%v", payload["amount"]), "%f", &amount)
		f.SpentUSD += amount
		pct := f.SpentUSD / f.LimitUSD
		switch {
		case pct >= 1.0:
			f.FSMState = "exhausted"
		case pct >= 0.8:
			f.FSMState = "throttled"
		case pct >= 0.6:
			f.FSMState = "warning"
		default:
			f.FSMState = "active"
		}
		return marshal(map[string]any{"state": f.FSMState, "spent": f.SpentUSD, "limit": f.LimitUSD})
	case "reset":
		f.SpentUSD = 0
		f.FSMState = "active"
		return marshal(map[string]any{"state": f.FSMState})
	case "set_limit":
		fmt.Sscanf(fmt.Sprintf("%v", payload["limit"]), "%f", &f.LimitUSD)
		return marshal(map[string]any{"limit": f.LimitUSD})
	case "get_state":
		return marshal(map[string]any{
			"state": f.FSMState, "tenant": f.TenantID,
			"spent": f.SpentUSD, "limit": f.LimitUSD,
			"pct_used": f.SpentUSD / f.LimitUSD * 100,
		})
	case "is_allowed":
		return marshal(map[string]any{"allowed": f.FSMState != "exhausted", "state": f.FSMState})
	}
	return marshal(map[string]any{"state": f.FSMState})
}

// ============================================================================
// Registration
// ============================================================================

func init() {
	router := plexspaces.NewActorRouter()
	router.Route("model_registry", NewModelRegistryActor)
	router.Route("inference_worker", NewInferenceActor)
	router.Route("budget_manager", NewBudgetManagerActor)
	router.Route("routing_workflow", NewRoutingWorkflow)
	router.Route("inference_event", NewInferenceEventActor)
	router.Route("budget_fsm", NewBudgetFSM)
	plexspaces.Register(router)
}

func main() {}
