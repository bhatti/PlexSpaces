# SPDX-License-Identifier: AGPL-3.0-or-later
"""ToolRegistryActor — tool catalog with JSON Schema validation.

Demonstrates: ContractFacet for tool-call guardrails, KV storage for schemas,
and the "register once, validate always" harness pattern.
"""

import json
from plexspaces import actor, state, init_handler, handler, host

_BUILTIN_TOOLS = {
    "web_search": {
        "description": "Search the web for information",
        "schema": {
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": 500},
                "num_results": {"type": "integer", "minimum": 1, "maximum": 20},
            }
        }
    },
    "calculator": {
        "description": "Evaluate a mathematical expression",
        "schema": {
            "type": "object",
            "required": ["expression"],
            "properties": {
                "expression": {"type": "string", "minLength": 1}
            }
        }
    },
    "kv_read": {
        "description": "Read a value from key-value store",
        "schema": {
            "type": "object",
            "required": ["key"],
            "properties": {
                "key": {"type": "string"}
            }
        }
    },
    "kv_write": {
        "description": "Write a value to key-value store",
        "schema": {
            "type": "object",
            "required": ["key", "value"],
            "properties": {
                "key": {"type": "string"},
                "value": {"type": "string"},
            }
        }
    },
}


@actor
class ToolRegistryActor:
    """
    Tool registry: maintains tool catalog and executes validated tool calls.

    ContractFacet (priority 95) validates all incoming tool_call messages
    against registered JSON Schemas before this actor processes them.
    This means invalid calls are rejected at the infrastructure level —
    this actor only sees valid, schema-conformant tool calls.
    """

    actor_id: str = state(default="")
    total_executions: int = state(default=0)
    total_rejections: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv_put("svc:tool_registry", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="tool_registry")
        except Exception:
            pass
        # Register built-in tool schemas in KV for ContractFacet to discover
        for tool_name, tool_def in _BUILTIN_TOOLS.items():
            try:
                key = f"tool_schema:{tool_name}"
                host.kv_put(key, json.dumps(tool_def["schema"]))
            except Exception:
                pass
        host.info(f"ToolRegistryActor init actor_id={self.actor_id} tools={list(_BUILTIN_TOOLS.keys())}")

    @handler("execute")
    def execute(self, name: str = "", input: dict = None) -> dict:
        """Execute a tool call. ContractFacet has already validated arguments."""
        if not name:
            return {"error": "tool name is required"}
        if input is None:
            input = {}

        self.total_executions += 1
        host.incr_counter("tool_executions_total", 1)

        # Route to tool implementation
        if name == "web_search":
            return self._web_search(input.get("query", ""), input.get("num_results", 3))
        elif name == "calculator":
            return self._calculator(input.get("expression", ""))
        elif name == "kv_read":
            return self._kv_read(input.get("key", ""))
        elif name == "kv_write":
            return self._kv_write(input.get("key", ""), input.get("value", ""))
        else:
            # Check custom registered tools
            schema_raw = host.kv_get(f"tool_schema:{name}")
            if schema_raw:
                return {"error": f"Tool '{name}' is registered but has no executor"}
            return {"error": f"Unknown tool: {name}"}

    @handler("register_tool")
    def register_tool(self, name: str = "", description: str = "", schema: dict = None) -> dict:
        """Register a custom tool with its JSON Schema."""
        if not name:
            return {"error": "tool name is required"}
        if schema:
            host.kv_put(f"tool_schema:{name}", json.dumps(schema))
        host.kv_put(f"tool_desc:{name}", description or "")
        return {"status": "ok", "tool": name}

    @handler("list_tools")
    def list_tools(self) -> dict:
        """Return all available tools with descriptions and schemas."""
        tools = []
        for name, defn in _BUILTIN_TOOLS.items():
            tools.append({
                "name": name,
                "description": defn["description"],
                "schema": defn["schema"],
            })
        return {"status": "ok", "tools": tools, "count": len(tools)}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "total_executions": self.total_executions,
            "total_rejections": self.total_rejections,
        }

    # ------------------------------------------------------------------

    def _web_search(self, query: str, num_results: int = 3) -> dict:
        """Mock web search — returns deterministic results for eval testing."""
        results = [
            {
                "title": f"Result {i+1} for: {query[:40]}",
                "url": f"https://example.com/result-{i+1}",
                "snippet": f"This is a relevant snippet about {query[:30]} from result {i+1}.",
            }
            for i in range(min(num_results, 3))
        ]
        return {"status": "ok", "query": query, "results": results}

    def _calculator(self, expression: str) -> dict:
        """Safe expression evaluator — restricted to arithmetic."""
        try:
            # Restrict to safe math operations
            allowed_chars = set("0123456789+-*/()., ")
            if not all(c in allowed_chars for c in expression):
                return {"error": f"Invalid expression: contains unsafe characters"}
            result = eval(expression, {"__builtins__": {}})  # noqa: S307
            return {"status": "ok", "expression": expression, "result": result}
        except Exception as e:
            return {"error": f"Calculation failed: {e}"}

    def _kv_read(self, key: str) -> dict:
        try:
            value = host.kv_get(f"tool_kv:{key}")
            return {"status": "ok", "key": key, "value": value}
        except Exception as e:
            return {"error": str(e)}

    def _kv_write(self, key: str, value: str) -> dict:
        try:
            host.kv_put(f"tool_kv:{key}", value)
            return {"status": "ok", "key": key}
        except Exception as e:
            return {"error": str(e)}
