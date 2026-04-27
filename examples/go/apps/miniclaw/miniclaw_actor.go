// SPDX-License-Identifier: AGPL-3.0-or-later
// MiniClaw — Mini Agent Framework on PlexSpaces (Go WASM)
//
// Demonstrates a minimal agentic AI framework using PlexSpaces actor primitives:
//   - LLMRouterActor:      Simulated LLM provider with prompt caching and circuit breaker
//   - ToolRegistryActor:   Tool registration, listing, and built-in tool execution
//   - AgentActor:          Core agent loop (message → LLM → tool_use → execute → repeat)
//   - SessionManagerActor: Session lifecycle management with KV persistence
//   - OrchestratorActor:   Durable workflow that decomposes tasks and delegates to agents
//   - MemoryActor:         Scoped memory storage via KV + TupleSpace
//   - AuditEventActor:     Fire-and-forget audit trail (GenEvent)
//   - AgentStateFSM:       Agent lifecycle state machine (GenFSM)
//
// Inspired by OpenClaw/NanoClaw/MicroClaw — same abstractions as a Go WASM example.
//
// SDK Features Used:
//   - plexspaces.BaseActor: JSON state serialization / deserialization
//   - plexspaces.WorkflowActor: Run/Signal/Query for durable orchestration
//   - host.Ask(): Request-reply delegation between actors
//   - host.Send(): Fire-and-forget messaging
//   - host.SendAfter(): Delayed timer messages (circuit recovery)
//   - host.KVGet/KVPut/KVList(): Persistent key-value storage
//   - host.TS(): TupleSpace for shared coordination state
//   - host.PG(): Process groups for location-transparent actor discovery
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/plexobject/plexspaces/sdks/go/plexspaces"
)

var host = plexspaces.NewHost()

// ========================================================================
// Helpers
// ========================================================================

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

func boolVal(m map[string]any, key string) bool {
	if v, ok := m[key]; ok {
		if b, ok := v.(bool); ok {
			return b
		}
	}
	return false
}

func sliceVal(m map[string]any, key string) []any {
	if v, ok := m[key]; ok {
		if s, ok := v.([]any); ok {
			return s
		}
	}
	return []any{}
}

// pgFirst returns the first member of a process group, or an error if empty.
// Preferred over role-based routing because the supervisor uses ULIDs as actor names.
func pgFirst(group string) (string, error) {
	members, err := host.PG().Members(group)
	if err != nil {
		return "", fmt.Errorf("pg.Members(%q): %w", group, err)
	}
	if len(members) == 0 {
		return "", fmt.Errorf("no members in pg %q", group)
	}
	return members[0], nil
}

// fireAudit sends a fire-and-forget audit event to the audit actor.
// Failures are logged as warnings but never block the caller.
func fireAudit(eventType, detail string) {
	auditID, err := pgFirst("svc:audit")
	if err != nil {
		host.Debug(fmt.Sprintf("fireAudit: pgFirst failed: %v", err))
		return
	}
	if result := host.Send(auditID, "log_event", map[string]any{
		"op":         "log_event",
		"event_type": eventType,
		"detail":     detail,
		"timestamp":  host.NowMs(),
	}); result != "" {
		host.Warn(fmt.Sprintf("fireAudit: Send failed: %s", result))
	}
}

// writeActorInfo writes agent capability tuples to the shared TupleSpace so
// external queries can discover actors without WASM-to-WASM routing.
func writeActorInfo(actorID, name, description string, capabilities []string) {
	ts := host.TS()
	if result := ts.Write([]any{"agent_card", actorID, name, description}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("writeActorInfo: failed to write card for %s: %s", actorID, result))
	}
	for _, cap := range capabilities {
		if result := ts.Write([]any{"agent_cap", cap, actorID}); strings.HasPrefix(result, "ERROR:") {
			host.Warn(fmt.Sprintf("writeActorInfo: failed to index capability %s for %s: %s", cap, actorID, result))
		}
	}
}

// ========================================================================
// LLMRouterActor (GenServer)
// ========================================================================

// LLMRouterActor simulates an LLM provider with prompt caching and a circuit breaker.
// It routes incoming messages to the appropriate tool or returns a text response
// based on keyword detection — fully deterministic for testing.
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
	// Schedule circuit recovery check every 30 seconds
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

func (l *LLMRouterActor) chatCompletion(p map[string]any) string {
	if l.CircuitOpen {
		return marshal(map[string]any{
			"error":        "circuit_open",
			"circuit_open": true,
			"model":        l.Model,
		})
	}

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
			"circuit_open":        false,
			"consecutive_failures": l.ConsecutiveFailures,
		})
	}

	// Extract last user message for routing logic
	messages := sliceVal(p, "messages")
	lastUserMsg := ""
	for _, rawMsg := range messages {
		if msg, ok := rawMsg.(map[string]any); ok {
			if stringVal(msg, "role", "") == "user" {
				lastUserMsg = stringVal(msg, "content", "")
			}
		}
	}

	// Check prompt cache
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

	// Keyword-based routing (deterministic for tests)
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
					map[string]any{
						"id":    "tc_1",
						"name":  "calculator",
						"input": map[string]any{"expression": expr},
					},
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
					map[string]any{
						"id":    "tc_1",
						"name":  "weather_lookup",
						"input": map[string]any{"location": loc},
					},
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
					map[string]any{
						"id":    "tc_1",
						"name":  "memory_search",
						"input": map[string]any{"query": lastUserMsg},
					},
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
					map[string]any{
						"id":    "tc_1",
						"name":  "web_search",
						"input": map[string]any{"query": lastUserMsg},
					},
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

	// Cache the response
	respJSON, _ := json.Marshal(response)
	host.KVPut(cacheKey, string(respJSON))

	l.RequestCount++
	l.ConsecutiveFailures = 0
	if usage, ok := response["usage"].(map[string]any); ok {
		l.TotalTokens += intVal(usage, "input_tokens", 0) + intVal(usage, "output_tokens", 0)
	}

	if _, err := host.ApplicationMetricsAdd(l.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"llm_completions": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("LLMRouterActor: metrics update failed: %v", err))
	}

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

// containsPattern detects patterns like "42 * 17" or "N op M" in the message.
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

// ========================================================================
// ToolRegistryActor (GenServer)
// ========================================================================

// ToolRegistryActor maintains a registry of tool definitions and executes built-in tools.
// Tools are persisted in KV; a "tool_names" key tracks the list for enumeration.
type ToolRegistryActor struct {
	plexspaces.BaseActor
	ToolCount      int `json:"tool_count"`
	ExecutionCount int `json:"execution_count"`
}

func NewToolRegistryActor() plexspaces.Actor {
	a := &ToolRegistryActor{}
	a.SetSelf(a)
	return a
}

func newToolRegistryActor() *ToolRegistryActor {
	a := &ToolRegistryActor{}
	a.SetSelf(a)
	return a
}

func (t *ToolRegistryActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	t.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:tool_registry"); err != nil {
		host.Warn(fmt.Sprintf("ToolRegistryActor: failed to join svc:tool_registry: %v", err))
	}

	// Register 4 built-in tools
	builtins := []map[string]any{
		{
			"name":        "calculator",
			"description": "Evaluate math expressions",
			"input_schema": map[string]any{
				"type":       "object",
				"properties": map[string]any{"expression": map[string]any{"type": "string"}},
			},
		},
		{
			"name":        "weather_lookup",
			"description": "Get current weather for a location",
			"input_schema": map[string]any{
				"type":       "object",
				"properties": map[string]any{"location": map[string]any{"type": "string"}},
			},
		},
		{
			"name":        "memory_search",
			"description": "Search stored memories",
			"input_schema": map[string]any{
				"type":       "object",
				"properties": map[string]any{"query": map[string]any{"type": "string"}},
			},
		},
		{
			"name":        "web_search",
			"description": "Search the web",
			"input_schema": map[string]any{
				"type":       "object",
				"properties": map[string]any{"query": map[string]any{"type": "string"}},
			},
		},
	}

	names := make([]string, 0, len(builtins))
	for _, tool := range builtins {
		name := tool["name"].(string)
		toolJSON, _ := json.Marshal(tool)
		host.KVPut("tool:"+name, string(toolJSON))
		names = append(names, name)
	}
	namesJSON, _ := json.Marshal(names)
	host.KVPut("tool_names", string(namesJSON))
	t.ToolCount = len(builtins)

	host.Info(fmt.Sprintf("ToolRegistryActor Init actor_id=%s tools=%d", config.ActorID, t.ToolCount))
	return ""
}

func (t *ToolRegistryActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "register_tool":
		return t.registerTool(p)
	case "list_tools":
		return t.listTools()
	case "execute_tool":
		return t.executeTool(p)
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"tool_count":      t.ToolCount,
			"execution_count": t.ExecutionCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (t *ToolRegistryActor) registerTool(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "name is required"})
	}
	toolJSON, _ := json.Marshal(p)
	host.KVPut("tool:"+name, string(toolJSON))

	// Update tool_names list
	namesRaw := host.KVGet("tool_names")
	var names []string
	_ = json.Unmarshal([]byte(namesRaw), &names)
	found := false
	for _, n := range names {
		if n == name {
			found = true
			break
		}
	}
	if !found {
		names = append(names, name)
		namesJSON, _ := json.Marshal(names)
		host.KVPut("tool_names", string(namesJSON))
		t.ToolCount++
	}

	host.Info(fmt.Sprintf("ToolRegistryActor: registered tool=%s", name))
	return marshal(map[string]any{"status": "ok", "tool": name})
}

func (t *ToolRegistryActor) listTools() string {
	namesRaw := host.KVGet("tool_names")
	var names []string
	_ = json.Unmarshal([]byte(namesRaw), &names)

	tools := make([]any, 0, len(names))
	for _, name := range names {
		toolRaw := host.KVGet("tool:" + name)
		if toolRaw == "" {
			continue
		}
		var tool map[string]any
		if err := json.Unmarshal([]byte(toolRaw), &tool); err == nil {
			tools = append(tools, tool)
		}
	}
	return marshal(map[string]any{"status": "ok", "tools": tools, "count": len(tools)})
}

func (t *ToolRegistryActor) executeTool(p map[string]any) string {
	name := stringVal(p, "name", "")
	inputRaw, _ := p["input"].(map[string]any)
	if inputRaw == nil {
		inputRaw = map[string]any{}
	}

	var result map[string]any
	switch name {
	case "calculator":
		result = t.execCalculator(inputRaw)
	case "weather_lookup":
		result = t.execWeather(inputRaw)
	case "memory_search":
		result = t.execMemorySearch(inputRaw)
	case "web_search":
		result = t.execWebSearch(inputRaw)
	default:
		return marshal(map[string]any{"error": "unknown_tool", "tool": name})
	}

	t.ExecutionCount++
	if _, err := host.ApplicationMetricsAdd(t.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"tool_executions":       1,
			"tool_" + name + "_ran": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("ToolRegistryActor: metrics update failed: %v", err))
	}
	fireAudit("tool_executed", fmt.Sprintf("tool=%s", name))
	return marshal(result)
}

func (t *ToolRegistryActor) execCalculator(input map[string]any) map[string]any {
	expr := stringVal(input, "expression", "")
	result, err := evalExpression(expr)
	if err != nil {
		return map[string]any{"error": err.Error(), "tool": "calculator"}
	}
	return map[string]any{
		"status": "ok",
		"tool":   "calculator",
		"output": map[string]any{
			"result":     result,
			"expression": expr,
		},
	}
}

// evalExpression parses and evaluates a simple "A op B" expression.
func evalExpression(expr string) (float64, error) {
	for _, op := range []string{"*", "+", "/", "-"} {
		idx := strings.Index(expr, op)
		if idx > 0 {
			leftStr := strings.TrimSpace(expr[:idx])
			rightStr := strings.TrimSpace(expr[idx+1:])
			left, e1 := strconv.ParseFloat(leftStr, 64)
			right, e2 := strconv.ParseFloat(rightStr, 64)
			if e1 != nil || e2 != nil {
				continue
			}
			switch op {
			case "*":
				return left * right, nil
			case "+":
				return left + right, nil
			case "-":
				return left - right, nil
			case "/":
				if right == 0 {
					return 0, fmt.Errorf("division by zero")
				}
				return left / right, nil
			}
		}
	}
	// Try parsing as a plain number
	v, err := strconv.ParseFloat(strings.TrimSpace(expr), 64)
	if err != nil {
		return 0, fmt.Errorf("cannot parse expression: %s", expr)
	}
	return v, nil
}

func (t *ToolRegistryActor) execWeather(input map[string]any) map[string]any {
	location := stringVal(input, "location", "San Francisco")
	return map[string]any{
		"status": "ok",
		"tool":   "weather_lookup",
		"output": map[string]any{
			"location":    location,
			"temperature": 68,
			"conditions":  "partly cloudy",
			"humidity":    72,
			"unit":        "fahrenheit",
		},
	}
}

func (t *ToolRegistryActor) execMemorySearch(input map[string]any) map[string]any {
	query := stringVal(input, "query", "")
	empty := map[string]any{
		"status": "ok",
		"tool":   "memory_search",
		"output": map[string]any{"memories": []any{}, "count": 0, "query": query},
	}
	// Discover MemoryActor via process group
	memID, err := pgFirst("svc:memory")
	if err != nil {
		host.Debug(fmt.Sprintf("execMemorySearch: pgFirst svc:memory: %v", err))
		return empty
	}
	resp, err := host.Ask(memID, "recall_memory", map[string]any{
		"op":       "recall_memory",
		"scope":    "global",
		"scope_id": "",
		"query":    query,
	}, 5000)
	if err != nil {
		host.Warn(fmt.Sprintf("execMemorySearch: Ask recall_memory failed: %v", err))
		return empty
	}
	return map[string]any{
		"status": "ok",
		"tool":   "memory_search",
		"output": resp,
	}
}

func (t *ToolRegistryActor) execWebSearch(input map[string]any) map[string]any {
	query := stringVal(input, "query", "")
	return map[string]any{
		"status": "ok",
		"tool":   "web_search",
		"output": map[string]any{
			"query": query,
			"results": []any{
				map[string]any{
					"title":   "PlexSpaces: Actor Framework for AI Agents",
					"snippet": "PlexSpaces provides actor isolation, process groups, and durable state for building secure agentic applications.",
					"url":     "https://example.com/plexspaces",
				},
			},
		},
	}
}

// ========================================================================
// AgentActor (GenServer)
// ========================================================================

// AgentActor implements the core agent loop: receive user message → call LLM →
// if tool_use, execute tools and feed results back → repeat until end_turn.
// Discovers LLM and tools via process groups (location transparent).
type AgentActor struct {
	plexspaces.BaseActor
	SystemPrompt string           `json:"system_prompt"`
	Messages     []map[string]any `json:"messages"`
	MaxHistory   int              `json:"max_history"`
	LoopCount    int              `json:"loop_count"`
	TotalChats   int              `json:"total_chats"`
	AgentName    string           `json:"agent_name"`
	Capabilities []string         `json:"capabilities"`
}

func NewAgentActor() plexspaces.Actor {
	a := &AgentActor{MaxHistory: 50, AgentName: "general-assistant"}
	a.SetSelf(a)
	return a
}

func newAgentActor() *AgentActor {
	a := &AgentActor{MaxHistory: 50, AgentName: "general-assistant"}
	a.SetSelf(a)
	return a
}

func (a *AgentActor) Init(configJSON string) string {
	var config struct {
		ActorID string            `json:"actor_id"`
		Args    map[string]string `json:"args"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	a.SetRuntimeMetadata(config.ActorID)
	if name := config.Args["agent_name"]; name != "" {
		a.AgentName = name
	}
	if sp := config.Args["system_prompt"]; sp != "" {
		a.SystemPrompt = sp
	} else {
		a.SystemPrompt = "You are a helpful AI assistant with access to tools."
	}
	if mh := config.Args["max_history"]; mh != "" {
		if n, err := strconv.Atoi(mh); err == nil {
			a.MaxHistory = n
		}
	}
	a.Capabilities = []string{"chat", "tool_use", "memory"}

	if err := host.PG().Join("svc:agent"); err != nil {
		host.Warn(fmt.Sprintf("AgentActor: failed to join svc:agent: %v", err))
	}

	// Publish agent capability info to TupleSpace for external discovery
	writeActorInfo(config.ActorID, a.AgentName,
		"Core agent loop with tool calling and session memory",
		a.Capabilities)

	host.Info(fmt.Sprintf("AgentActor Init actor_id=%s name=%s", config.ActorID, a.AgentName))
	return ""
}

func (a *AgentActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "chat":
		return a.chat(p)
	case "set_system_prompt":
		a.SystemPrompt = stringVal(p, "prompt", a.SystemPrompt)
		return marshal(map[string]any{"status": "ok"})
	case "get_history":
		return marshal(map[string]any{
			"status":   "ok",
			"messages": a.Messages,
			"count":    len(a.Messages),
		})
	case "compact_context":
		return a.compactContext(10)
	case "get_capabilities":
		return marshal(map[string]any{"status": "ok", "capabilities": a.Capabilities})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (a *AgentActor) chat(p map[string]any) string {
	userMsg := stringVal(p, "message", "")
	sessionID := stringVal(p, "session_id", "")

	if userMsg == "" {
		return marshal(map[string]any{"error": "message is required"})
	}

	// Add user message to history
	a.Messages = append(a.Messages, map[string]any{"role": "user", "content": userMsg})

	// Discover tool registry
	toolRegID, _ := pgFirst("svc:tool_registry")
	tools := []any{}
	if toolRegID != "" {
		resp, err := host.Ask(toolRegID, "list_tools", map[string]any{"op": "list_tools"}, 5000)
		if err == nil {
			if respMap, ok := resp.(map[string]any); ok {
				if ts, ok := respMap["tools"].([]any); ok {
					tools = ts
				}
			}
		}
	}

	// Update FSM to processing
	fsmID, _ := pgFirst("svc:agent_fsm")
	if fsmID != "" {
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "processing"})
	}

	// Agent loop — max 5 iterations
	const maxIter = 5
	loopIter := 0
	finalResponse := ""

	for loopIter < maxIter {
		loopIter++

		llmID, _ := pgFirst("svc:llm_router")
		if llmID == "" {
			finalResponse = fmt.Sprintf("Agent processed: %s (LLM not available in test mode)", userMsg)
			break
		}

		llmResp, err := host.Ask(llmID, "chat_completion", map[string]any{
			"op":       "chat_completion",
			"messages": a.Messages,
			"tools":    tools,
		}, 10000)
		if err != nil {
			finalResponse = fmt.Sprintf("LLM error: %v", err)
			break
		}

		// Parse response
		llmMap, ok := llmResp.(map[string]any)
		if !ok {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}

		// Check for circuit open error
		if _, hasErr := llmMap["error"]; hasErr {
			finalResponse = fmt.Sprintf("LLM unavailable: %v", llmMap["error"])
			break
		}

		respInner, _ := llmMap["response"].(map[string]any)
		if respInner == nil {
			finalResponse = fmt.Sprintf("Agent processed: %s", userMsg)
			break
		}

		stopReason := stringVal(respInner, "stop_reason", "end_turn")
		content := stringVal(respInner, "content", "")

		// Append assistant message to history
		assistantMsg := map[string]any{
			"role":        "assistant",
			"content":     content,
			"stop_reason": stopReason,
		}
		if tcs := respInner["tool_calls"]; tcs != nil {
			assistantMsg["tool_calls"] = tcs
		}
		a.Messages = append(a.Messages, assistantMsg)

		if stopReason == "end_turn" {
			finalResponse = content
			break
		}

		if stopReason == "tool_use" {
			toolCalls, _ := respInner["tool_calls"].([]any)
			allToolResults := []string{}

			if fsmID != "" {
				_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "tool_executing"})
			}

			for _, tcRaw := range toolCalls {
				tc, ok := tcRaw.(map[string]any)
				if !ok {
					continue
				}
				tcID := stringVal(tc, "id", "tc_unknown")
				tcName := stringVal(tc, "name", "")
				tcInput, _ := tc["input"].(map[string]any)
				if tcInput == nil {
					tcInput = map[string]any{}
				}

				var toolOutput map[string]any
				if toolRegID != "" {
					toolResp, err := host.Ask(toolRegID, "execute_tool", map[string]any{
						"op":    "execute_tool",
						"name":  tcName,
						"input": tcInput,
					}, 5000)
					if err == nil {
						if tr, ok := toolResp.(map[string]any); ok {
							toolOutput = tr
						}
					}
				}
				if toolOutput == nil {
					toolOutput = map[string]any{"error": "tool execution failed", "tool": tcName}
				}

				toolOutputJSON, _ := json.Marshal(toolOutput)
				a.Messages = append(a.Messages, map[string]any{
					"role":         "tool",
					"tool_call_id": tcID,
					"content":      string(toolOutputJSON),
				})
				allToolResults = append(allToolResults, fmt.Sprintf("%s: %s", tcName, string(toolOutputJSON)))

				fireAudit("tool_called", fmt.Sprintf("tool=%s session=%s", tcName, sessionID))
			}

			if fsmID != "" {
				_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "processing"})
			}

			// Build a response from tool results for the final text
			if len(allToolResults) > 0 {
				finalResponse = fmt.Sprintf("Tool results: %s", strings.Join(allToolResults, "; "))
			}
			continue
		}

		// Unknown stop reason — break
		finalResponse = content
		break
	}

	a.LoopCount += loopIter

	// Update FSM to idle
	if fsmID != "" {
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "responding"})
		_ = host.Send(fsmID, "transition", map[string]any{"op": "transition", "to": "idle"})
	}

	// Context compaction if over limit
	if len(a.Messages) > a.MaxHistory {
		_ = a.compactContext(a.MaxHistory / 2)
	}

	// Persist session history
	if sessionID != "" {
		msgsJSON, _ := json.Marshal(a.Messages)
		host.KVPut("session_history:"+sessionID, string(msgsJSON))
	}

	a.TotalChats++

	if _, err := host.ApplicationMetricsAdd(a.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"agent_chats": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("AgentActor: metrics update failed: %v", err))
	}

	fireAudit("agent_chat", fmt.Sprintf("session=%s iterations=%d", sessionID, loopIter))

	return marshal(map[string]any{
		"status":           "ok",
		"response":         finalResponse,
		"session_id":       sessionID,
		"loop_iterations":  loopIter,
		"messages_count":   len(a.Messages),
	})
}

func (a *AgentActor) compactContext(keep int) string {
	if len(a.Messages) <= keep {
		return marshal(map[string]any{"status": "ok", "messages_count": len(a.Messages)})
	}
	// Keep the first message (system prompt context) + last `keep` messages
	compacted := make([]map[string]any, 0, keep+1)
	if len(a.Messages) > 0 {
		compacted = append(compacted, a.Messages[0])
	}
	start := len(a.Messages) - keep
	if start < 1 {
		start = 1
	}
	compacted = append(compacted, a.Messages[start:]...)
	a.Messages = compacted
	return marshal(map[string]any{"status": "ok", "messages_count": len(a.Messages)})
}

// ========================================================================
// SessionManagerActor (GenServer)
// ========================================================================

// SessionManagerActor manages agent session lifecycle: create, get, end, list.
// Sessions are persisted in KV; a mapping by channel+user enables lookup.
type SessionManagerActor struct {
	plexspaces.BaseActor
	ActiveSessions int      `json:"active_sessions"`
	TotalCreated   int      `json:"total_created"`
	SessionIDs     []string `json:"session_ids"`
}

func NewSessionManagerActor() plexspaces.Actor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func newSessionManagerActor() *SessionManagerActor {
	a := &SessionManagerActor{}
	a.SetSelf(a)
	return a
}

func (s *SessionManagerActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	s.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:session_manager"); err != nil {
		host.Warn(fmt.Sprintf("SessionManagerActor: failed to join svc:session_manager: %v", err))
	}
	host.Info(fmt.Sprintf("SessionManagerActor Init actor_id=%s", config.ActorID))
	return ""
}

func (s *SessionManagerActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "create_session":
		return s.createSession(p)
	case "get_session":
		return s.getSession(p)
	case "end_session":
		return s.endSession(p)
	case "list_sessions":
		return s.listSessions()
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"active_sessions": s.ActiveSessions,
			"total_created":   s.TotalCreated,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (s *SessionManagerActor) createSession(p map[string]any) string {
	channel := stringVal(p, "channel", "web")
	userID := stringVal(p, "user_id", "anonymous")
	agentID := stringVal(p, "agent_id", "agent")

	// Generate session ID using counter + hash
	sessionID := fmt.Sprintf("sess-%s-%s-%d", channel, userID, host.NowMs())

	meta := map[string]any{
		"session_id": sessionID,
		"channel":    channel,
		"user_id":    userID,
		"agent_id":   agentID,
		"created_at": host.NowMs(),
		"status":     "active",
	}
	metaJSON, _ := json.Marshal(meta)
	host.KVPut("session:"+sessionID, string(metaJSON))
	host.KVPut("session_map:"+channel+":"+userID, sessionID)

	s.SessionIDs = append(s.SessionIDs, sessionID)
	s.ActiveSessions++
	s.TotalCreated++

	if _, err := host.ApplicationMetricsAdd(s.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"sessions_created": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("SessionManagerActor: metrics update failed: %v", err))
	}

	fireAudit("session_created", fmt.Sprintf("session_id=%s channel=%s user_id=%s", sessionID, channel, userID))
	host.Info(fmt.Sprintf("SessionManager: created session_id=%s", sessionID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) getSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		// Lookup by channel + user
		channel := stringVal(p, "channel", "")
		userID := stringVal(p, "user_id", "")
		if channel != "" && userID != "" {
			sessionID = host.KVGet("session_map:" + channel + ":" + userID)
		}
	}
	if sessionID == "" {
		return marshal(map[string]any{"error": "session not found"})
	}
	raw := host.KVGet("session:" + sessionID)
	if raw == "" {
		return marshal(map[string]any{"error": "session not found", "session_id": sessionID})
	}
	var meta map[string]any
	if err := json.Unmarshal([]byte(raw), &meta); err != nil {
		return marshal(map[string]any{"error": "corrupt session data"})
	}
	meta["status"] = "ok"
	return marshal(meta)
}

func (s *SessionManagerActor) endSession(p map[string]any) string {
	sessionID := stringVal(p, "session_id", "")
	if sessionID == "" {
		return marshal(map[string]any{"error": "session_id is required"})
	}
	host.KVDelete("session:" + sessionID)

	// Remove from SessionIDs
	newIDs := make([]string, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		if id != sessionID {
			newIDs = append(newIDs, id)
		}
	}
	s.SessionIDs = newIDs
	if s.ActiveSessions > 0 {
		s.ActiveSessions--
	}

	fireAudit("session_ended", fmt.Sprintf("session_id=%s", sessionID))
	return marshal(map[string]any{"status": "ok", "session_id": sessionID})
}

func (s *SessionManagerActor) listSessions() string {
	sessions := make([]any, 0, len(s.SessionIDs))
	for _, id := range s.SessionIDs {
		raw := host.KVGet("session:" + id)
		if raw == "" {
			continue
		}
		var meta map[string]any
		if err := json.Unmarshal([]byte(raw), &meta); err == nil {
			sessions = append(sessions, meta)
		}
	}
	return marshal(map[string]any{"status": "ok", "sessions": sessions, "count": len(sessions)})
}

// ========================================================================
// OrchestratorActor (WorkflowActor)
// ========================================================================

// OrchestratorActor is a durable workflow that decomposes a task into sub-tasks,
// delegates each to an AgentActor discovered via process group, and aggregates
// results via TupleSpace coordination.
//
// The framework routes workflow_run/workflow_signal/workflow_query to Run/Signal/Query
// automatically. Handle() must not dispatch these itself.
type OrchestratorActor struct {
	plexspaces.BaseActor
	Status   string `json:"status"`
	TaskID   string `json:"task_id"`
	Progress int    `json:"progress"`
}

func NewOrchestratorActor() plexspaces.Actor {
	a := &OrchestratorActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func newOrchestratorActor() *OrchestratorActor {
	a := &OrchestratorActor{Status: "idle"}
	a.SetSelf(a)
	return a
}

func (o *OrchestratorActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	o.SetRuntimeMetadata(config.ActorID)
	o.Status = "idle"
	host.Info(fmt.Sprintf("OrchestratorActor Init actor_id=%s", config.ActorID))
	return ""
}

// Handle must NOT dispatch workflow_run/signal/query — the framework does that.
func (o *OrchestratorActor) Handle(from, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	switch msgType {
	case "workflow_run":
		return o.Run(payloadJSON)
	case "workflow_query":
		name := stringVal(p, "name", "status")
		return o.Query(name, payloadJSON)
	case "workflow_signal":
		name := stringVal(p, "name", "")
		o.Signal(name, payloadJSON)
		return marshal(map[string]any{"ok": true})
	}
	return marshal(map[string]any{"error": "use workflow_run / workflow_signal / workflow_query"})
}

func (o *OrchestratorActor) Run(payloadJSON string) string {
	p := parsePayload(payloadJSON)
	task := stringVal(p, "task", "explain how agents work")
	taskID := stringVal(p, "task_id", fmt.Sprintf("orch-%d", host.NowMs()))

	o.Status = "running"
	o.TaskID = taskID
	o.Progress = 0

	host.Info(fmt.Sprintf("OrchestratorActor Run taskID=%s task=%s", taskID, task))

	// Discover agent via process group
	agentID, err := pgFirst("svc:agent")
	if err != nil {
		o.Status = "failed"
		return marshal(map[string]any{"error": "no agents in svc:agent process group", "task_id": taskID})
	}

	// Decompose task into sub-tasks by splitting on " and "
	subTasks := []string{}
	if idx := strings.Index(strings.ToLower(task), " and "); idx >= 0 {
		subTasks = append(subTasks, strings.TrimSpace(task[:idx]))
		subTasks = append(subTasks, strings.TrimSpace(task[idx+5:]))
	} else {
		subTasks = []string{task}
	}

	subResults := make([]any, 0, len(subTasks))
	for i, subTask := range subTasks {
		o.Progress = (i + 1) * 100 / len(subTasks)
		resp, err := host.Ask(agentID, "chat", map[string]any{
			"op":         "chat",
			"message":    subTask,
			"session_id": fmt.Sprintf("orch-%s-%d", taskID, i),
		}, 15000)
		if err != nil {
			o.Status = "failed"
			return marshal(map[string]any{"error": "sub-task failed: " + err.Error(), "task_id": taskID})
		}

		// Store result in TupleSpace for coordination
		resultJSON, _ := json.Marshal(resp)
		_ = host.TS().Write([]any{"orch_result", taskID, i, string(resultJSON)})
		subResults = append(subResults, resp)
	}

	// Aggregate sub-results
	summaries := make([]string, 0, len(subResults))
	for _, r := range subResults {
		if rm, ok := r.(map[string]any); ok {
			if response := stringVal(rm, "response", ""); response != "" {
				summaries = append(summaries, response)
			}
		}
	}
	aggregated := strings.Join(summaries, " | ")

	o.Status = "completed"
	o.Progress = 100

	if _, err := host.ApplicationMetricsAdd(o.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"orchestrator_runs": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("OrchestratorActor: metrics update failed: %v", err))
	}

	fireAudit("orchestrator_completed", fmt.Sprintf("task_id=%s subtasks=%d", taskID, len(subTasks)))
	return marshal(map[string]any{
		"status":      "ok",
		"task_id":     taskID,
		"result":      aggregated,
		"sub_results": subResults,
		"sub_tasks":   len(subTasks),
	})
}

func (o *OrchestratorActor) Signal(name, payloadJSON string) {
	switch name {
	case "cancel":
		o.Status = "cancelled"
		host.Info(fmt.Sprintf("OrchestratorActor cancelled task_id=%s", o.TaskID))
	}
}

func (o *OrchestratorActor) Query(name, _ string) string {
	if name == "status" {
		return marshal(map[string]any{
			"task_id":  o.TaskID,
			"status":   o.Status,
			"progress": o.Progress,
		})
	}
	return marshal(map[string]any{"error": "unknown_query", "name": name})
}

// ========================================================================
// MemoryActor (GenServer)
// ========================================================================

// MemoryActor provides scoped memory storage backed by KV (persistent) and
// TupleSpace (queryable). Supports global, agent, and session scope namespacing.
type MemoryActor struct {
	plexspaces.BaseActor
	MemoryCount int `json:"memory_count"`
}

func NewMemoryActor() plexspaces.Actor {
	a := &MemoryActor{}
	a.SetSelf(a)
	return a
}

func newMemoryActor() *MemoryActor {
	a := &MemoryActor{}
	a.SetSelf(a)
	return a
}

func (m *MemoryActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	m.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:memory"); err != nil {
		host.Warn(fmt.Sprintf("MemoryActor: failed to join svc:memory: %v", err))
	}
	host.Info(fmt.Sprintf("MemoryActor Init actor_id=%s", config.ActorID))
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
	case "get_stats":
		return marshal(map[string]any{"status": "ok", "memory_count": m.MemoryCount})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (m *MemoryActor) storeMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	key := stringVal(p, "key", "")
	value := stringVal(p, "value", "")

	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}

	kvKey := fmt.Sprintf("mem:%s:%s:%s", scope, scopeID, key)
	host.KVPut(kvKey, value)

	// Also write to TupleSpace for queryable access
	if result := host.TS().Write([]any{"memory", scope, scopeID, key, value}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("MemoryActor: TS write failed for key=%s: %s", key, result))
	}

	m.MemoryCount++

	if _, err := host.ApplicationMetricsAdd(m.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"memories_stored": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("MemoryActor: metrics update failed: %v", err))
	}

	host.Info(fmt.Sprintf("MemoryActor: stored scope=%s key=%s", scope, key))
	return marshal(map[string]any{"status": "ok", "key": key, "scope": scope})
}

func (m *MemoryActor) recallMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	query := stringVal(p, "query", "")

	// Read all tuples matching scope/scopeID
	tuples := host.TS().ReadAll([]any{"memory", scope, scopeID, nil, nil})

	memories := make([]any, 0)
	for _, t := range tuples {
		if len(t) < 5 {
			continue
		}
		memKey, _ := t[3].(string)
		memVal, _ := t[4].(string)

		// Filter by query keyword match
		if query == "" ||
			strings.Contains(strings.ToLower(memKey), strings.ToLower(query)) ||
			strings.Contains(strings.ToLower(memVal), strings.ToLower(query)) {
			memories = append(memories, map[string]any{
				"key":      memKey,
				"value":    memVal,
				"scope":    scope,
				"scope_id": scopeID,
			})
		}
	}

	return marshal(map[string]any{
		"status":   "ok",
		"memories": memories,
		"count":    len(memories),
		"query":    query,
	})
}

func (m *MemoryActor) listMemories(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")

	tuples := host.TS().ReadAll([]any{"memory", scope, scopeID, nil, nil})
	memories := make([]any, 0, len(tuples))
	for _, t := range tuples {
		if len(t) < 5 {
			continue
		}
		memKey, _ := t[3].(string)
		memVal, _ := t[4].(string)
		memories = append(memories, map[string]any{
			"key":   memKey,
			"value": memVal,
			"scope": scope,
		})
	}
	return marshal(map[string]any{"status": "ok", "memories": memories, "count": len(memories)})
}

func (m *MemoryActor) deleteMemory(p map[string]any) string {
	scope := stringVal(p, "scope", "global")
	scopeID := stringVal(p, "scope_id", "")
	key := stringVal(p, "key", "")
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	kvKey := fmt.Sprintf("mem:%s:%s:%s", scope, scopeID, key)
	host.KVDelete(kvKey)
	if m.MemoryCount > 0 {
		m.MemoryCount--
	}
	return marshal(map[string]any{"status": "ok", "key": key})
}

// ========================================================================
// AuditEventActor (GenEvent)
// ========================================================================

// AuditEventActor records fire-and-forget audit events to TupleSpace.
// Declared with behavior_kind = "GenEvent" in app-config.toml.
type AuditEventActor struct {
	plexspaces.BaseActor
	EventsLogged  int    `json:"events_logged"`
	LastEventType string `json:"last_event_type"`
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
	case "get_stats":
		return marshal(map[string]any{
			"status":          "ok",
			"events_logged":   a.EventsLogged,
			"last_event_type": a.LastEventType,
		})
	default:
		// GenEvent: treat any unrecognized op as a log event if it has event_type
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

	detailsJSON, _ := json.Marshal(map[string]any{
		"detail": detail,
		"source": stringVal(p, "source", ""),
	})
	if result := host.TS().Write([]any{"audit", eventType, ts, string(detailsJSON)}); strings.HasPrefix(result, "ERROR:") {
		host.Warn(fmt.Sprintf("AuditEventActor: TS write failed for event=%s: %s", eventType, result))
	}

	a.EventsLogged++
	a.LastEventType = eventType

	if _, err := host.ApplicationMetricsAdd(a.ApplicationID(), map[string]any{
		"message_count": 1,
		"counter_metrics": map[string]any{
			"audit_events": 1,
		},
	}); err != nil {
		host.Warn(fmt.Sprintf("AuditEventActor: metrics update failed: %v", err))
	}

	return marshal(map[string]any{"ok": true})
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
		events = append(events, map[string]any{
			"event_type": et,
			"timestamp":  ts,
			"detail":     detail,
		})
	}
	return marshal(map[string]any{"status": "ok", "events": events, "count": len(events)})
}

// ========================================================================
// AgentStateFSM (GenFSM)
// ========================================================================

// AgentStateFSM tracks the agent processing lifecycle.
// States: idle → processing → tool_executing → processing → responding → idle
// Any state can transition to error; error can recover to idle.
type AgentStateFSM struct {
	plexspaces.BaseActor
	FSMState        string `json:"fsm_state"`
	TransitionCount int    `json:"transition_count"`
}

func NewAgentStateFSM() plexspaces.Actor {
	a := &AgentStateFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func newAgentStateFSM() *AgentStateFSM {
	a := &AgentStateFSM{FSMState: "idle"}
	a.SetSelf(a)
	return a
}

func (f *AgentStateFSM) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	f.SetRuntimeMetadata(config.ActorID)
	f.FSMState = "idle"
	if err := host.PG().Join("svc:agent_fsm"); err != nil {
		host.Warn(fmt.Sprintf("AgentStateFSM: failed to join svc:agent_fsm: %v", err))
	}
	host.Info(fmt.Sprintf("AgentStateFSM Init actor_id=%s", config.ActorID))
	return ""
}

func (f *AgentStateFSM) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "transition":
		return f.transition(p)
	case "get_state":
		return marshal(map[string]any{
			"status":           "ok",
			"state":            f.FSMState,
			"transition_count": f.TransitionCount,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (f *AgentStateFSM) transition(p map[string]any) string {
	to := stringVal(p, "to", "")
	if to == "" {
		return marshal(map[string]any{"error": "to state is required"})
	}

	// Always allow transition to error; allow recovery from error to idle
	if to == "error" || (f.FSMState == "error" && to == "idle") {
		from := f.FSMState
		f.FSMState = to
		f.TransitionCount++
		return marshal(map[string]any{
			"status": "ok",
			"from":   from,
			"to":     to,
			"state":  f.FSMState,
		})
	}

	// Validate transition against allowed table
	allowed := validTransitions()
	validTargets, ok := allowed[f.FSMState]
	if !ok {
		return marshal(map[string]any{
			"error": fmt.Sprintf("unknown source state: %s", f.FSMState),
			"state": f.FSMState,
		})
	}
	for _, valid := range validTargets {
		if valid == to {
			from := f.FSMState
			f.FSMState = to
			f.TransitionCount++
			if _, err := host.ApplicationMetricsAdd(f.ApplicationID(), map[string]any{
				"message_count": 1,
				"counter_metrics": map[string]any{
					"fsm_transitions": 1,
				},
			}); err != nil {
				host.Warn(fmt.Sprintf("AgentStateFSM: metrics update failed: %v", err))
			}
			host.Debug(fmt.Sprintf("AgentStateFSM: %s → %s", from, to))
			return marshal(map[string]any{
				"status": "ok",
				"from":   from,
				"to":     to,
				"state":  f.FSMState,
			})
		}
	}

	host.Warn(fmt.Sprintf("AgentStateFSM: invalid transition %s → %s", f.FSMState, to))
	return marshal(map[string]any{
		"error": fmt.Sprintf("invalid transition %s → %s", f.FSMState, to),
		"state": f.FSMState,
	})
}

// validTransitions returns the allowed state transitions map.
func validTransitions() map[string][]string {
	return map[string][]string{
		"idle":           {"processing"},
		"processing":     {"tool_executing", "responding"},
		"tool_executing": {"processing"},
		"responding":     {"idle"},
		"error":          {"idle"},
	}
}

// ========================================================================
// Registration
// ========================================================================

// init registers all actors with the ActorRouter.
// IMPORTANT: Registration MUST happen in init(), not main().
// The WASM runtime calls init() before main(); the router must be registered
// before the framework starts dispatching messages.
func init() {
	router := plexspaces.NewActorRouter()
	router.Route("llm_router", NewLLMRouterActor)
	router.Route("tool_registry", NewToolRegistryActor)
	router.Route("agent", NewAgentActor)
	router.Route("session_manager", NewSessionManagerActor)
	router.Route("orchestrator", NewOrchestratorActor)
	router.Route("memory", NewMemoryActor)
	router.Route("audit_event", NewAuditEventActor)
	router.Route("agent_fsm", NewAgentStateFSM)
	plexspaces.Register(router)
}

func main() {}
