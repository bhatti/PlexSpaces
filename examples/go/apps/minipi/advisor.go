// SPDX-License-Identifier: AGPL-3.0-or-later
// AdvisorActor — two-tier LLM pattern: fast executor + expensive advisor on-demand.
//
// Demonstrates the "Advisor" strategy (Anthropic 2026):
// - Executor (cheap model, every turn) handles most decisions
// - Advisor (expensive model) is consulted only when executor confidence is low
// - Token cost split: executor tokens vs advisor tokens (trackable per eval run)
// - Escalation rate feeds into BenchmarkActor for cost/quality tradeoff analysis
//
// Config knob: confidence_threshold (0.0–1.0). Lower = more advisor calls.
// BenchmarkActor can sweep this: same scenarios, 3 thresholds → cost/quality table.
package main

import (
	"encoding/json"
	"fmt"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	advisorDefaultThreshold   = 0.8
	advisorFastModel          = "llama3.2"
	advisorExpensiveModel     = "llama3.3:70b"
	advisorEscalationPrompt   = "You are an expert advisor. The primary agent was not confident about this decision. Review the task and provide a better answer."
)

// AdvisorActor routes LLM calls to cheap or expensive models based on confidence.
// Tracks escalation_rate and token split for eval/benchmark analysis.
type AdvisorActor struct {
	plexspaces.BaseActor
	ActorID             string  `json:"actor_id"`
	ConfidenceThreshold float64 `json:"confidence_threshold"`
	TotalRequests       int     `json:"total_requests"`
	EscalationCount     int     `json:"escalation_count"`
	FastInputTokens     int     `json:"fast_input_tokens"`
	FastOutputTokens    int     `json:"fast_output_tokens"`
	AdvisorInputTokens  int     `json:"advisor_input_tokens"`
	AdvisorOutputTokens int     `json:"advisor_output_tokens"`
}

func NewAdvisorActor() plexspaces.Actor {
	a := &AdvisorActor{
		ConfidenceThreshold: advisorDefaultThreshold,
	}
	a.SetSelf(a)
	return a
}

func (a *AdvisorActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	a.ActorID = config.ActorID
	if v := config.Args["confidence_threshold"]; v != "" {
		var t float64
		if _, err := fmt.Sscanf(v, "%f", &t); err == nil && t >= 0 && t <= 1 {
			a.ConfidenceThreshold = t
		}
	}
	if err := host.PG().Join("svc:advisor"); err != nil {
		host.Warn(fmt.Sprintf("AdvisorActor: failed to join svc:advisor: %v", err))
	}
	host.Info(fmt.Sprintf("AdvisorActor Init actor_id=%s threshold=%.2f", config.ActorID, a.ConfidenceThreshold))
	return ""
}

func (a *AdvisorActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "advise":
		return a.advise(p)
	case "get_stats":
		return a.stats()
	case "reset_stats":
		a.TotalRequests = 0
		a.EscalationCount = 0
		a.FastInputTokens = 0
		a.FastOutputTokens = 0
		a.AdvisorInputTokens = 0
		a.AdvisorOutputTokens = 0
		return marshal(map[string]any{"status": "ok"})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// advise runs the two-tier pattern:
// 1. Send to fast model (executor)
// 2. If confidence >= threshold: return fast result
// 3. If confidence < threshold: escalate to advisor model, merge responses
func (a *AdvisorActor) advise(p map[string]any) string {
	messages, _ := p["messages"].([]any)
	// Accept prompt/context shorthand and build messages array
	if len(messages) == 0 {
		prompt := stringVal(p, "prompt", "")
		if prompt == "" {
			return marshal(map[string]any{"error": "messages or prompt is required"})
		}
		ctx := stringVal(p, "context", "")
		systemContent := "You are a helpful assistant."
		if ctx != "" {
			systemContent = fmt.Sprintf("You are a helpful assistant. Context: %s", ctx)
		}
		messages = []any{
			map[string]any{"role": "system", "content": systemContent},
			map[string]any{"role": "user", "content": prompt},
		}
	}

	a.TotalRequests++
	llmID, err := registryFirst("llm_gateway", "svc:llm_gateway")
	if err != nil || llmID == "" {
		return marshal(map[string]any{"error": "llm_gateway unavailable"})
	}

	// ── Step 1: Fast executor ──────────────────────────────────────────────
	fastResp, fastErr := askActor(llmID, "completion", map[string]any{
		"op":       "completion",
		"model":    advisorFastModel,
		"messages": messages,
	}, 15000)
	if fastErr != nil {
		return marshal(map[string]any{"error": "fast_model_failed", "detail": fastErr.Error()})
	}

	a.FastInputTokens += intVal(fastResp, "input_tokens", 0)
	a.FastOutputTokens += intVal(fastResp, "output_tokens", 0)

	confidence := float64Val(fastResp, "confidence", 1.0)
	response, _ := fastResp["response"].(map[string]any)
	if response == nil {
		response = map[string]any{}
	}

	if confidence >= a.ConfidenceThreshold {
		a.IncrCounter(host, "advisor_fast_path")
		return marshal(map[string]any{
			"status":           "ok",
			"tier":             "fast",
			"confidence":       confidence,
			"response":         response,
			"input_tokens":     a.FastInputTokens,
			"output_tokens":    a.FastOutputTokens,
			"escalation_rate":  a.escalationRate(),
		})
	}

	// ── Step 2: Escalate to advisor model ─────────────────────────────────
	a.EscalationCount++
	a.IncrCounter(host, "advisor_escalations")
	host.Info(fmt.Sprintf("AdvisorActor: escalating (confidence=%.2f < threshold=%.2f) total_escalations=%d",
		confidence, a.ConfidenceThreshold, a.EscalationCount))

	// Build advisor messages: include original context + fast model's tentative answer
	fastContent := stringVal(response, "content", "")
	advisorMessages := append(messages, map[string]any{
		"role":    "assistant",
		"content": fmt.Sprintf("[Tentative answer, low confidence %.2f]: %s", confidence, fastContent),
	}, map[string]any{
		"role":    "user",
		"content": advisorEscalationPrompt,
	})

	advisorResp, advisorErr := askActor(llmID, "completion", map[string]any{
		"op":       "completion",
		"model":    advisorExpensiveModel,
		"messages": advisorMessages,
	}, 30000)

	if advisorErr != nil {
		// Advisor unavailable — fall back to fast result rather than error
		host.Warn(fmt.Sprintf("AdvisorActor: advisor model failed, using fast result: %v", advisorErr))
		return marshal(map[string]any{
			"status":          "ok",
			"tier":            "fast_fallback",
			"confidence":      confidence,
			"response":        response,
			"escalation_rate": a.escalationRate(),
		})
	}

	a.AdvisorInputTokens += intVal(advisorResp, "input_tokens", 0)
	a.AdvisorOutputTokens += intVal(advisorResp, "output_tokens", 0)

	advisorResponse, _ := advisorResp["response"].(map[string]any)
	if advisorResponse == nil {
		advisorResponse = map[string]any{}
	}

	return marshal(map[string]any{
		"status":                  "ok",
		"tier":                    "advisor",
		"confidence":              confidence,
		"response":                advisorResponse,
		"fast_response":           response,
		"escalation_rate":         a.escalationRate(),
		"total_input_tokens":      a.FastInputTokens + a.AdvisorInputTokens,
		"total_output_tokens":     a.FastOutputTokens + a.AdvisorOutputTokens,
		"fast_input_tokens":       a.FastInputTokens,
		"advisor_input_tokens":    a.AdvisorInputTokens,
	})
}

func (a *AdvisorActor) escalationRate() float64 {
	if a.TotalRequests == 0 {
		return 0.0
	}
	return roundFloat(float64(a.EscalationCount)/float64(a.TotalRequests)*100, 1)
}

func (a *AdvisorActor) stats() string {
	totalIn := a.FastInputTokens + a.AdvisorInputTokens
	totalOut := a.FastOutputTokens + a.AdvisorOutputTokens
	advisorShare := 0.0
	if totalIn > 0 {
		advisorShare = roundFloat(float64(a.AdvisorInputTokens)/float64(totalIn)*100, 1)
	}
	return marshal(map[string]any{
		"status":               "ok",
		"actor_id":             a.ActorID,
		"confidence_threshold": a.ConfidenceThreshold,
		"total_requests":       a.TotalRequests,
		"escalation_count":     a.EscalationCount,
		"escalation_rate_pct":  a.escalationRate(),
		"fast_input_tokens":    a.FastInputTokens,
		"fast_output_tokens":   a.FastOutputTokens,
		"advisor_input_tokens": a.AdvisorInputTokens,
		"advisor_output_tokens": a.AdvisorOutputTokens,
		"total_input_tokens":   totalIn,
		"total_output_tokens":  totalOut,
		"advisor_token_share_pct": advisorShare,
	})
}
