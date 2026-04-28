// SPDX-License-Identifier: AGPL-3.0-or-later
// LLMRouterActor — simulated LLM provider with prompt caching, circuit breaker,
// and phantom-token credential proxy (NanoClaw Pattern 1).
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

// LLMRouterActor simulates an LLM provider with:
//   - Prompt caching keyed by a hash of the last user message
//   - Circuit breaker that opens after 3 consecutive failures and self-heals via timer
//   - Phantom-token credential proxy: callers supply a session token; the real API
//     key is resolved here and never returned to the caller
type LLMRouterActor struct {
	plexspaces.BaseActor
	RequestCount        int    `json:"request_count"`
	TotalTokens         int    `json:"total_tokens"`
	CacheHits           int    `json:"cache_hits"`
	CircuitOpen         bool   `json:"circuit_open"`
	ConsecutiveFailures int    `json:"consecutive_failures"`
	Model               string `json:"model"`
}

func NewLLMRouterActor() plexspaces.Actor {
	a := &LLMRouterActor{Model: "miniclaw-simulated-v1"}
	a.SetSelf(a)
	return a
}

func newLLMRouterActor() *LLMRouterActor {
	a := &LLMRouterActor{Model: "miniclaw-simulated-v1"}
	a.SetSelf(a)
	return a
}

func (l *LLMRouterActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	l.SetRuntimeMetadata(config.ActorID)
	if m := config.Args["model"]; m != "" {
		l.Model = m
	}
	if err := host.PG().Join("svc:llm_router"); err != nil {
		host.Warn(fmt.Sprintf("LLMRouterActor: failed to join svc:llm_router: %v", err))
	}
	_ = host.SendAfter(30000, "timer_tick", map[string]any{"op": "timer_tick"})
	host.Info(fmt.Sprintf("LLMRouterActor Init actor_id=%s model=%s", config.ActorID, l.Model))
	return ""
}

func (l *LLMRouterActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "chat_completion":
		return l.chatCompletion(p)
	case "register_credential":
		return l.registerCredential(p)
	case "reset_circuit":
		l.CircuitOpen = false
		l.ConsecutiveFailures = 0
		return marshal(map[string]any{"status": "ok", "circuit_open": false})
	case "get_stats":
		return marshal(map[string]any{
			"status":               "ok",
			"request_count":        l.RequestCount,
			"total_tokens":         l.TotalTokens,
			"cache_hits":           l.CacheHits,
			"circuit_open":         l.CircuitOpen,
			"consecutive_failures": l.ConsecutiveFailures,
			"model":                l.Model,
		})
	case "timer_tick":
		return l.timerTick()
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

// registerCredential stores a phantom_token → real_api_key mapping in KV.
// Only the LLMRouterActor reads the real key; callers always use the phantom token.
func (l *LLMRouterActor) registerCredential(p map[string]any) string {
	token := stringVal(p, "phantom_token", "")
	apiKey := stringVal(p, "api_key", "")
	if token == "" || apiKey == "" {
		return marshal(map[string]any{"error": "phantom_token and api_key required"})
	}
	host.KVPut("cred:"+token, apiKey)
	host.Info(fmt.Sprintf("LLMRouterActor: registered credential for token=%s", token))
	return marshal(map[string]any{"status": "ok", "phantom_token": token})
}

// resolveCredential looks up the real API key for a phantom token.
// Returns empty string if not found (simulated LLM proceeds regardless).
func (l *LLMRouterActor) resolveCredential(phantomToken string) string {
	if phantomToken == "" {
		return ""
	}
	return host.KVGet("cred:" + phantomToken)
}

func (l *LLMRouterActor) chatCompletion(p map[string]any) string {
	if l.CircuitOpen {
		return marshal(map[string]any{
			"error":        "circuit_open",
			"circuit_open": true,
			"model":        l.Model,
		})
	}

	// Phantom token pattern: caller supplies a session-scoped token; we resolve
	// it to the real API key here. The key never leaves this actor.
	phantomToken := stringVal(p, "phantom_token", "")
	resolvedKey := l.resolveCredential(phantomToken)
	if phantomToken != "" && resolvedKey == "" {
		host.Warn(fmt.Sprintf("LLMRouterActor: unregistered phantom_token=%s, proceeding as anonymous", phantomToken))
	}
	_ = resolvedKey // used by a real HTTP client; simulated LLM proceeds regardless

	simulateFailure := boolVal(p, "simulate_failure")
	if simulateFailure {
		l.ConsecutiveFailures++
		if l.ConsecutiveFailures >= 3 {
			l.CircuitOpen = true
			return marshal(map[string]any{
				"error":        "circuit opened after consecutive failures",
				"circuit_open": true,
				"failures":     l.ConsecutiveFailures,
			})
		}
		return marshal(map[string]any{
			"error":                "simulated_llm_timeout",
			"circuit_open":         false,
			"consecutive_failures": l.ConsecutiveFailures,
		})
	}

	messages := sliceVal(p, "messages")
	lastUserMsg := ""
	for _, rawMsg := range messages {
		if msg, ok := rawMsg.(map[string]any); ok {
			if stringVal(msg, "role", "") == "user" {
				lastUserMsg = stringVal(msg, "content", "")
			}
		}
	}

	cacheKey := "llm_cache:" + cacheKeyFor(lastUserMsg)
	cached := host.KVGet(cacheKey)
	if cached != "" {
		l.CacheHits++
		var cachedResp map[string]any
		if err := json.Unmarshal([]byte(cached), &cachedResp); err == nil {
			cachedResp["cached"] = true
			return marshal(cachedResp)
		}
	}

	lower := strings.ToLower(lastUserMsg)
	var response map[string]any

	switch {
	case strings.Contains(lower, "calculate") || strings.Contains(lower, "compute") ||
		strings.Contains(lower, "math") || containsPattern(lower):
		expr := extractExpression(lastUserMsg)
		response = map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role":        "assistant",
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls": []any{
					map[string]any{"id": "tc_1", "name": "calculator", "input": map[string]any{"expression": expr}},
				},
			},
			"model":  l.Model,
			"usage":  map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20},
			"cached": false,
		}
	case strings.Contains(lower, "weather") || strings.Contains(lower, "temperature") ||
		strings.Contains(lower, "forecast"):
		loc := extractLocation(lastUserMsg)
		response = map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role":        "assistant",
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls": []any{
					map[string]any{"id": "tc_1", "name": "weather_lookup", "input": map[string]any{"location": loc}},
				},
			},
			"model":  l.Model,
			"usage":  map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20},
			"cached": false,
		}
	case strings.Contains(lower, "remember") || strings.Contains(lower, "recall") ||
		strings.Contains(lower, "memory"):
		response = map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role":        "assistant",
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls": []any{
					map[string]any{"id": "tc_1", "name": "memory_search", "input": map[string]any{"query": lastUserMsg}},
				},
			},
			"model":  l.Model,
			"usage":  map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20},
			"cached": false,
		}
	case strings.Contains(lower, "search") || strings.Contains(lower, "find") ||
		strings.Contains(lower, "look up"):
		response = map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role":        "assistant",
				"content":     "",
				"stop_reason": "tool_use",
				"tool_calls": []any{
					map[string]any{"id": "tc_1", "name": "web_search", "input": map[string]any{"query": lastUserMsg}},
				},
			},
			"model":  l.Model,
			"usage":  map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": 20},
			"cached": false,
		}
	default:
		reply := fmt.Sprintf("I understand you said: \"%s\". Here is a helpful response: This topic relates to AI agents, actor systems, and PlexSpaces primitives that enable scalable, isolated computation.", lastUserMsg)
		response = map[string]any{
			"status": "ok",
			"response": map[string]any{
				"role":        "assistant",
				"content":     reply,
				"stop_reason": "end_turn",
				"tool_calls":  []any{},
			},
			"model":  l.Model,
			"usage":  map[string]any{"input_tokens": len(lastUserMsg) / 4, "output_tokens": len(reply) / 4},
			"cached": false,
		}
	}

	respJSON, err := json.Marshal(response)
	if err != nil {
		host.Warn(fmt.Sprintf("LLMRouterActor: failed to marshal response for cache: %v", err))
	} else {
		host.KVPut(cacheKey, string(respJSON))
	}

	l.RequestCount++
	l.ConsecutiveFailures = 0
	if usage, ok := response["usage"].(map[string]any); ok {
		l.TotalTokens += intVal(usage, "input_tokens", 0) + intVal(usage, "output_tokens", 0)
	}

	l.IncrCounter(host, "llm_completions")
	fireAudit("llm_completion", fmt.Sprintf("model=%s cached=false", l.Model))
	return marshal(response)
}

func (l *LLMRouterActor) timerTick() string {
	if l.CircuitOpen && l.ConsecutiveFailures > 0 {
		l.ConsecutiveFailures--
		if l.ConsecutiveFailures == 0 {
			l.CircuitOpen = false
			host.Info("LLMRouterActor: circuit closed via timer recovery")
		}
	}
	_ = host.SendAfter(30000, "timer_tick", map[string]any{"op": "timer_tick"})
	return marshal(map[string]any{"status": "ok", "circuit_open": l.CircuitOpen})
}

// cacheKeyFor creates a short deterministic key from the message text.
func cacheKeyFor(msg string) string {
	h := 0
	for _, c := range strings.ToLower(strings.TrimSpace(msg)) {
		h = h*31 + int(c)
		if h < 0 {
			h = -h
		}
	}
	return strconv.Itoa(h % 1000000)
}

// containsPattern detects inline math patterns like "42 * 17".
func containsPattern(lower string) bool {
	for _, op := range []string{"*", "+", "-", "/"} {
		parts := strings.Split(lower, op)
		if len(parts) >= 2 {
			left := strings.TrimSpace(parts[0])
			right := strings.TrimSpace(parts[len(parts)-1])
			words := strings.Fields(left)
			if len(words) > 0 {
				last := words[len(words)-1]
				firstRight := strings.Fields(right)
				if len(firstRight) > 0 {
					if _, e1 := strconv.ParseFloat(last, 64); e1 == nil {
						if _, e2 := strconv.ParseFloat(firstRight[0], 64); e2 == nil {
							return true
						}
					}
				}
			}
		}
	}
	return false
}

// extractExpression extracts a math expression like "42 * 17" from a message.
func extractExpression(msg string) string {
	for _, op := range []string{"*", "+", "-", "/"} {
		parts := strings.Split(msg, op)
		if len(parts) >= 2 {
			left := strings.TrimSpace(parts[0])
			right := strings.TrimSpace(parts[1])
			leftWords := strings.Fields(left)
			rightWords := strings.Fields(right)
			if len(leftWords) > 0 && len(rightWords) > 0 {
				l := leftWords[len(leftWords)-1]
				r := rightWords[0]
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

// extractLocation extracts a location from "weather in X" / "weather for X".
func extractLocation(msg string) string {
	lower := strings.ToLower(msg)
	for _, prep := range []string{" in ", " for ", " at "} {
		idx := strings.Index(lower, prep)
		if idx >= 0 {
			rest := strings.TrimSpace(msg[idx+len(prep):])
			words := strings.Fields(rest)
			if len(words) > 0 {
				end := len(words)
				if end > 3 {
					end = 3
				}
				return strings.Join(words[:end], " ")
			}
		}
	}
	return "San Francisco"
}
