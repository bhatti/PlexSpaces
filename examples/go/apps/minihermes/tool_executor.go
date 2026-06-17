// SPDX-License-Identifier: AGPL-3.0-or-later
// ToolExecutorActor — extensible tool registry and executor.
// Built-in tools: calculator, http_request, memory_store, memory_recall, list_skills, create_cron_job.
// Demonstrates: KV (tool definitions), HTTPFetch (http_request tool), PG discovery, extensible dispatch.
package main

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/bhatti/PlexSpaces/sdks/go/plexspaces"
)

// ToolExecutorActor manages a registry of tools and dispatches execution.
// Built-in tools are implemented locally; service-link tools use HTTPFetch;
// skill-backed tools delegate to SkillStoreActor.
type ToolExecutorActor struct {
	plexspaces.BaseActor
	ExecCount   int      `json:"exec_count"`
	ToolNames   []string `json:"tool_names"`
}

func NewToolExecutorActor() plexspaces.Actor {
	a := &ToolExecutorActor{ToolNames: []string{}}
	a.SetSelf(a)
	return a
}

func newToolExecutorActor() *ToolExecutorActor {
	a := &ToolExecutorActor{ToolNames: []string{}}
	a.SetSelf(a)
	return a
}

func (t *ToolExecutorActor) Init(configJSON string) string {
	var config struct {
		ActorID string `json:"actor_id"`
	}
	if err := json.Unmarshal([]byte(configJSON), &config); err != nil {
		return "ERROR: " + err.Error()
	}
	t.SetRuntimeMetadata(config.ActorID)
	if err := host.PG().Join("svc:tools"); err != nil {
		host.Warn(fmt.Sprintf("ToolExecutorActor: failed to join svc:tools: %v", err))
	}
	t.seedBuiltinTools()
	host.Info(fmt.Sprintf("ToolExecutorActor Init actor_id=%s tools=%d", config.ActorID, len(t.ToolNames)))
	return ""
}

var builtinToolDefs = []map[string]any{
	{
		"name":        "calculator",
		"description": "Evaluate arithmetic expressions. Returns exact numeric result.",
		"input_schema": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"expression": map[string]any{"type": "string", "description": "Math expression, e.g. '42 * 17'"},
			},
			"required": []string{"expression"},
		},
		"handler_type": "builtin",
	},
	{
		"name":        "http_request",
		"description": "Make an HTTP request to a URL. Returns status and body.",
		"input_schema": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"url":    map[string]any{"type": "string"},
				"method": map[string]any{"type": "string", "enum": []string{"GET", "POST"}},
				"body":   map[string]any{"type": "string"},
			},
			"required": []string{"url"},
		},
		"handler_type": "builtin",
	},
	{
		"name":        "memory_store",
		"description": "Store a key-value pair in persistent memory.",
		"input_schema": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"key":      map[string]any{"type": "string"},
				"value":    map[string]any{"type": "string"},
				"scope":    map[string]any{"type": "string", "default": "global"},
				"scope_id": map[string]any{"type": "string", "default": ""},
			},
			"required": []string{"key", "value"},
		},
		"handler_type": "builtin",
	},
	{
		"name":        "memory_recall",
		"description": "Recall memories matching a query from persistent memory.",
		"input_schema": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"query":    map[string]any{"type": "string"},
				"scope":    map[string]any{"type": "string", "default": "global"},
				"scope_id": map[string]any{"type": "string", "default": ""},
			},
			"required": []string{"query"},
		},
		"handler_type": "builtin",
	},
	{
		"name":        "list_skills",
		"description": "List all learned skills available to the agent.",
		"input_schema": map[string]any{
			"type":       "object",
			"properties": map[string]any{},
		},
		"handler_type": "builtin",
	},
	{
		"name":        "create_cron_job",
		"description": "Schedule a task to run automatically on a recurring interval.",
		"input_schema": map[string]any{
			"type": "object",
			"properties": map[string]any{
				"job_id":   map[string]any{"type": "string", "description": "Unique job identifier"},
				"prompt":   map[string]any{"type": "string", "description": "The task prompt to execute"},
				"schedule": map[string]any{"type": "string", "description": "Schedule: every_1m, every_5m, every_1h, every_24h"},
			},
			"required": []string{"job_id", "prompt", "schedule"},
		},
		"handler_type": "builtin",
	},
}

func (t *ToolExecutorActor) seedBuiltinTools() {
	existing := host.KVGet("tool_names")
	if existing != "" {
		t.ToolNames = strings.Split(existing, ",")
		return
	}
	for _, def := range builtinToolDefs {
		name := def["name"].(string)
		defJSON, _ := json.Marshal(def)
		host.KVPut("tool:"+name, string(defJSON))
		t.ToolNames = append(t.ToolNames, name)
	}
	host.KVPut("tool_names", strings.Join(t.ToolNames, ","))
}

func (t *ToolExecutorActor) Handle(fromActor, msgType, payloadJSON string) string {
	p := parsePayload(payloadJSON)
	op := stringVal(p, "op", msgType)
	switch op {
	case "register_tool":
		return t.registerTool(p)
	case "list_tools":
		return t.listTools()
	case "execute":
		return t.execute(p)
	case "get_stats":
		depth, _ := host.Ch().Depth("", "tool:results")
		return marshal(map[string]any{
			"status":     "ok",
			"exec_count": t.ExecCount,
			"tool_count": len(t.ToolNames),
			"depth":      depth,
		})
	default:
		return marshal(map[string]any{"error": "unknown_op", "op": op})
	}
}

func (t *ToolExecutorActor) registerTool(p map[string]any) string {
	name := stringVal(p, "name", "")
	if name == "" {
		return marshal(map[string]any{"error": "name is required"})
	}
	def := map[string]any{
		"name":         name,
		"description":  stringVal(p, "description", ""),
		"input_schema": p["input_schema"],
		"handler_type": stringVal(p, "handler_type", "builtin"),
		"handler_config": p["handler_config"],
	}
	if def["input_schema"] == nil {
		def["input_schema"] = map[string]any{"type": "object", "properties": map[string]any{}}
	}
	defJSON, _ := json.Marshal(def)
	host.KVPut("tool:"+name, string(defJSON))

	found := false
	for _, n := range t.ToolNames {
		if n == name {
			found = true
			break
		}
	}
	if !found {
		t.ToolNames = append(t.ToolNames, name)
		host.KVPut("tool_names", strings.Join(t.ToolNames, ","))
	}
	fireAudit("tool_registered", fmt.Sprintf("name=%s", name))
	host.Info(fmt.Sprintf("ToolExecutorActor: registered tool=%s", name))
	return marshal(map[string]any{"status": "ok", "name": name})
}

func (t *ToolExecutorActor) listTools() string {
	tools := make([]any, 0, len(t.ToolNames))
	for _, name := range t.ToolNames {
		raw := host.KVGet("tool:" + name)
		if raw == "" {
			continue
		}
		var def map[string]any
		if err := json.Unmarshal([]byte(raw), &def); err == nil {
			tools = append(tools, def)
		}
	}
	return marshal(map[string]any{"status": "ok", "tools": tools, "count": len(tools)})
}

func (t *ToolExecutorActor) execute(p map[string]any) string {
	name := stringVal(p, "name", "")
	input, _ := p["input"].(map[string]any)
	if input == nil {
		input = map[string]any{}
	}

	raw := host.KVGet("tool:" + name)
	if raw == "" {
		return marshal(map[string]any{"error": fmt.Sprintf("unknown tool: %s", name)})
	}
	var def map[string]any
	if err := json.Unmarshal([]byte(raw), &def); err != nil {
		return marshal(map[string]any{"error": "corrupt tool definition"})
	}

	handlerType := stringVal(def, "handler_type", "builtin")
	var output map[string]any

	switch handlerType {
	case "builtin":
		output = t.dispatchBuiltin(name, input)
	case "service_link":
		output = t.dispatchServiceLink(def, input)
	default:
		output = map[string]any{"error": fmt.Sprintf("unsupported handler_type: %s", handlerType)}
	}

	t.ExecCount++
	t.IncrCounter(host, "tool_executions")
	fireAudit("tool_executed", fmt.Sprintf("name=%s", name))
	return marshal(map[string]any{"status": "ok", "name": name, "output": output})
}

func (t *ToolExecutorActor) dispatchBuiltin(name string, input map[string]any) map[string]any {
	switch name {
	case "calculator":
		expr := stringVal(input, "expression", "")
		result, err := evalExpression(expr)
		if err != nil {
			return map[string]any{"error": err.Error(), "expression": expr}
		}
		return map[string]any{"result": result, "expression": expr}

	case "http_request":
		url := stringVal(input, "url", "")
		method := stringVal(input, "method", "GET")
		body := stringVal(input, "body", "")
		client := plexspaces.NewServiceHTTPClient(host, "http-tool")
		var respMap map[string]any
		var err error
		if method == "POST" {
			respMap, err = client.Post(url, []byte(body), map[string]string{"Content-Type": "application/json"})
		} else {
			respMap, err = client.Get(url, map[string]string{})
		}
		if err != nil {
			return map[string]any{"error": err.Error(), "url": url}
		}
		return respMap

	case "memory_store":
		memID, err := registryFirst("memory", "svc:memory")
		if err != nil {
			return map[string]any{"error": "memory actor not available"}
		}
		resp, askErr := host.Ask(memID, "store_memory", map[string]any{
			"op":       "store_memory",
			"key":      stringVal(input, "key", ""),
			"value":    stringVal(input, "value", ""),
			"scope":    stringVal(input, "scope", "global"),
			"scope_id": stringVal(input, "scope_id", ""),
		}, 5000)
		if askErr != nil {
			return map[string]any{"error": askErr.Error()}
		}
		if rm, ok := resp.(map[string]any); ok {
			return rm
		}
		return map[string]any{"status": "ok"}

	case "memory_recall":
		memID, err := registryFirst("memory", "svc:memory")
		if err != nil {
			return map[string]any{"error": "memory actor not available"}
		}
		resp, askErr := host.Ask(memID, "recall_memory", map[string]any{
			"op":       "recall_memory",
			"query":    stringVal(input, "query", ""),
			"scope":    stringVal(input, "scope", "global"),
			"scope_id": stringVal(input, "scope_id", ""),
		}, 5000)
		if askErr != nil {
			return map[string]any{"error": askErr.Error()}
		}
		if rm, ok := resp.(map[string]any); ok {
			return rm
		}
		return map[string]any{"memories": []any{}}

	case "list_skills":
		skillID, err := registryFirst("skill_store", "svc:skills")
		if err != nil {
			return map[string]any{"skills": []any{}, "count": 0}
		}
		resp, askErr := host.Ask(skillID, "list_skills", map[string]any{"op": "list_skills"}, 5000)
		if askErr != nil {
			return map[string]any{"error": askErr.Error()}
		}
		if rm, ok := resp.(map[string]any); ok {
			return rm
		}
		return map[string]any{"skills": []any{}, "count": 0}

	case "create_cron_job":
		cronID, err := registryFirst("cron_scheduler", "svc:cron")
		if err != nil {
			return map[string]any{"error": "cron_scheduler not available"}
		}
		resp, askErr := host.Ask(cronID, "create_job", map[string]any{
			"op":       "create_job",
			"job_id":   stringVal(input, "job_id", ""),
			"prompt":   stringVal(input, "prompt", ""),
			"schedule": stringVal(input, "schedule", "every_1h"),
		}, 5000)
		if askErr != nil {
			return map[string]any{"error": askErr.Error()}
		}
		if rm, ok := resp.(map[string]any); ok {
			return rm
		}
		return map[string]any{"status": "ok"}

	default:
		return map[string]any{"error": fmt.Sprintf("no builtin handler for tool: %s", name)}
	}
}

func (t *ToolExecutorActor) dispatchServiceLink(def map[string]any, input map[string]any) map[string]any {
	cfg, _ := def["handler_config"].(map[string]any)
	if cfg == nil {
		return map[string]any{"error": "missing handler_config for service_link tool"}
	}
	linkName := stringVal(cfg, "link_name", "")
	path := stringVal(cfg, "path", "/")
	method := stringVal(cfg, "method", "POST")

	client := plexspaces.NewServiceHTTPClient(host, linkName)
	bodyJSON, _ := json.Marshal(input)
	var respMap map[string]any
	var err error
	if method == "POST" {
		respMap, err = client.Post(path, bodyJSON, map[string]string{"Content-Type": "application/json"})
	} else {
		respMap, err = client.Get(path, map[string]string{})
	}
	if err != nil {
		return map[string]any{"error": err.Error()}
	}
	return respMap
}

// evalExpression evaluates a simple arithmetic expression like "42 * 17".
func evalExpression(expr string) (float64, error) {
	expr = strings.TrimSpace(expr)
	for _, op := range []string{"*", "/", "+", "-"} {
		parts := strings.Split(expr, op)
		if len(parts) == 2 {
			l, e1 := strconv.ParseFloat(strings.TrimSpace(parts[0]), 64)
			r, e2 := strconv.ParseFloat(strings.TrimSpace(parts[1]), 64)
			if e1 == nil && e2 == nil {
				switch op {
				case "*":
					return l * r, nil
				case "/":
					if r == 0 {
						return 0, fmt.Errorf("division by zero")
					}
					return l / r, nil
				case "+":
					return l + r, nil
				case "-":
					return l - r, nil
				}
			}
		}
	}
	if n, err := strconv.ParseFloat(expr, 64); err == nil {
		return n, nil
	}
	return 0, fmt.Errorf("cannot evaluate expression: %s", expr)
}
