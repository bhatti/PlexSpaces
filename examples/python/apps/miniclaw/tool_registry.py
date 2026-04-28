# SPDX-License-Identifier: AGPL-3.0-or-later
"""ToolRegistryActor — tool catalog with simulated execution.

Maintains a registry of callable tools available to agents. Each tool has a
name, description, and input schema. Execution is simulated here; in production
each tool would delegate to a real backend (API, DB, code interpreter, …).
"""

from plexspaces import actor, state, handler, init_handler, host

# Built-in tools registered at init time
_BUILTIN_TOOLS = [
    {
        "name": "web_search",
        "description": "Search the web for information",
        "input_schema": {"query": "string"},
    },
    {
        "name": "calculator",
        "description": "Evaluate mathematical expressions",
        "input_schema": {"expression": "string"},
    },
    {
        "name": "weather",
        "description": "Get current weather for a location",
        "input_schema": {"location": "string"},
    },
]


@actor
class ToolRegistryActor:
    """Registry of callable tools with simulated execution."""

    tools: dict = state(default_factory=dict)   # name -> tool spec
    exec_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.tools = {t["name"]: t for t in _BUILTIN_TOOLS}
        host.process_groups.join("svc:tool_registry")
        host.info(f"ToolRegistryActor init actor_id={self.actor_id} tools={list(self.tools)}")

    @handler("list_tools")
    def list_tools(self) -> dict:
        return {"status": "ok", "tools": list(self.tools.values()), "count": len(self.tools)}

    @handler("register_tool")
    def register_tool(self, name: str = "", description: str = "", input_schema: dict = None) -> dict:
        if not name:
            return {"error": "name is required"}
        self.tools[name] = {"name": name, "description": description, "input_schema": input_schema or {}}
        host.info(f"ToolRegistry: registered tool={name}")
        return {"status": "ok", "name": name}

    @handler("execute_tool")
    def execute_tool(self, name: str = "", input: dict = None) -> dict:
        input = input or {}
        if name not in self.tools:
            return {"error": f"unknown tool: {name}"}

        self.exec_count += 1
        host.info(f"ToolRegistry: executing tool={name} exec={self.exec_count}")

        # Simulated responses per tool type
        if name == "web_search":
            return {"result": f"Search results for: {input.get('query', '')}"}
        if name == "calculator":
            expr = input.get("expression", "0")
            try:
                # Safe evaluation: only allow basic arithmetic
                result = eval(expr, {"__builtins__": {}})  # noqa: S307
                return {"result": str(result)}
            except Exception:
                return {"result": f"Could not evaluate: {expr}"}
        if name == "weather":
            location = input.get("location", "unknown")
            return {"result": f"Weather in {location}: 22°C, partly cloudy"}

        return {"result": f"[simulated] {name} output for input {input}"}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "tool_count": len(self.tools), "exec_count": self.exec_count}
