# SPDX-License-Identifier: AGPL-3.0-or-later
"""ToolExecutorActor — extensible tool registry with built-in and service-link dispatchers."""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_first, fire_audit, ask

_BUILTIN_TOOLS = [
    {"name": "calculator", "description": "Evaluate math expressions (add, subtract, multiply, divide)", "input_schema": {"type": "object", "properties": {"expression": {"type": "string"}}}},
    {"name": "http_request", "description": "Make HTTP GET/POST requests to external APIs", "input_schema": {"type": "object", "properties": {"method": {"type": "string"}, "url": {"type": "string"}, "body": {"type": "string"}}}},
    {"name": "memory_store", "description": "Store a value in tiered memory (core/reachable/deep)", "input_schema": {"type": "object", "properties": {"key": {"type": "string"}, "value": {"type": "string"}, "tier": {"type": "string"}}}},
    {"name": "memory_recall", "description": "Recall values from memory by query", "input_schema": {"type": "object", "properties": {"query": {"type": "string"}, "scope": {"type": "string"}}}},
    {"name": "list_skills", "description": "List learned skills matching a query", "input_schema": {"type": "object", "properties": {"query": {"type": "string"}}}},
    {"name": "create_cron_job", "description": "Schedule a recurring automated task", "input_schema": {"type": "object", "properties": {"prompt": {"type": "string"}, "schedule": {"type": "string", "enum": ["every_1m", "every_5m", "every_1h", "every_24h"]}}}},
]


@actor
class ToolExecutorActor:
    """Registry and dispatcher for built-in and custom tools."""

    tool_names: list = state(default_factory=list)
    execution_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.process_groups.join("svc:tools")
        # Seed built-in tools into KV
        for tool in _BUILTIN_TOOLS:
            host.kv.put(f"tool_def:{tool['name']}", json.dumps(tool))
        self.tool_names = [t["name"] for t in _BUILTIN_TOOLS]
        host.info(f"ToolExecutorActor init actor_id={self.actor_id} tools={len(self.tool_names)}")

    @handler("list_tools")
    def list_tools(self) -> dict:
        tools = []
        for name in self.tool_names:
            raw = host.kv.get(f"tool_def:{name}")
            if raw:
                try:
                    tools.append(json.loads(raw))
                except Exception:
                    pass
        return {"status": "ok", "tools": tools, "count": len(tools)}

    @handler("register_tool")
    def register_tool(self, name: str = "", description: str = "", input_schema: dict = None, handler_type: str = "builtin") -> dict:
        if not name:
            return {"error": "name is required"}
        tool_def = {"name": name, "description": description, "input_schema": input_schema or {}, "handler_type": handler_type}
        host.kv.put(f"tool_def:{name}", json.dumps(tool_def))
        if name not in self.tool_names:
            self.tool_names.append(name)
        fire_audit("tool_registered", f"name={name} type={handler_type}")
        return {"status": "ok", "tool": name}

    @handler("execute")
    def execute(self, name: str = "", input: dict = None) -> dict:
        input = input or {}
        raw = host.kv.get(f"tool_def:{name}")
        if not raw:
            return {"error": f"unknown tool: {name}"}

        self.execution_count += 1
        host.incr_counter("tool_executions", 1)

        try:
            tool_def = json.loads(raw)
        except Exception:
            return {"error": "corrupted tool definition"}

        handler_type = tool_def.get("handler_type", "builtin")

        if name in ("calculator", "http_request", "memory_store", "memory_recall", "list_skills", "create_cron_job"):
            result = self._dispatch_builtin(name, input)
        elif handler_type == "service_link":
            result = self._dispatch_service_link(name, tool_def, input)
        else:
            result = {"result": f"executed {name} with {input}"}

        fire_audit("tool_executed", f"name={name}")
        return {"status": "ok", "tool": name, "output": result}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "tool_count": len(self.tool_names), "execution_count": self.execution_count}

    # ------------------------------------------------------------------

    def _dispatch_builtin(self, name: str, input: dict) -> dict:
        if name == "calculator":
            expr = input.get("expression", "0")
            result, err = self._eval_expression(expr)
            if err:
                return {"error": err}
            return {"result": result, "expression": expr}

        elif name == "http_request":
            method = input.get("method", "GET")
            url = input.get("url", "")
            body = input.get("body", "")
            try:
                resp = host.http_fetch("external", method, url, body)
                return {"status": 200, "body": str(resp)[:500]}
            except Exception as e:
                return {"error": f"http_request failed: {e}", "url": url}

        elif name == "memory_store":
            mem_id, _ = registry_first("memory", fallback_group="svc:memory")
            if not mem_id:
                return {"error": "memory actor not available"}
            key = input.get("key", "")
            value = input.get("value", "")
            tier = input.get("tier", "core")
            scope = input.get("scope", "global")
            resp = ask(mem_id, "store_memory", {"tier": tier, "key": key, "value": value, "scope": scope})
            return resp or {"error": "memory_store failed"}

        elif name == "memory_recall":
            mem_id, _ = registry_first("memory", fallback_group="svc:memory")
            if not mem_id:
                return {"error": "memory actor not available"}
            query = input.get("query", "")
            scope = input.get("scope", "global")
            resp = ask(mem_id, "recall_memory", {"query": query, "scope": scope})
            return resp or {"error": "memory_recall failed"}

        elif name == "list_skills":
            skill_id, _ = registry_first("skill_store", fallback_group="svc:skills")
            if not skill_id:
                return {"error": "skill store not available"}
            query = input.get("query", "")
            resp = ask(skill_id, "match_skills", {"query": query, "limit": 5})
            return resp or {"skills": [], "count": 0}

        elif name == "create_cron_job":
            cron_id, _ = registry_first("cron_scheduler", fallback_group="svc:cron")
            if not cron_id:
                return {"error": "cron actor not available"}
            prompt = input.get("prompt", "")
            schedule = input.get("schedule", "every_1h")
            import hashlib
            job_id = hashlib.md5(prompt.encode()).hexdigest()[:8]
            resp = ask(cron_id, "create_job", {"job_id": job_id, "prompt": prompt, "schedule": schedule})
            return resp or {"error": "create_cron_job failed"}

        return {"error": f"unknown builtin: {name}"}

    def _dispatch_service_link(self, name: str, tool_def: dict, input: dict) -> dict:
        link = tool_def.get("service_link", name)
        path = tool_def.get("path", "/")
        try:
            resp = host.http_fetch(link, "POST", path, json.dumps(input))
            return {"result": str(resp)[:500]}
        except Exception as e:
            return {"error": f"service_link {link} failed: {e}"}

    def _eval_expression(self, expr: str):
        """Simple arithmetic evaluator — no eval()."""
        expr = expr.strip()
        for op, fn in [("*", lambda a, b: a * b), ("/", lambda a, b: a / b if b != 0 else None),
                       ("+", lambda a, b: a + b), ("-", lambda a, b: a - b)]:
            if op in expr:
                parts = expr.split(op, 1)
                if len(parts) == 2:
                    try:
                        a, b = float(parts[0].strip()), float(parts[1].strip())
                        result = fn(a, b)
                        if result is None:
                            return 0, "division by zero"
                        return result, None
                    except ValueError:
                        pass
        try:
            return float(expr), None
        except ValueError:
            return 0, f"cannot evaluate: {expr}"
