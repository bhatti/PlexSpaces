// SPDX-License-Identifier: AGPL-3.0-or-later
// ToolRegistryActor — tool catalog with JSON Schema validation.
//
// Demonstrates: SchemaValidationFacet for tool-call guardrails, KV storage for schemas,
// and the "register once, validate always" harness pattern.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"unicode"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// builtinTool describes a registered tool.
type builtinTool struct {
	Description string
	Schema      map[string]any
}

var builtinTools = map[string]builtinTool{
	"web_search": {
		Description: "Search the web for information",
		Schema: map[string]any{
			"type":     "object",
			"required": []any{"query"},
			"properties": map[string]any{
				"query":       map[string]any{"type": "string", "minLength": 1, "maxLength": 500},
				"num_results": map[string]any{"type": "integer", "minimum": 1, "maximum": 20},
			},
		},
	},
	"calculator": {
		Description: "Evaluate a mathematical expression",
		Schema: map[string]any{
			"type":     "object",
			"required": []any{"expression"},
			"properties": map[string]any{
				"expression": map[string]any{"type": "string", "minLength": 1},
			},
		},
	},
	"kv_read": {
		Description: "Read a value from key-value store",
		Schema: map[string]any{
			"type":     "object",
			"required": []any{"key"},
			"properties": map[string]any{
				"key": map[string]any{"type": "string"},
			},
		},
	},
	"kv_write": {
		Description: "Write a value to key-value store",
		Schema: map[string]any{
			"type":     "object",
			"required": []any{"key", "value"},
			"properties": map[string]any{
				"key":   map[string]any{"type": "string"},
				"value": map[string]any{},
			},
		},
	},
}

// ToolRegistryActor maintains the tool catalog and executes validated tool calls.
//
// SchemaValidationFacet (priority 95) validates all incoming method calls
// against registered JSON Schemas before this actor processes them.
// This means invalid calls are rejected at the infrastructure level —
// this actor only sees valid, schema-conformant tool calls.
type ToolRegistryActor struct {
	plexspaces.BaseActor
	ActorID          string `json:"actor_id"`
	TotalExecutions  int    `json:"total_executions"`
	TotalRejections  int    `json:"total_rejections"`
}

func NewToolRegistryActor() plexspaces.Actor {
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
	t.ActorID = config.ActorID

	if err := host.PG().Join("svc:tools"); err != nil {
		host.Warn(fmt.Sprintf("ToolRegistryActor: failed to join svc:tools: %v", err))
	}
	// Store built-in tool schemas in KV for SchemaValidationFacet to discover
	for toolName, def := range builtinTools {
		schemaJSON, _ := json.Marshal(def.Schema)
		host.KV().Put("tool_schema:"+toolName, string(schemaJSON))
	}
	toolNames := make([]string, 0, len(builtinTools))
	for k := range builtinTools {
		toolNames = append(toolNames, k)
	}
	host.Info(fmt.Sprintf("ToolRegistryActor Init actor_id=%s tools=%v", config.ActorID, toolNames))
	return ""
}

func (t *ToolRegistryActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "execute":
		return t.execute(p)
	case "register_tool":
		return t.registerTool(p)
	case "list_tools":
		return t.listTools()
	case "get_stats":
		return marshal(map[string]any{
			"status":           "ok",
			"total_executions": t.TotalExecutions,
			"total_rejections": t.TotalRejections,
		})
	default:
		// Fall through to tool execution by name for direct invocations
		if _, ok := builtinTools[op]; ok {
			p["name"] = op
			return t.execute(p)
		}
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (t *ToolRegistryActor) execute(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "tool name is required"})
	}
	input, _ := p["input"].(map[string]any)
	if input == nil {
		input = map[string]any{}
	}

	t.TotalExecutions++
	t.IncrCounter(host, "tool_executions_total")

	switch name {
	case "web_search":
		query := stringVal(input, "query", "")
		numResults := intVal(input, "num_results", 3)
		return t.webSearch(query, numResults)
	case "calculator":
		expr := stringVal(input, "expression", "")
		return t.calculator(expr)
	case "kv_read":
		key := stringVal(input, "key", "")
		return t.kvRead(key)
	case "kv_write":
		key := stringVal(input, "key", "")
		value := fmt.Sprintf("%v", input["value"])
		return t.kvWrite(key, value)
	default:
		if schemaVal, _ := host.KV().Get("tool_schema:" + name); schemaVal != "" {
			return marshal(map[string]any{"error": fmt.Sprintf("tool '%s' is registered but has no executor", name)})
		}
		return marshal(map[string]any{"error": fmt.Sprintf("unknown tool: %s", name)})
	}
}

func (t *ToolRegistryActor) registerTool(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "tool name is required"})
	}
	description := stringVal(p, "description", "")
	schema, _ := p["schema"].(map[string]any)
	if schema != nil {
		schemaJSON, _ := json.Marshal(schema)
		host.KV().Put("tool_schema:"+name, string(schemaJSON))
	}
	host.KV().Put("tool_desc:"+name, description)
	return marshal(map[string]any{"status": "ok", "tool": name})
}

func (t *ToolRegistryActor) listTools() string {
	tools := make([]any, 0, len(builtinTools))
	for name, def := range builtinTools {
		tools = append(tools, map[string]any{
			"name":        name,
			"description": def.Description,
			"schema":      def.Schema,
		})
	}
	return marshal(map[string]any{"status": "ok", "tools": tools, "count": len(tools)})
}

// webSearch returns deterministic mock results for eval testing.
func (t *ToolRegistryActor) webSearch(query string, numResults int) string {
	if query == "" {
		t.TotalRejections++
		return marshal(map[string]any{"error": "query is required (minLength:1)"})
	}
	if numResults <= 0 {
		numResults = 3
	}
	if numResults > 20 {
		numResults = 20
	}
	results := make([]any, 0, numResults)
	for i := 0; i < numResults && i < 3; i++ {
		results = append(results, map[string]any{
			"title":   fmt.Sprintf("Result %d for: %.40s", i+1, query),
			"url":     fmt.Sprintf("https://example.com/result-%d", i+1),
			"snippet": fmt.Sprintf("This is a relevant snippet about %.30s from result %d.", query, i+1),
		})
	}
	return marshal(map[string]any{"status": "ok", "query": query, "results": results})
}

// calculator performs safe arithmetic evaluation.
func (t *ToolRegistryActor) calculator(expression string) string {
	if expression == "" {
		return marshal(map[string]any{"error": "expression is required"})
	}
	result, err := evalExpression(expression)
	if err != nil {
		return marshal(map[string]any{"error": fmt.Sprintf("calculation failed: %v", err)})
	}
	return marshal(map[string]any{"status": "ok", "expression": expression, "result": result})
}

// evalExpression evaluates a simple arithmetic expression safely.
func evalExpression(expr string) (float64, error) {
	expr = strings.TrimSpace(expr)
	// Only allow safe chars
	for _, c := range expr {
		if !unicode.IsDigit(c) && c != '+' && c != '-' && c != '*' && c != '/' &&
			c != '(' && c != ')' && c != '.' && c != ' ' {
			return 0, fmt.Errorf("unsafe characters in expression")
		}
	}
	// Try to parse simple expressions: number op number
	parts := strings.Fields(expr)
	if len(parts) == 3 {
		a, err1 := strconv.ParseFloat(parts[0], 64)
		b, err2 := strconv.ParseFloat(parts[2], 64)
		if err1 == nil && err2 == nil {
			switch parts[1] {
			case "+":
				return a + b, nil
			case "-":
				return a - b, nil
			case "*":
				return a * b, nil
			case "/":
				if b == 0 {
					return 0, fmt.Errorf("division by zero")
				}
				return a / b, nil
			}
		}
	}
	// Try more complex expressions: "17 * 24 + 89 - 45"
	return evalComplex(expr)
}

// evalComplex handles multi-operator expressions left-to-right (no precedence).
func evalComplex(expr string) (float64, error) {
	expr = strings.ReplaceAll(expr, " ", "")
	// Split on + and - at top level
	tokens := tokenize(expr)
	if len(tokens) == 0 {
		return 0, fmt.Errorf("empty expression")
	}
	result, err := strconv.ParseFloat(tokens[0], 64)
	if err != nil {
		return 0, fmt.Errorf("invalid number: %s", tokens[0])
	}
	for i := 1; i+1 < len(tokens); i += 2 {
		op := tokens[i]
		val, err2 := strconv.ParseFloat(tokens[i+1], 64)
		if err2 != nil {
			return 0, fmt.Errorf("invalid number: %s", tokens[i+1])
		}
		switch op {
		case "+":
			result += val
		case "-":
			result -= val
		case "*":
			result *= val
		case "/":
			if val == 0 {
				return 0, fmt.Errorf("division by zero")
			}
			result /= val
		default:
			return 0, fmt.Errorf("unknown operator: %s", op)
		}
	}
	return result, nil
}

func tokenize(expr string) []string {
	var tokens []string
	var cur strings.Builder
	for i, c := range expr {
		if (c == '+' || c == '-' || c == '*' || c == '/') && i > 0 {
			if cur.Len() > 0 {
				tokens = append(tokens, cur.String())
				cur.Reset()
			}
			tokens = append(tokens, string(c))
		} else {
			cur.WriteRune(c)
		}
	}
	if cur.Len() > 0 {
		tokens = append(tokens, cur.String())
	}
	return tokens
}

func (t *ToolRegistryActor) kvRead(key string) string {
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	value, _ := host.KV().Get("tool_kv:" + key)
	return marshal(map[string]any{"status": "ok", "key": key, "value": value})
}

func (t *ToolRegistryActor) kvWrite(key, value string) string {
	if key == "" {
		return marshal(map[string]any{"error": "key is required"})
	}
	host.KV().Put("tool_kv:"+key, value)
	return marshal(map[string]any{"status": "ok", "key": key})
}
