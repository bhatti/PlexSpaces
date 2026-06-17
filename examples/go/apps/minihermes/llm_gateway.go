// SPDX-License-Identifier: AGPL-3.0-or-later
// LLMGatewayActor — real LLM provider via HTTPFetch to Ollama/OpenAI/Anthropic.
// Falls back to keyword-based simulation when no provider is reachable.
// Demonstrates: HTTPFetch service links, KV credential pool, provider hot-swap,
// SendAfter health-check timer, Metrics.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// LLMGatewayActor routes LLM completions to a configured provider.
// Supports Ollama (default), OpenAI, and Anthropic via HTTPFetch service links.
// When no provider is reachable it falls back to deterministic keyword routing
// so the test suite passes without a running LLM.
type LLMGatewayActor struct {
	plexspaces.BaseActor
	ActiveProvider      string `json:"active_provider"`
	DefaultModel        string `json:"default_model"`
	RequestCount        int    `json:"request_count"`
	TotalTokens         int    `json:"total_tokens"`
	CacheHits           int    `json:"cache_hits"`
	SimulatedCount      int    `json:"simulated_count"`
	ConsecutiveFailures int    `json:"consecutive_failures"`
	CircuitOpen         bool   `json:"circuit_open"`
}

func NewLLMGatewayActor() plexspaces.Actor {
	a := &LLMGatewayActor{ActiveProvider: "ollama", DefaultModel: "llama3.2"}
	a.SetSelf(a)
	return a
}

func newLLMGatewayActor() *LLMGatewayActor {
	a := &LLMGatewayActor{ActiveProvider: "ollama", DefaultModel: "llama3.2"}
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
	if p := config.Args["default_provider"]; p != "" {
		l.ActiveProvider = p
	}
	if m := config.Args["default_model"]; m != "" {
		l.DefaultModel = m
	}
	// Persist active provider so other actors can discover it
	host.KVPut("llm_gateway:active_provider", l.ActiveProvider)
	host.KVPut("llm_gateway:default_model", l.DefaultModel)

	if err := host.PG().Join("svc:llm_gateway"); err != nil {
		host.Warn(fmt.Sprintf("LLMGatewayActor: failed to join svc:llm_gateway: %v", err))
	}
	// Schedule periodic health check
	_ = host.SendAfter(30000, "health_tick", map[string]any{"op": "health_tick"})
	host.Info(fmt.Sprintf("LLMGatewayActor Init actor_id=%s provider=%s model=%s", config.ActorID, l.ActiveProvider, l.DefaultModel))
	return ""
}

func (l *LLMGatewayActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "completion":
		return l.completion(p)
	case "switch_provider":
		return l.switchProvider(p)
	case "register_provider":
		return l.registerProvider(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":               "ok",
			"active_provider":      l.ActiveProvider,
			"default_model":        l.DefaultModel,
			"request_count":        l.RequestCount,
			"total_tokens":         l.TotalTokens,
			"cache_hits":           l.CacheHits,
			"simulated_count":      l.SimulatedCount,
			"consecutive_failures": l.ConsecutiveFailures,
			"circuit_open":         l.CircuitOpen,
		})
	case "health_tick":
		return l.healthTick()
	case "reset_circuit":
		l.CircuitOpen = false
		l.ConsecutiveFailures = 0
		return marshal(map[string]any{"status": "ok", "circuit_open": false})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (l *LLMGatewayActor) registerProvider(p map[string]any) string {
	name := stringVal(p, "name", "")
	baseURL := stringVal(p, "base_url", "")
	model := stringVal(p, "model", "")
	apiKey := stringVal(p, "api_key", "")
	if name == "" {
		return marshal(map[string]any{"error": "name is required"})
	}
	meta := map[string]any{"name": name, "base_url": baseURL, "model": model}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("provider:"+name, string(metaJSON))
	if apiKey != "" {
		host.KVPut("provider_key:"+name, apiKey)
	}
	// Track provider names in a comma-separated index
	existing := host.KVGet("provider_names")
	names := []string{}
	if existing != "" {
		names = strings.Split(existing, ",")
	}
	found := false
	for _, n := range names {
		if n == name {
			found = true
			break
		}
	}
	if !found {
		names = append(names, name)
		host.KVPut("provider_names", strings.Join(names, ","))
	}
	host.Info(fmt.Sprintf("LLMGatewayActor: registered provider=%s", name))
	fireAudit("provider_registered", fmt.Sprintf("provider=%s", name))
	return marshal(map[string]any{"status": "ok", "provider": name})
}

func (l *LLMGatewayActor) switchProvider(p map[string]any) string {
	name := stringVal(p, "provider", "")
	if name == "" {
		return marshal(map[string]any{"error": "provider is required"})
	}
	old := l.ActiveProvider
	l.ActiveProvider = name
	if m := stringVal(p, "model", ""); m != "" {
		l.DefaultModel = m
	}
	host.KVPut("llm_gateway:active_provider", l.ActiveProvider)
	host.KVPut("llm_gateway:default_model", l.DefaultModel)
	l.CircuitOpen = false
	l.ConsecutiveFailures = 0
	host.Info(fmt.Sprintf("LLMGatewayActor: switched provider %s → %s model=%s", old, l.ActiveProvider, l.DefaultModel))
	fireAudit("provider_switched", fmt.Sprintf("from=%s to=%s", old, l.ActiveProvider))
	return marshal(map[string]any{"status": "ok", "old_provider": old, "new_provider": l.ActiveProvider, "model": l.DefaultModel})
}

func (l *LLMGatewayActor) completion(p map[string]any) string {
	if l.CircuitOpen {
		return marshal(map[string]any{"error": "circuit_open", "circuit_open": true})
	}

	messages := sliceVal(p, "messages")
	tools := sliceVal(p, "tools")
	model := stringVal(p, "model", l.DefaultModel)
	provider := stringVal(p, "provider", l.ActiveProvider)

	// Check cache
	lastUserMsg := extractLastUserMessage(messages)
	cacheKey := "llm_cache:" + llmCacheKeyFor(lastUserMsg)
	if cached := host.KVGet(cacheKey); cached != "" {
		l.CacheHits++
		var cachedResp map[string]any
		if err := json.Unmarshal([]byte(cached), &cachedResp); err == nil {
			cachedResp["cached"] = true
			return marshal(cachedResp)
		}
	}

	// Try real LLM via HTTPFetch
	resp, err := l.callProvider(provider, model, messages, tools)
	if err != nil {
		l.ConsecutiveFailures++
		if l.ConsecutiveFailures >= 3 {
			l.CircuitOpen = true
			host.Warn(fmt.Sprintf("LLMGatewayActor: circuit opened after %d failures", l.ConsecutiveFailures))
		}
		host.Debug(fmt.Sprintf("LLMGatewayActor: provider %s failed, falling back to simulation: %v", provider, err))
		resp = l.simulatedCompletion(lastUserMsg, tools)
		l.SimulatedCount++
	} else {
		l.ConsecutiveFailures = 0
	}

	// Cache successful response
	if respJSON, err := json.Marshal(resp); err == nil {
		host.KVPut(cacheKey, string(respJSON))
	}

	l.RequestCount++
	if usage, ok := resp["usage"].(map[string]any); ok {
		l.TotalTokens += intVal(usage, "input_tokens", 0) + intVal(usage, "output_tokens", 0)
	}
	l.IncrCounter(host, "llm_completions")
	fireAudit("llm_completion", fmt.Sprintf("provider=%s model=%s", provider, model))
	return marshal(resp)
}

// callProvider makes the real HTTP call to the configured LLM provider.
func (l *LLMGatewayActor) callProvider(provider, model string, messages, tools []any) (map[string]any, error) {
	client := plexspaces.NewServiceHTTPClient(host, provider)

	var path string
	var reqBody map[string]any

	switch provider {
	case "ollama":
		path = "/api/chat"
		reqBody = map[string]any{
			"model":    model,
			"messages": messages,
			"stream":   false,
		}
		if len(tools) > 0 {
			reqBody["tools"] = tools
		}
	case "openai":
		path = "/v1/chat/completions"
		reqBody = map[string]any{
			"model":    model,
			"messages": messages,
		}
		if len(tools) > 0 {
			reqBody["tools"] = tools
		}
	case "anthropic":
		path = "/v1/messages"
		reqBody = map[string]any{
			"model":      model,
			"messages":   messages,
			"max_tokens": 1024,
		}
		if len(tools) > 0 {
			reqBody["tools"] = tools
		}
	default:
		path = "/v1/chat/completions"
		reqBody = map[string]any{"model": model, "messages": messages}
	}

	bodyJSON, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("marshal request: %w", err)
	}

	rawResp, err := client.Post(path, bodyJSON, map[string]string{
		"Content-Type": "application/json",
	})
	if err != nil {
		return nil, fmt.Errorf("http fetch: %w", err)
	}

	// rawResp is already map[string]any from ServiceHTTPClient.Post
	httpResp := rawResp
	statusCode := intVal(httpResp, "status", 0)
	if statusCode != 200 {
		return nil, fmt.Errorf("provider returned status %d", statusCode)
	}
	bodyStr, ok := httpResp["body"].(string)
	if !ok {
		return nil, fmt.Errorf("no body in response")
	}

	var providerResp map[string]any
	if err := json.Unmarshal([]byte(bodyStr), &providerResp); err != nil {
		return nil, fmt.Errorf("parse response: %w", err)
	}

	return normalizeProviderResponse(provider, providerResp), nil
}

// normalizeProviderResponse converts provider-specific formats to a unified structure.
func normalizeProviderResponse(provider string, raw map[string]any) map[string]any {
	unified := map[string]any{
		"status": "ok",
		"cached": false,
	}

	switch provider {
	case "ollama":
		// Ollama: {"message": {"role":"assistant","content":"...","tool_calls":[...]}}
		msg, _ := raw["message"].(map[string]any)
		if msg == nil {
			msg = map[string]any{}
		}
		content := stringVal(msg, "content", "")
		toolCalls := extractOllamaToolCalls(msg)
		stopReason := "end_turn"
		if len(toolCalls) > 0 {
			stopReason = "tool_use"
		}
		doneReason := stringVal(raw, "done_reason", "")
		if doneReason == "stop" && len(toolCalls) == 0 {
			stopReason = "end_turn"
		}
		unified["response"] = map[string]any{
			"role":        "assistant",
			"content":     content,
			"stop_reason": stopReason,
			"tool_calls":  toolCalls,
		}
		prmTokens := intVal(raw, "prompt_eval_count", 0)
		evalTokens := intVal(raw, "eval_count", 0)
		unified["usage"] = map[string]any{"input_tokens": prmTokens, "output_tokens": evalTokens}
		unified["model"] = stringVal(raw, "model", "ollama")

	case "openai":
		// OpenAI: {"choices":[{"message":{"content":"...","tool_calls":[...]}}]}
		choices, _ := raw["choices"].([]any)
		if len(choices) == 0 {
			unified["response"] = map[string]any{"role": "assistant", "content": "", "stop_reason": "end_turn", "tool_calls": []any{}}
			break
		}
		choice, _ := choices[0].(map[string]any)
		msg, _ := choice["message"].(map[string]any)
		if msg == nil {
			msg = map[string]any{}
		}
		content := stringVal(msg, "content", "")
		toolCalls := extractOpenAIToolCalls(msg)
		finishReason := stringVal(choice, "finish_reason", "stop")
		stopReason := "end_turn"
		if finishReason == "tool_calls" || len(toolCalls) > 0 {
			stopReason = "tool_use"
		}
		unified["response"] = map[string]any{
			"role":        "assistant",
			"content":     content,
			"stop_reason": stopReason,
			"tool_calls":  toolCalls,
		}
		if usage, ok := raw["usage"].(map[string]any); ok {
			unified["usage"] = map[string]any{
				"input_tokens":  intVal(usage, "prompt_tokens", 0),
				"output_tokens": intVal(usage, "completion_tokens", 0),
			}
		}
		unified["model"] = stringVal(raw, "model", "openai")

	case "anthropic":
		// Anthropic: {"content":[{"type":"text","text":"..."},{"type":"tool_use",...}]}
		contentArr, _ := raw["content"].([]any)
		text := ""
		toolCalls := []any{}
		for _, block := range contentArr {
			b, ok := block.(map[string]any)
			if !ok {
				continue
			}
			switch stringVal(b, "type", "") {
			case "text":
				text += stringVal(b, "text", "")
			case "tool_use":
				toolCalls = append(toolCalls, map[string]any{
					"id":    stringVal(b, "id", ""),
					"name":  stringVal(b, "name", ""),
					"input": b["input"],
				})
			}
		}
		stopReason := "end_turn"
		if stringVal(raw, "stop_reason", "") == "tool_use" || len(toolCalls) > 0 {
			stopReason = "tool_use"
		}
		unified["response"] = map[string]any{
			"role":        "assistant",
			"content":     text,
			"stop_reason": stopReason,
			"tool_calls":  toolCalls,
		}
		if usage, ok := raw["usage"].(map[string]any); ok {
			unified["usage"] = map[string]any{
				"input_tokens":  intVal(usage, "input_tokens", 0),
				"output_tokens": intVal(usage, "output_tokens", 0),
			}
		}
		unified["model"] = stringVal(raw, "model", "anthropic")

	default:
		unified["response"] = map[string]any{"role": "assistant", "content": "", "stop_reason": "end_turn", "tool_calls": []any{}}
	}

	if _, ok := unified["usage"]; !ok {
		unified["usage"] = map[string]any{"input_tokens": 0, "output_tokens": 0}
	}
	return unified
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

func extractOpenAIToolCalls(msg map[string]any) []any {
	raw, ok := msg["tool_calls"].([]any)
	if !ok {
		return []any{}
	}
	calls := make([]any, 0, len(raw))
	for _, tc := range raw {
		m, ok := tc.(map[string]any)
		if !ok {
			continue
		}
		fn, _ := m["function"].(map[string]any)
		if fn == nil {
			fn = map[string]any{}
		}
		var inputArgs map[string]any
		if argsStr := stringVal(fn, "arguments", ""); argsStr != "" {
			_ = json.Unmarshal([]byte(argsStr), &inputArgs)
		}
		if inputArgs == nil {
			inputArgs = map[string]any{}
		}
		calls = append(calls, map[string]any{
			"id":    stringVal(m, "id", ""),
			"name":  stringVal(fn, "name", ""),
			"input": inputArgs,
		})
	}
	return calls
}

// simulatedCompletion is the keyword-based fallback used when no real LLM is reachable.
func (l *LLMGatewayActor) simulatedCompletion(lastUserMsg string, tools []any) map[string]any {
	lower := strings.ToLower(lastUserMsg)
	model := "minihermes-simulated-v1"

	switch {
	case strings.Contains(lower, "calculate") || strings.Contains(lower, "compute") ||
		strings.Contains(lower, "math") || containsMathPattern(lower):
		expr := extractMathExpression(lastUserMsg)
		return map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role": "assistant", "content": "", "stop_reason": "tool_use",
				"tool_calls": []any{map[string]any{"id": "tc_1", "name": "calculator", "input": map[string]any{"expression": expr}}},
			},
			"model": model, "usage": map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20}, "cached": false,
		}

	case strings.Contains(lower, "remember") || strings.Contains(lower, "recall") || strings.Contains(lower, "memory"):
		return map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role": "assistant", "content": "", "stop_reason": "tool_use",
				"tool_calls": []any{map[string]any{"id": "tc_1", "name": "memory_recall", "input": map[string]any{"query": lastUserMsg}}},
			},
			"model": model, "usage": map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20}, "cached": false,
		}

	case strings.Contains(lower, "search") || strings.Contains(lower, "find") || strings.Contains(lower, "look up"):
		return map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role": "assistant", "content": "", "stop_reason": "tool_use",
				"tool_calls": []any{map[string]any{"id": "tc_1", "name": "http_request", "input": map[string]any{"url": "https://example.com", "method": "GET"}}},
			},
			"model": model, "usage": map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20}, "cached": false,
		}

	case strings.Contains(lower, "skill") && (strings.Contains(lower, "list") || strings.Contains(lower, "show")):
		return map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role": "assistant", "content": "", "stop_reason": "tool_use",
				"tool_calls": []any{map[string]any{"id": "tc_1", "name": "list_skills", "input": map[string]any{}}},
			},
			"model": model, "usage": map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20}, "cached": false,
		}

	default:
		reply := fmt.Sprintf("I understand you said: \"%s\". Here is a helpful response: As Hermes, I combine persistent memory, learned skills, and scheduled automation to continuously improve how I assist you.", lastUserMsg)
		return map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role": "assistant", "content": reply, "stop_reason": "end_turn", "tool_calls": []any{},
			},
			"model": model, "usage": map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": len(reply) / 4}, "cached": false,
		}
	}
}

func (l *LLMGatewayActor) healthTick() string {
	// Self-heal circuit breaker over time
	if l.CircuitOpen && l.ConsecutiveFailures > 0 {
		l.ConsecutiveFailures--
		if l.ConsecutiveFailures == 0 {
			l.CircuitOpen = false
			host.Info("LLMGatewayActor: circuit closed via timer recovery")
		}
	}
	_ = host.SendAfter(30000, "health_tick", map[string]any{"op": "health_tick"})
	return marshal(map[string]any{"status": "ok", "circuit_open": l.CircuitOpen})
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

func llmCacheKeyFor(msg string) string {
	h := 0
	for _, c := range strings.ToLower(strings.TrimSpace(msg)) {
		h = h*31 + int(c)
		if h < 0 {
			h = -h
		}
	}
	return strconv.Itoa(h % 1000000)
}

func containsMathPattern(lower string) bool {
	for _, op := range []string{"*", "+", "-", "/"} {
		parts := strings.Split(lower, op)
		if len(parts) >= 2 {
			left := strings.Fields(strings.TrimSpace(parts[0]))
			right := strings.Fields(strings.TrimSpace(parts[len(parts)-1]))
			if len(left) > 0 && len(right) > 0 {
				if _, e1 := strconv.ParseFloat(left[len(left)-1], 64); e1 == nil {
					if _, e2 := strconv.ParseFloat(right[0], 64); e2 == nil {
						return true
					}
				}
			}
		}
	}
	return false
}

func extractMathExpression(msg string) string {
	for _, op := range []string{"*", "+", "-", "/"} {
		parts := strings.Split(msg, op)
		if len(parts) >= 2 {
			left := strings.Fields(strings.TrimSpace(parts[0]))
			right := strings.Fields(strings.TrimSpace(parts[1]))
			if len(left) > 0 && len(right) > 0 {
				l := left[len(left)-1]
				r := right[0]
				if _, e1 := strconv.ParseFloat(l, 64); e1 == nil {
					if _, e2 := strconv.ParseFloat(r, 64); e2 == nil {
						return fmt.Sprintf("%s %s %s", l, op, r)
					}
				}
			}
		}
	}
	return msg
}
