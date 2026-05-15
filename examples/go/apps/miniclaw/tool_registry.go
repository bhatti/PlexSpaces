// SPDX-License-Identifier: AGPL-3.0-or-later
// ToolRegistryActor — tool registration, listing, and built-in tool execution.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/plexspaces/sdks/go/plexspaces"
)

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
		name, ok := tool["name"].(string)
		if !ok || name == "" {
			host.Warn("ToolRegistryActor: built-in tool has missing or non-string name, skipping")
			continue
		}
		toolJSON, err := json.Marshal(tool)
		if err != nil {
			host.Warn(fmt.Sprintf("ToolRegistryActor: failed to marshal built-in tool %s: %v", name, err))
			continue
		}
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
	toolJSON, err := json.Marshal(p)
	if err != nil {
		return marshal(map[string]any{"error": "failed to serialize tool definition"})
	}
	host.KVPut("tool:"+name, string(toolJSON))

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
	t.IncrCounters(host, map[string]int{"tool_executions": 1, "tool_" + name + "_ran": 1})
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
		"output": map[string]any{"result": result, "expression": expr},
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
	return map[string]any{"status": "ok", "tool": "memory_search", "output": resp}
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
