# MCP Tool Server — PlexSpaces Python WASM Example

A Model Context Protocol (MCP) style tool server implemented with PlexSpaces actors in Python WASM. Demonstrates tool discovery, structured tool execution, multi-tenant access control, and circuit-breaker error handling — all within the actor model.

## What it Demonstrates

- **MCP tool discovery** (`tools/list`) — enumerate available tools with JSON Schema descriptors
- **MCP tool execution** (`tools/call`) — route structured requests to specialist actors
- **Dynamic tool registration** (`register_tool`) — add new tools at runtime without redeployment
- **Per-tool observability** (`get_stats`) — invocation and error counts per tool
- **Multi-tenant namespace validation** — the gateway validates `tenant` fields on incoming requests
- **Circuit-breaker error handling** — failed tool calls return structured JSON errors without crashing the registry
- **KV-backed document search** — documents seeded into the KV store at init time
- **Deterministic simulated weather** — no external HTTP dependency; hash-based simulation
- **Workflow actor as gateway** — `MCPGatewayWorkflow` uses `@run_handler`, `@signal_handler`, `@query_handler`

## Architecture

```
MCPGatewayWorkflow          (mcp_gateway — Workflow actor)
       │
       │  host.ask()
       ▼
ToolRegistryActor           (tool_registry — regular actor)
  ├── host.ask() ──► CalculatorToolActor  (calculator_tool)
  ├── host.ask() ──► SearchToolActor      (search_tool)
  └── host.ask() ──► WeatherToolActor     (weather_tool)
```

All actors are supervised under a `one_for_one` supervisor. Each specialist actor is independent and restartable.

## Actors

| Actor | Role | Description |
|---|---|---|
| `ToolRegistryActor` | `tool_registry` | Catalogue, routing, stats, dynamic registration |
| `CalculatorToolActor` | `calculator_tool` | add / subtract / multiply / divide |
| `SearchToolActor` | `search_tool` | Keyword search over KV-backed documents |
| `WeatherToolActor` | `weather_tool` | Deterministic simulated weather data |
| `MCPGatewayWorkflow` | `mcp_gateway` | JSON-RPC 2.0 gateway with tenant validation |

## Prerequisites

- PlexSpaces node(s) running (default ports 8092, 8094)
- Python 3.11+ with `plexspaces` SDK installed
- `plexspaces-py` CLI on `$PATH`

## Build

```bash
./build.sh
```

Produces `mcp_tool_server_actor.wasm`.

## Run Tests

```bash
# Against default nodes (localhost:8092 and localhost:8094)
./test.sh

# Single node
./test.sh 8092

# Specific addresses
./test.sh localhost:8092 localhost:8094
```

The test script:
1. Builds the WASM binary if not present
2. Deploys the application to all specified nodes
3. Runs 7 integration scenarios (see below)
4. Cleans up on exit

## Test Scenarios

| # | Scenario | What is verified |
|---|---|---|
| 1 | `tools_list` | 3 built-in tools returned with correct names |
| 2 | `tools_call` calculator `add(10, 5)` | `result == 15` |
| 3 | `tools_call` calculator divide-by-zero | `error == "division_by_zero"` |
| 4 | `tools_call` search `query="actor"` | At least 1 actor-related document returned |
| 5 | `tools_call` weather `London celsius` | `temperature`, `condition`, `humidity` fields present |
| 6 | `get_stats` | Invocation counts match number of calls made |
| 7 | `register_tool` + `tools_list` | New tool appears in subsequent list |

## MCP Request Format

The `ToolRegistryActor` accepts direct actor asks:

```json
{ "op": "tools_list" }
{ "op": "tools_call", "tool_name": "calculator", "input": {"operation": "add", "a": 10, "b": 5} }
{ "op": "register_tool", "tool_schema": { "name": "...", "description": "...", "inputSchema": {...} } }
{ "op": "get_stats" }
```

The `MCPGatewayWorkflow` accepts JSON-RPC 2.0 style requests:

```json
{ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }
{ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "calculator", "arguments": {"operation": "add", "a": 1, "b": 2} } }
{ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "calculator", "arguments": {"operation": "add", "a": 1, "b": 2} }, "tenant": "my-namespace" }
```

## Key PlexSpaces Patterns Used

- `@actor` / `@workflow_actor` decorators for actor class definition
- `state(default=...)` / `state(default_factory=...)` for persistent actor state
- `@init_handler` for actor initialisation
- `@handler("name")` for regular message handlers
- `@run_handler` / `@signal_handler` / `@query_handler` for workflow lifecycle
- `host.ask()` for request-reply between actors
- `host.kv_put()` / `host.kv_get()` / `host.kv_list()` for KV store access
- `host.application_metrics_add()` for structured metrics emission
- `host.info()` / `host.warn()` / `host.error()` for structured logging

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design](../../../../docs/detailed-design.md)
- [Python SDK](../../../../sdks/python/)
