// SPDX-License-Identifier: AGPL-3.0-or-later
// LLMGatewayActor — model abstraction with cost tracking and caching.
//
// Demonstrates: GenServer pattern, KV caching, circuit breaker pattern,
// token usage tracking (feeds ExecutionTraceFacet).
package main

import (
	"encoding/json"
	"fmt"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

const (
	defaultModel      = "llama3.2"
	ollamaBaseURL     = "http://localhost:11434"
)

// LLMGatewayActor routes completion requests to Ollama with KV caching and mock fallback.
type LLMGatewayActor struct {
	plexspaces.BaseActor
	ActorID             string `json:"actor_id"`
	Model               string `json:"model"`
	Provider            string `json:"provider"`
	BaseURL             string `json:"base_url"`
	TotalRequests       int    `json:"total_requests"`
	TotalInputTokens    int    `json:"total_input_tokens"`
	TotalOutputTokens   int    `json:"total_output_tokens"`
	CacheHits           int    `json:"cache_hits"`
	ConsecutiveFailures int    `json:"consecutive_failures"`
	CircuitOpen         bool   `json:"circuit_open"`
}

func NewLLMGatewayActor() plexspaces.Actor {
	a := &LLMGatewayActor{
		Model:    defaultModel,
		Provider: "ollama",
		BaseURL:  ollamaBaseURL,
	}
	a.SetSelf(a)
	return a
}

func (l *LLMGatewayActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	l.SetRuntimeMetadata(config.ActorID)
	l.ActorID = config.ActorID
	if v := config.Args["model"]; v != "" {
		l.Model = v
	}
	if v := config.Args["provider"]; v != "" {
		l.Provider = v
	}
	if v := config.Args["base_url"]; v != "" {
		l.BaseURL = v
	}
	if err := host.PG().Join("svc:llm_gateway"); err != nil {
		host.Warn(fmt.Sprintf("LLMGatewayActor: failed to join svc:llm_gateway: %v", err))
	}
	host.Info(fmt.Sprintf("LLMGatewayActor Init actor_id=%s provider=%s model=%s", config.ActorID, l.Provider, l.Model))
	return ""
}

func (l *LLMGatewayActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "completion":
		return l.completion(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":                "ok",
			"model":                 l.Model,
			"provider":              l.Provider,
			"total_requests":        l.TotalRequests,
			"total_input_tokens":    l.TotalInputTokens,
			"total_output_tokens":   l.TotalOutputTokens,
			"cache_hits":            l.CacheHits,
			"consecutive_failures":  l.ConsecutiveFailures,
			"circuit_open":          l.CircuitOpen,
		})
	case "set_model":
		model := stringVal(p, "model", "")
		if model == "" {
			return marshal(map[string]any{"error": "model is required"})
		}
		l.Model = model
		return marshal(map[string]any{"status": "ok", "model": l.Model})
	case "reset_circuit":
		l.CircuitOpen = false
		l.ConsecutiveFailures = 0
		host.Info("LLMGatewayActor: circuit breaker reset")
		return marshal(map[string]any{"status": "ok", "circuit_open": false})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (l *LLMGatewayActor) completion(p map[string]any) string {
	if l.CircuitOpen {
		return marshal(map[string]any{"error": "circuit_open", "circuit_open": true})
	}
	messages := sliceVal(p, "messages")
	if len(messages) == 0 {
		return marshal(map[string]any{"error": "messages is required"})
	}
	tools := sliceVal(p, "tools")

	// Check deterministic cache
	cacheKey := l.cacheKeyFor(messages, tools)
	if cached := host.KVGet(cacheKey); cached != "" {
		l.CacheHits++
		l.IncrCounter(host, "llm_cache_hits")
		var cachedResp map[string]any
		if err := json.Unmarshal([]byte(cached), &cachedResp); err == nil {
			cachedResp["cached"] = true
			return marshal(cachedResp)
		}
	}

	var result map[string]any
	if l.Provider == "ollama" {
		var err error
		result, err = l.ollamaCompletion(messages, tools)
		if err != nil {
			l.ConsecutiveFailures++
			if l.ConsecutiveFailures >= 3 {
				l.CircuitOpen = true
				host.Warn(fmt.Sprintf("LLMGatewayActor: circuit opened after %d failures", l.ConsecutiveFailures))
			}
			host.Debug(fmt.Sprintf("LLMGatewayActor: ollama failed, falling back to mock: %v", err))
			result = l.mockCompletion(messages, tools)
		} else {
			l.ConsecutiveFailures = 0
		}
	} else {
		result = l.mockCompletion(messages, tools)
	}

	// Inject confidence if not set by model (Ollama doesn't return confidence)
	if _, hasConf := result["confidence"]; !hasConf {
		lastMsg := extractLastUserMessage(messages)
		wc := len(strings.Fields(lastMsg))
		conf := 0.95
		if wc > 30 {
			conf = 0.55
		} else if wc > 15 {
			conf = 0.72
		}
		result["confidence"] = conf
	}

	// Cache successful response
	if resultJSON, err := json.Marshal(result); err == nil {
		host.KVPut(cacheKey, string(resultJSON))
	}

	l.TotalRequests++
	l.TotalInputTokens += intVal(result, "input_tokens", 0)
	l.TotalOutputTokens += intVal(result, "output_tokens", 0)
	l.IncrCounter(host, "llm_completions_total")

	return marshal(result)
}

func (l *LLMGatewayActor) ollamaCompletion(messages, tools []any) (map[string]any, error) {
	body := map[string]any{
		"model":    l.Model,
		"messages": messages,
		"stream":   false,
		"options":  map[string]any{"temperature": 0.7},
	}
	if len(tools) > 0 {
		body["tools"] = tools
	}
	bodyJSON, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	client := plexspaces.NewServiceHTTPClient(host, l.Provider)
	rawResp, err := client.Post("/api/chat", bodyJSON, map[string]string{
		"Content-Type": "application/json",
	})
	if err != nil {
		return nil, fmt.Errorf("http post: %w", err)
	}
	statusCode := intVal(rawResp, "status", 0)
	if statusCode != 200 {
		return nil, fmt.Errorf("ollama returned status %d", statusCode)
	}
	bodyStr, ok := rawResp["body"].(string)
	if !ok {
		return nil, fmt.Errorf("no body in response")
	}
	var data map[string]any
	if err2 := json.Unmarshal([]byte(bodyStr), &data); err2 != nil {
		return nil, fmt.Errorf("parse response: %w", err2)
	}

	message, _ := data["message"].(map[string]any)
	if message == nil {
		message = map[string]any{}
	}
	content := stringVal(message, "content", "")
	toolCalls := extractOllamaToolCalls(message)
	stopReason := "end_turn"
	if len(toolCalls) > 0 {
		stopReason = "tool_use"
	}

	return map[string]any{
		"response": map[string]any{
			"content":     content,
			"stop_reason": stopReason,
			"tool_calls":  toolCalls,
		},
		"input_tokens":  intVal(data, "prompt_eval_count", 0),
		"output_tokens": intVal(data, "eval_count", 0),
		"model":         l.Model,
		"cached":        false,
	}, nil
}

func extractOllamaToolCalls(msg map[string]any) []any {
	raw, ok := msg["tool_calls"].([]any)
	if !ok {
		return []any{}
	}
	calls := make([]any, 0, len(raw))
	for i, tc := range raw {
		m, ok := tc.(map[string]any)
		if !ok {
			continue
		}
		fn, _ := m["function"].(map[string]any)
		if fn == nil {
			fn = map[string]any{}
		}
		name := stringVal(fn, "name", "")
		var inputArgs map[string]any
		if argsRaw, ok := fn["arguments"]; ok {
			switch v := argsRaw.(type) {
			case string:
				_ = json.Unmarshal([]byte(v), &inputArgs)
			case map[string]any:
				inputArgs = v
			}
		}
		if inputArgs == nil {
			inputArgs = map[string]any{}
		}
		calls = append(calls, map[string]any{
			"id":    fmt.Sprintf("tc_%d", i+1),
			"name":  name,
			"input": inputArgs,
		})
	}
	return calls
}

// mockCompletion returns deterministic heuristic-based responses for testing.
func (l *LLMGatewayActor) mockCompletion(messages, tools []any) map[string]any {
	lastUserMsg := extractLastUserMessage(messages)
	lower := strings.ToLower(lastUserMsg)

	// Confidence: short/simple prompts score high; long/complex prompts score low (will escalate)
	wordCount := len(strings.Fields(lastUserMsg))
	confidence := 0.95
	if wordCount > 30 {
		confidence = 0.55 // long prompt → low confidence → advisor escalation
	} else if wordCount > 15 {
		confidence = 0.72
	}

	if containsAny(lower, "search", "find") {
		return map[string]any{
			"response": map[string]any{
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls":  []any{map[string]any{"name": "web_search", "input": map[string]any{"query": truncate(lastUserMsg, 50)}}},
			},
			"confidence":    confidence,
			"input_tokens":  wordCount * 2,
			"output_tokens": 20,
			"model":         "mock",
			"cached":        false,
		}
	} else if containsAny(lower, "calculate", "compute", "+", "-", "*", "/") {
		return map[string]any{
			"response": map[string]any{
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls":  []any{map[string]any{"name": "calculator", "input": map[string]any{"expression": lastUserMsg}}},
			},
			"confidence":    confidence,
			"input_tokens":  wordCount * 2,
			"output_tokens": 15,
			"model":         "mock",
			"cached":        false,
		}
	}
	reply := fmt.Sprintf("I processed your request: %.60s", lastUserMsg)
	return map[string]any{
		"response": map[string]any{
			"content":     reply,
			"stop_reason": "end_turn",
			"tool_calls":  []any{},
		},
		"confidence":    confidence,
		"input_tokens":  wordCount * 2,
		"output_tokens": 25,
		"model":         "mock",
		"cached":        false,
	}
}

func extractLastUserMessage(messages []any) string {
	last := ""
	for _, rawMsg := range messages {
		if msg, ok := rawMsg.(map[string]any); ok {
			if stringVal(msg, "role", "") == "user" {
				last = stringVal(msg, "content", "")
			}
		}
	}
	return last
}

func (l *LLMGatewayActor) cacheKeyFor(messages, tools []any) string {
	content := map[string]any{"messages": messages, "tools": tools, "model": l.Model}
	data, _ := json.Marshal(content)
	h := 0
	for _, b := range data {
		h = h*31 + int(b)
		if h < 0 {
			h = -h
		}
	}
	return fmt.Sprintf("llm_cache:%x", h%0x10000000)
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
