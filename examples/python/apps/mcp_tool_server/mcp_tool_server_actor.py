# SPDX-License-Identifier: AGPL-3.0-or-later
"""MCP Tool Server — Model Context Protocol style tool calling via PlexSpaces actors.

Demonstrates:
- Tool discovery (tools/list)
- Tool execution (tools/call)
- Multi-tenant access control via namespace validation
- Circuit breaker error handling
- KV-backed document search
- Simulated weather data
"""

from __future__ import annotations

import json

from plexspaces import ActorID, actor, event_actor, fsm_actor, handler, host, init_handler, query_handler, run_handler, signal_handler, state, workflow_actor


def actor_application_id(actor_id: str) -> str:
    """Extract the application namespace from a canonical actor id."""
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return actor_id


def pg_first(group: str) -> str | None:
    """Return the first canonical actor ID from a process group, or None."""
    try:
        members = host.process_groups.members(group)
        return members[0] if members else None
    except Exception:
        return None


def sibling_actor_id(my_canonical_id: str, sibling_name: str) -> str:
    """Derive a sibling actor's canonical ID — prefers PG discovery over role-based routing.

    Supervisor-spawned actors have ULID names, so role-based routing would hit
    a new virtual_actor instance with empty KV. PG returns the actual live instance.
    """
    live = pg_first(f"svc:{sibling_name}")
    if live:
        return live
    try:
        return ActorID.parse(my_canonical_id).with_type_and_name(sibling_name, sibling_name).to_str()
    except ValueError:
        return sibling_name


@actor
class ToolRegistryActor:
    """Registry actor that manages tool discovery and routing.

    Maintains a catalogue of available tools, routes tool/call requests to
    the appropriate specialist actor, and tracks per-tool invocation counts
    and error counts for observability.
    """

    tools: dict = state(default_factory=dict)
    invocation_counts: dict = state(default_factory=dict)
    error_counts: dict = state(default_factory=dict)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.invocation_counts = {}
        self.error_counts = {}
        host.process_groups.join("svc:tool_registry")
        self.tools = {
            "calculator": {
                "name": "calculator",
                "description": "Perform arithmetic operations",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["add", "subtract", "multiply", "divide"],
                        },
                        "a": {"type": "number"},
                        "b": {"type": "number"},
                    },
                    "required": ["operation", "a", "b"],
                },
            },
            "search": {
                "name": "search",
                "description": "Search documents by keyword",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "max_results": {"type": "integer", "default": 5},
                    },
                    "required": ["query"],
                },
            },
            "weather": {
                "name": "weather",
                "description": "Get weather for a location",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"},
                        "units": {
                            "type": "string",
                            "enum": ["celsius", "fahrenheit"],
                            "default": "celsius",
                        },
                    },
                    "required": ["location"],
                },
            },
        }
        host.info(f"ToolRegistryActor initialised with {len(self.tools)} built-in tools")

    @handler("tools_list")
    def tools_list(self) -> dict:
        """Return all registered tools (MCP tools/list response)."""
        return {"tools": list(self.tools.values()), "count": len(self.tools)}

    @handler("tools_call")
    def tools_call(self, tool_name: str = "", input: dict = None) -> dict:  # noqa: A002
        """Execute a named tool with the provided input.

        Validates existence and required fields, routes to the appropriate
        specialist actor via host.ask(), tracks counts, and handles errors
        with a circuit-breaker-style error response.
        """
        if input is None:
            input = {}  # noqa: A001

        if tool_name not in self.tools:
            return {
                "error": "tool_not_found",
                "tool": tool_name,
                "message": f"Tool '{tool_name}' is not registered",
                "available_tools": list(self.tools.keys()),
            }

        schema = self.tools[tool_name]
        required_fields = schema.get("inputSchema", {}).get("required", [])
        missing = [f for f in required_fields if f not in input]
        if missing:
            return {
                "error": "missing_required_fields",
                "tool": tool_name,
                "missing": missing,
                "message": f"Required fields missing: {missing}",
            }

        # Map tool name to sibling actor role name, then build canonical ID
        target_role_map = {
            "calculator": "calculator_tool",
            "search": "search_tool",
            "weather": "weather_tool",
        }
        target_role = target_role_map.get(tool_name, tool_name)
        target_actor = sibling_actor_id(self.actor_id, target_role)

        self.invocation_counts[tool_name] = self.invocation_counts.get(tool_name, 0) + 1

        audit_actor_id = sibling_actor_id(self.actor_id, "tool_audit")
        call_start_ms = host.now_ms()

        try:
            result = host.ask(target_actor, "execute", input, timeout_ms=10000)
            latency_ms = host.now_ms() - call_start_ms
            # Report metrics
            try:
                host.application_metrics_add(
                    self.application_id,
                    {
                        "message_count": 1,
                        "counter_metrics": {
                            "tools_called": 1,
                            f"tool_{tool_name}_calls": 1,
                        },
                    },
                )
            except Exception:
                pass
            # Fire audit event (fire-and-forget)
            try:
                host.send(audit_actor_id, "tool_invoked", {
                    "tool_name": tool_name,
                    "success": True,
                    "latency_ms": latency_ms,
                })
            except Exception:
                pass
            return result
        except Exception as exc:
            self.error_counts[tool_name] = self.error_counts.get(tool_name, 0) + 1
            host.warn(f"Tool execution failed for '{tool_name}': {exc}")
            try:
                host.application_metrics_add(
                    self.application_id,
                    {
                        "message_count": 1,
                        "counter_metrics": {
                            "tools_errors": 1,
                            f"tool_{tool_name}_errors": 1,
                        },
                    },
                )
            except Exception:
                pass
            # Fire audit event for failure (fire-and-forget)
            try:
                host.send(audit_actor_id, "tool_invoked", {
                    "tool_name": tool_name,
                    "success": False,
                    "latency_ms": 0,
                })
            except Exception:
                pass
            return {
                "error": "tool_execution_failed",
                "tool": tool_name,
                "message": str(exc),
            }

    @handler("register_tool")
    def register_tool(self, tool_schema: dict = None) -> dict:
        """Dynamically register a new tool in the registry."""
        if not tool_schema:
            return {"error": "missing_tool_schema"}
        name = tool_schema.get("name", "")
        if not name:
            return {"error": "tool_schema_missing_name"}
        self.tools[name] = tool_schema
        host.info(f"Registered new tool: {name}")
        return {"ok": True, "registered": name, "total_tools": len(self.tools)}

    @handler("get_stats")
    def get_stats(self) -> dict:
        """Return per-tool invocation and error counts."""
        stats = {}
        all_tools = set(list(self.invocation_counts.keys()) + list(self.error_counts.keys()))
        for tool in all_tools:
            stats[tool] = {
                "invocations": self.invocation_counts.get(tool, 0),
                "errors": self.error_counts.get(tool, 0),
            }
        return {
            "stats": stats,
            "total_invocations": sum(self.invocation_counts.values()),
            "total_errors": sum(self.error_counts.values()),
        }


@actor
class CalculatorToolActor:
    """Performs arithmetic operations for the MCP calculator tool."""

    calculations_done: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.calculations_done = 0
        host.process_groups.join("svc:calculator_tool")

    @handler("execute")
    def execute(self, operation: str = "", a: float = 0.0, b: float = 0.0) -> dict:
        """Execute an arithmetic operation and return the result."""
        if not isinstance(a, (int, float)):
            try:
                a = float(a)
            except (TypeError, ValueError):
                return {"error": "invalid_operand", "message": "operand 'a' must be numeric"}
        if not isinstance(b, (int, float)):
            try:
                b = float(b)
            except (TypeError, ValueError):
                return {"error": "invalid_operand", "message": "operand 'b' must be numeric"}

        a = float(a)
        b = float(b)

        if operation == "add":
            result = a + b
        elif operation == "subtract":
            result = a - b
        elif operation == "multiply":
            result = a * b
        elif operation == "divide":
            if b == 0.0:
                return {
                    "error": "division_by_zero",
                    "tool": "calculator",
                    "message": "Cannot divide by zero",
                    "operation": operation,
                    "a": a,
                    "b": b,
                }
            result = a / b
        else:
            return {
                "error": "unknown_operation",
                "message": f"Unknown operation '{operation}'. Supported: add, subtract, multiply, divide",
            }

        self.calculations_done += 1
        return {
            "result": result,
            "operation": operation,
            "a": a,
            "b": b,
        }


@actor
class SearchToolActor:
    """Document search actor backed by KV store."""

    search_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.search_count = 0
        host.process_groups.join("svc:search_tool")
        # Seed KV store with sample documents
        sample_docs = [
            {"id": "doc1", "content": "PlexSpaces is an actor framework for distributed systems"},
            {"id": "doc2", "content": "Actors provide location-transparent message passing"},
            {"id": "doc3", "content": "WASM enables polyglot actor deployment across languages"},
            {"id": "doc4", "content": "Shard groups enable parallel data processing at scale"},
            {"id": "doc5", "content": "TupleSpace coordinates distributed actors via shared state"},
            {"id": "doc6", "content": "GenServer actors handle request-reply patterns efficiently"},
            {"id": "doc7", "content": "Workflow actors support durable long-running processes"},
            {"id": "doc8", "content": "Supervisors provide fault-tolerant actor hierarchies"},
            {"id": "doc9", "content": "MCP enables LLMs to call tools via structured JSON-RPC"},
            {"id": "doc10", "content": "Circuit breakers prevent cascading failures in microservices"},
        ]
        for doc in sample_docs:
            host.kv.put(f"doc:{doc['id']}", json.dumps(doc))
        host.info(f"SearchToolActor seeded {len(sample_docs)} documents in KV store")

    @handler("execute")
    def execute(self, query: str = "", max_results: int = 5) -> dict:
        """Search documents matching the query keywords."""
        if not query:
            return {"error": "missing_query", "message": "query must not be empty"}

        if not isinstance(max_results, int):
            try:
                max_results = int(max_results)
            except (TypeError, ValueError):
                max_results = 5
        max_results = max(1, min(max_results, 50))

        query_words = [w.lower() for w in query.split() if w]

        # Retrieve all doc keys
        raw_keys = host.kv.list("doc:")
        try:
            keys = json.loads(raw_keys) if raw_keys else []
        except (json.JSONDecodeError, TypeError):
            keys = []

        results = []
        for key in keys:
            raw_doc = host.kv.get(key)
            if not raw_doc:
                continue
            try:
                doc = json.loads(raw_doc)
            except (json.JSONDecodeError, TypeError):
                continue
            content_lower = doc.get("content", "").lower()
            if any(word in content_lower for word in query_words):
                results.append({"id": doc.get("id", key), "content": doc.get("content", "")})
            if len(results) >= max_results:
                break

        self.search_count += 1
        return {
            "results": results,
            "count": len(results),
            "query": query,
        }


@actor
class WeatherToolActor:
    """Returns simulated weather data for requested locations."""

    weather_calls: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.weather_calls = 0
        host.process_groups.join("svc:weather_tool")

    @handler("execute")
    def execute(self, location: str = "", units: str = "celsius") -> dict:
        """Return deterministic simulated weather for a location."""
        if not location:
            return {"error": "missing_location", "message": "location must not be empty"}

        # Deterministic simulated weather based on location string hash
        loc_hash = hash(location)
        temp_c = (loc_hash % 35) + 5  # 5-40 °C range
        conditions = ["sunny", "cloudy", "rainy", "windy"]
        condition = conditions[loc_hash % len(conditions)]
        humidity = (loc_hash % 40) + 40  # 40-80% range

        if units == "fahrenheit":
            temp = temp_c * 9 / 5 + 32
        else:
            temp = temp_c
            units = "celsius"

        self.weather_calls += 1
        return {
            "location": location,
            "temperature": temp,
            "units": units,
            "condition": condition,
            "humidity": humidity,
        }


@workflow_actor(facets=["virtual_actor", "durability"])
class MCPGatewayWorkflow:
    """MCP-protocol gateway workflow.

    Accepts JSON-RPC 2.0 style requests, routes them to the ToolRegistryActor,
    applies lightweight tenant namespace validation, and returns MCP-compliant
    responses.
    """

    session_id: str = state(default="")
    requests_processed: int = state(default=0)
    last_error: str = state(default="")
    actor_id: str = state(default="")

    @run_handler
    def start(self, request: dict = None) -> dict:
        """Process an MCP JSON-RPC request.

        Supports methods: tools/list, tools/call.
        Validates tenant namespace when the 'tenant' field is present.
        """
        if request is None:
            request = {}

        self.actor_id = host.self_id()
        if not self.session_id:
            self.session_id = f"session-{host.now_ms()}"

        request_id = request.get("id", 0)
        method = request.get("method", "")
        params = request.get("params", {})
        if not isinstance(params, dict):
            params = {}

        # Tenant namespace validation
        tenant = request.get("tenant", "")
        if tenant:
            self_ns = actor_application_id(self.actor_id)
            if self_ns and tenant != self_ns:
                self.last_error = f"tenant mismatch: got {tenant}, expected {self_ns}"
                return {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32600,
                        "message": f"Tenant namespace mismatch: '{tenant}' is not authorised for namespace '{self_ns}'",
                    },
                }

        registry_actor_id = sibling_actor_id(self.actor_id, "tool_registry")

        try:
            if method == "tools/list":
                result = host.ask(registry_actor_id, "tools_list", {}, timeout_ms=10000)
            elif method == "tools/call":
                tool_name = params.get("name", "")
                tool_input = params.get("arguments", params.get("input", {}))
                result = host.ask(
                    registry_actor_id,
                    "tools_call",
                    {"tool_name": tool_name, "input": tool_input},
                    timeout_ms=15000,
                )
            else:
                self.requests_processed += 1
                self.last_error = f"unknown method: {method}"
                return {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32601,
                        "message": f"Method not found: '{method}'. Supported: tools/list, tools/call",
                    },
                }
        except Exception as exc:
            self.requests_processed += 1
            self.last_error = str(exc)
            host.error(f"MCPGateway error processing '{method}': {exc}")
            return {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {"code": -32603, "message": str(exc)},
            }

        self.requests_processed += 1
        self.last_error = ""
        return {"jsonrpc": "2.0", "id": request_id, "result": result}

    @signal_handler("reset")
    def reset(self, reason: str = "manual") -> None:
        """Reset session state (circuit-breaker reset or manual reset)."""
        host.info(f"MCPGatewayWorkflow reset: reason={reason}")
        self.requests_processed = 0
        self.last_error = ""
        self.session_id = f"session-{host.now_ms()}"

    @query_handler("stats")
    def stats(self) -> dict:
        """Return workflow statistics."""
        return {
            "session_id": self.session_id,
            "requests_processed": self.requests_processed,
            "last_error": self.last_error,
            "actor_id": self.actor_id,
        }


@event_actor
class ToolAuditEventActor:
    """GenEvent actor: audit trail for all tool invocations (fire-and-forget)."""

    audit_count: int = state(default=0)
    recent_events: list = state(default_factory=list)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        host.process_groups.join("tool-audit")
        host.process_groups.join("svc:tool_audit")

    @handler("tool_invoked", "cast")
    def on_tool_invoked(
        self,
        tool_name: str = "",
        tenant: str = "default",
        success: bool = True,
        latency_ms: int = 0,
        from_actor: str = "",
    ) -> None:
        self.audit_count += 1
        event = {
            "tool": tool_name,
            "tenant": tenant,
            "success": success,
            "latency_ms": latency_ms,
            "ts_ms": host.now_ms(),
        }
        self.recent_events = (self.recent_events + [event])[-20:]  # keep last 20
        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "audit_events": 1,
                        f"tool_{tool_name}_calls": 1,
                    },
                    "latency_totals_ms": {"tool_execution": latency_ms},
                    "latency_max_ms": {"tool_execution": latency_ms},
                    "latency_samples": {"tool_execution": 1},
                },
            )
        except Exception:
            pass

    @handler("get_audit_log")
    def get_audit_log(self, limit: int = 10, from_actor: str = "") -> dict:
        return {
            "audit_count": self.audit_count,
            "recent": self.recent_events[-limit:],
        }


@fsm_actor(states=["healthy", "degraded", "circuit_open"], initial="healthy")
class ToolCircuitBreakerFSM:
    """FSM actor: circuit-breaker protecting tool servers from cascading failures."""

    failure_count: int = state(default=0)
    fsm_state: str = state(default="healthy")
    degraded_threshold: int = state(default=2)
    open_threshold: int = state(default=5)
    monitored_tool: str = state(default="")
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        args = config.get("args", {})
        self.monitored_tool = args.get("monitored_tool", "all")
        self.degraded_threshold = int(args.get("degraded_threshold", 2))
        self.open_threshold = int(args.get("open_threshold", 5))

    @handler("record_failure")
    def record_failure(self, tool_name: str = "", from_actor: str = "") -> dict:
        self.failure_count += 1
        if self.failure_count >= self.open_threshold:
            self.fsm_state = "circuit_open"
            host.send_after(10000, "attempt_recovery", {})
        elif self.failure_count >= self.degraded_threshold:
            self.fsm_state = "degraded"
        return {"state": self.fsm_state, "failures": self.failure_count}

    @handler("record_success")
    def record_success(self, tool_name: str = "", from_actor: str = "") -> dict:
        if self.fsm_state == "circuit_open":
            pass  # wait for attempt_recovery signal
        else:
            self.failure_count = max(0, self.failure_count - 1)
            if self.failure_count < self.degraded_threshold:
                self.fsm_state = "healthy"
        return {"state": self.fsm_state, "failures": self.failure_count}

    @handler("attempt_recovery")
    def attempt_recovery(self, from_actor: str = "") -> dict:
        if self.fsm_state == "circuit_open":
            self.fsm_state = "degraded"
            self.failure_count = self.degraded_threshold
        return {"state": self.fsm_state}

    @handler("get_health")
    def get_health(self, from_actor: str = "") -> dict:
        return {
            "state": self.fsm_state,
            "failures": self.failure_count,
            "allowed": self.fsm_state != "circuit_open",
            "tool": self.monitored_tool,
        }


