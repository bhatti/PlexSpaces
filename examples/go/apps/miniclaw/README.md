# MiniClaw — Mini Agent Framework on PlexSpaces

MiniClaw demonstrates how PlexSpaces actor primitives power an agentic AI framework
inspired by OpenClaw, NanoClaw, and MicroClaw. It implements 10 actors covering the
core abstractions of any production agent system: LLM routing, tool execution, the
agent loop, session management, multi-agent orchestration, scoped memory, audit
trails, lifecycle state machines, a durable task queue, and a health monitor.

## Architecture

```mermaid
graph TB
    User([User / HTTP API])

    subgraph GenServer["GenServer Actors (request-reply)"]
        Agent["AgentActor\nCore loop · KV session history"]
        LLM["LLMRouterActor\nPhantom token · Prompt cache · Circuit breaker"]
        Tools["ToolRegistryActor\nSchema registry · Built-in execution"]
        Memory["MemoryActor\nKV + TupleSpace · Scoped recall"]
        Session["SessionManagerActor\nKV lifecycle · channel+user index"]
        TaskQ["TaskQueueActor\nKV-backed queue · enqueue/dequeue/ack/nack"]
        Health["HealthMonitorActor\nSendAfter polling · PG health snapshots"]
    end

    subgraph Other["Other Behaviors"]
        Orch["OrchestratorActor\n(WorkflowActor)\nDurable · Checkpointed · TupleSpace results"]
        Audit["AuditEventActor\n(GenEvent)\nFire-and-forget · Watermark · Two-cursor poll"]
        FSM["AgentStateFSM\n(GenFSM)\nidle→processing→tool_executing→responding"]
    end

    subgraph PG["Process Groups (svc:*)"]
        PG1["svc:agent"]
        PG2["svc:llm_router"]
        PG3["svc:tool_registry"]
        PG4["svc:memory"]
        PG5["svc:session_manager"]
        PG6["svc:audit"]
        PG7["svc:agent_fsm"]
        PG8["svc:task_queue"]
        PG9["svc:health_monitor"]
    end

    User -->|Ask: chat| Agent
    User -->|Ask: create/get/end session| Session
    User -->|Ask: workflow_run/query/signal| Orch
    User -->|Ask: store/recall| Memory
    User -->|Ask: register/list/execute| Tools
    User -->|Ask: stats / reset_circuit| LLM
    User -->|Ask: query_events / poll_events / get_stats| Audit
    User -->|Ask: enqueue / dequeue / ack / nack| TaskQ
    User -->|Ask: get_health / get_stats| Health

    Agent -->|Ask: chat_completion| LLM
    Agent -->|Ask: list_tools / execute_tool| Tools
    Agent -->|Send: log_event| Audit
    Agent -->|Send: transition| FSM
    Agent -->|KVPut: session_history| Agent

    Tools -->|Ask: recall_memory| Memory

    Orch -->|Ask: chat via svc:agent PG| Agent
    Orch -->|TS.Write: orch_result checkpoints| Orch

    LLM -->|KVGet/KVPut: llm_cache, cred:token| LLM
    LLM -->|SendAfter 30s: timer_tick| LLM
    Memory -->|KVPut/TS.Write: mem:scope:id:key| Memory
    Health -->|SendAfter: poll_tick| Health
    Health -->|PG.Members: serviceGroups| Health
    Health -->|TS.Write: health_snapshot| Health

    Agent -.->|joins| PG1
    LLM -.->|joins| PG2
    Tools -.->|joins| PG3
    Memory -.->|joins| PG4
    Session -.->|joins| PG5
    Audit -.->|joins| PG6
    FSM -.->|joins| PG7
    TaskQ -.->|joins| PG8
    Health -.->|joins| PG9

    style User fill:#E65100,color:#fff,stroke:#BF360C,stroke-width:3px
    style Agent fill:#1565C0,color:#fff,stroke:#0D47A1,stroke-width:2px
    style LLM fill:#6A1B9A,color:#fff,stroke:#4A148C,stroke-width:2px
    style Tools fill:#2E7D32,color:#fff,stroke:#1B5E20,stroke-width:2px
    style Memory fill:#00838F,color:#fff,stroke:#006064,stroke-width:2px
    style Session fill:#283593,color:#fff,stroke:#1A237E,stroke-width:2px
    style TaskQ fill:#4E342E,color:#fff,stroke:#3E2723,stroke-width:2px
    style Health fill:#558B2F,color:#fff,stroke:#33691E,stroke-width:2px
    style Orch fill:#1565C0,color:#fff,stroke:#0D47A1,stroke-width:2px
    style Audit fill:#AD1457,color:#fff,stroke:#880E4F,stroke-width:2px
    style FSM fill:#4E342E,color:#fff,stroke:#3E2723,stroke-width:2px
    style GenServer fill:#0D1B2A,color:#90CAF9,stroke:#1565C0,stroke-width:2px
    style Other fill:#0D1B2A,color:#CE93D8,stroke:#6A1B9A,stroke-width:2px
    style PG fill:#1A2F1A,color:#A5D6A7,stroke:#2E7D32,stroke-width:1px,stroke-dasharray:4 3
```

<details>
<summary>ASCII fallback</summary>

```
User (HTTP API)
    │
    ├─Ask──→ AgentActor ──Ask──→ LLMRouterActor (chat_completion, phantom token resolved here)
    │              │                     │
    │              │              ←── response (tool_use or end_turn)
    │              │
    │              ├──Ask──→ ToolRegistryActor (execute_tool)
    │              │              │
    │              │              ├── (memory_search) ──Ask──→ MemoryActor
    │              │              │
    │              │              ←── tool output
    │              │
    │              ├──Send─→ AuditEventActor (fire-and-forget, watermark+cursor)
    │              ├──Send─→ AgentStateFSM  (state transitions)
    │              │
    │              ←── final response to user
    │
    ├──Ask──→ SessionManagerActor (create/get/end session)
    ├──Ask──→ MemoryActor         (store/recall/list)
    ├──Ask──→ OrchestratorActor   (workflow_run → delegates to AgentActor)
    ├──Ask──→ ToolRegistryActor   (register/list/execute tools)
    ├──Ask──→ LLMRouterActor      (register_credential, stats, reset_circuit)
    ├──Ask──→ AuditEventActor     (query_events, poll_events, stats)
    ├──Ask──→ TaskQueueActor      (enqueue, dequeue, ack, nack, get_stats)
    └──Ask──→ HealthMonitorActor  (get_health, get_stats)
```
</details>

## Actors

| Actor | Behavior | Purpose | PlexSpaces Primitives |
|-------|----------|---------|----------------------|
| `LLMRouterActor` | GenServer | Simulated LLM with phantom-token credential proxy, prompt caching, and circuit breaker | KV (cache, credentials), PG (svc:llm_router), SendAfter (timer recovery) |
| `ToolRegistryActor` | GenServer | Tool registration and built-in execution (calculator, weather, memory, web search) | KV (tool defs + tool_names index), PG (svc:tool_registry) |
| `AgentActor` | GenServer | Core agent loop: message → LLM → tool_use → execute → repeat | Ask (LLM + tools), Send (FSM + audit), KV (session history), PG (svc:agent) |
| `SessionManagerActor` | GenServer | Session lifecycle with channel+user mapping | KV (session metadata), PG (svc:session_manager) |
| `OrchestratorActor` | WorkflowActor | Durable multi-agent task decomposition and delegation | Ask (agents), TupleSpace (result coordination), PG (svc:agent) |
| `MemoryActor` | GenServer | Scoped memory (global/agent/session) | KV (persistent), TupleSpace (queryable), PG (svc:memory) |
| `AuditEventActor` | GenEvent | Fire-and-forget audit trail with monotonic watermark and two-cursor polling | TupleSpace (audit log), KV (per-seq entries + cursors), PG (svc:audit) |
| `AgentStateFSM` | GenFSM | Agent processing lifecycle state machine | PG (svc:agent_fsm) |
| `TaskQueueActor` | GenServer | Durable task queue backed by KV — no external broker | KV (queue:pending:seq), PG (svc:task_queue) |
| `HealthMonitorActor` | GenServer | Periodic PG membership polling; writes health snapshots | SendAfter (poll interval), PG.Members (all service groups), TupleSpace (snapshots), PG (svc:health_monitor) |

## NanoClaw Patterns Demonstrated

### Pattern 1 — Phantom Token / Credential Proxy

Callers never see real API keys. They supply an opaque phantom token; `LLMRouterActor` resolves it to the real credential internally and discards the resolved key before any response leaves the actor.

```go
// Register a credential — token is the only handle callers get
actor.Handle("", "register_credential",
    `{"op":"register_credential","phantom_token":"tok_abc","api_key":"sk-real-key"}`)

// Callers supply only the token; the real key never appears in payloads
actor.Handle("", "chat_completion",
    `{"op":"chat_completion","phantom_token":"tok_abc","messages":[...]}`)
```

### Pattern 3 — Two-Cursor / Watermark Audit Polling

`AuditEventActor` assigns a monotonically increasing `Watermark` to every event. Each consumer tracks its own cursor in KV. `poll_events` returns everything from `(cursor+1)` to `Watermark` and advances the cursor atomically — no events are missed, no duplicates, no shared offset.

```go
// Consumer polls from its last-seen cursor
actor.Handle("", "poll_events",
    `{"op":"poll_events","consumer_id":"compliance-agent","limit":50}`)
// → {"events": [...], "cursor": 42, "watermark": 42}
```

### Pattern 4 — KV as Message Queue (TaskQueueActor)

Tasks are stored under `queue:pending:<seq>` in KV. No external broker required. Producers call `enqueue`; consumers call `dequeue` (marks tasks `in_flight`), then `ack` (deletes + advances head) or `nack` (resets to `pending` for redelivery).

```go
actor.Handle("", "enqueue", `{"op":"enqueue","task_type":"summarize","payload":{"doc":"..."}}`
// → {"status":"ok","seq":1}
actor.Handle("", "dequeue", `{"op":"dequeue","limit":1}`)
// → {"tasks":[{"seq":1,"status":"in_flight",...}],"count":1}
actor.Handle("", "ack", `{"op":"ack","seq":1}`)
```

### Pattern 5 — Polling Over Events (HealthMonitorActor)

`HealthMonitorActor` never subscribes to PG change events. Instead it calls `SendAfter` at the end of every `poll_tick` to schedule the next poll. Simple polling eliminates subscription races and is correct when sub-second latency is not required.

```go
// Each tick reschedules the next one — no external scheduler, no event subscription
func (h *HealthMonitorActor) doPoll() string {
    for _, grp := range serviceGroups {
        members, _ := host.PG().Members(grp)
        h.GroupHealth[grp] = len(members)
    }
    _ = host.SendAfter(h.PollInterval, "poll_tick", map[string]any{"op": "poll_tick"})
    return marshal(map[string]any{"status": "ok", "poll_count": h.PollCount})
}
```

## Additional Key Patterns

5. **Agent loop** — `AgentActor.chat()` implements the agentic message → LLM → tool → repeat cycle
6. **Tool calling** — `ToolRegistryActor` dispatches to built-in handlers; memory_search delegates via PG
7. **Circuit breaker** — `LLMRouterActor` opens after 3 consecutive failures; auto-recovers via timer
8. **Prompt caching** — LLM responses cached in KV by message hash; hit ratio tracked
9. **Multi-agent orchestration** — `OrchestratorActor.Run()` decomposes tasks and delegates via PG discovery
10. **Session persistence** — conversation history stored in KV per session_id; compacted when over limit
11. **Scoped memory** — `MemoryActor` namespaces by scope (global/agent/session) via KV + TupleSpace
12. **Capability discovery** — all actors join PGs on init; clients call `pgFirst()` for location-transparent routing
13. **FSM lifecycle** — `AgentStateFSM` enforces valid state transitions: idle→processing→tool_executing→responding→idle

## PlexSpaces Primitives

| Primitive | How Used in MiniClaw |
|-----------|---------------------|
| `host.Ask()` | Request-reply: agent→LLM, agent→tools, orchestrator→agent |
| `host.Send()` | Fire-and-forget: all actors→audit, agent→FSM state updates |
| `host.SendAfter()` | Delayed timer: LLMRouter circuit recovery (30s), HealthMonitor poll ticks |
| `host.KVGet/KVPut/KVDelete()` | LLM prompt cache, credentials, tool registry, sessions, memory, task queue |
| `host.TS().Write/ReadAll()` | Agent info, orchestration results, audit events, memory tuples, health snapshots |
| `host.PG().Join/Members()` | Service discovery: every actor joins a named PG on init; HealthMonitor polls all PGs |
| `BaseActor` | JSON state serialization for durable checkpointing |
| `WorkflowActor` | Durable `Run()`/`Signal()`/`Query()` for OrchestratorActor |

## Message Flow Example: "What is 42 * 17?"

```
1. User → agent: chat {message: "Please calculate 42 * 17", session_id: "s1"}
2. AgentActor → tool_registry: list_tools {}
   ← [{name: "calculator", ...}, ...]
3. AgentActor → agent_fsm: transition {to: "processing"}
4. AgentActor → llm_router: chat_completion {messages: [...], tools: [...]}
   ← {stop_reason: "tool_use", tool_calls: [{name: "calculator", input: {expression: "42 * 17"}}]}
5. AgentActor appends assistant message to history
6. AgentActor → agent_fsm: transition {to: "tool_executing"}
7. AgentActor → tool_registry: execute_tool {name: "calculator", input: {expression: "42 * 17"}}
   ← {status: "ok", output: {result: 714, expression: "42 * 17"}}
8. AgentActor appends tool result to history
9. AgentActor → audit_event: log_event {event_type: "tool_called", detail: "tool=calculator"}
10. AgentActor → agent_fsm: transition {to: "processing"}
11. AgentActor → llm_router: chat_completion {messages: [...with tool result...], tools: [...]}
    ← {stop_reason: "end_turn", content: "Tool results: calculator: ...result=714..."}
12. AgentActor → agent_fsm: transition {to: "idle"}
13. AgentActor → KV: put session_history:s1
14. User ← {status: "ok", response: "Tool results: calculator: ...result=714...", loop_iterations: 2}
```

## Build & Test

```bash
# Build WASM binary
./build.sh

# Run contract tests (no node required)
go test ./...

# Run integration tests against a live node
./test.sh 8092        # defaults to port 8092
```

**Prerequisites:**
- `tinygo` — `brew tap tinygo-org/tools && brew install tinygo`
- `wasm-tools` — `cargo install wasm-tools`
- `jco` — `npm install -g @bytecodealliance/jco`
- A running PlexSpaces node: `cargo run --release -- --http-port 8092 --cluster-port 8091`

## API Reference

### LLMRouterActor

| Op | Payload | Response |
|----|---------|----------|
| `register_credential` | `{phantom_token, api_key}` | `{status: "ok", phantom_token}` |
| `chat_completion` | `{messages: [...], tools: [...], phantom_token?: string, simulate_failure?: bool}` | `{response: {stop_reason, content, tool_calls}, model, usage, cached}` |
| `reset_circuit` | `{}` | `{status: "ok", circuit_open: false}` |
| `get_stats` | `{}` | `{request_count, total_tokens, cache_hits, circuit_open, consecutive_failures, model}` |

### ToolRegistryActor

| Op | Payload | Response |
|----|---------|----------|
| `register_tool` | `{name, description, input_schema}` | `{status: "ok", tool: name}` |
| `list_tools` | `{}` | `{tools: [...], count}` |
| `execute_tool` | `{name, input: {...}}` | `{status: "ok", tool, output: {...}}` |
| `get_stats` | `{}` | `{tool_count, execution_count}` |

### AgentActor

| Op | Payload | Response |
|----|---------|----------|
| `chat` | `{message, session_id?}` | `{status: "ok", response, session_id, loop_iterations, messages_count}` |
| `set_system_prompt` | `{prompt}` | `{status: "ok"}` |
| `get_history` | `{}` | `{messages: [...], count}` |
| `compact_context` | `{}` | `{status: "ok", messages_count}` |
| `get_capabilities` | `{}` | `{capabilities: [...]}` |

### SessionManagerActor

| Op | Payload | Response |
|----|---------|----------|
| `create_session` | `{channel, user_id, agent_id}` | `{status: "ok", session_id}` |
| `get_session` | `{session_id}` or `{channel, user_id}` | session metadata |
| `end_session` | `{session_id}` | `{status: "ok"}` |
| `list_sessions` | `{}` | `{sessions: [...], count}` |

### OrchestratorActor (WorkflowActor)

| Op | Payload | Response |
|----|---------|----------|
| `workflow_run` | `{task, task_id}` | `{status: "ok", result, sub_results, sub_tasks}` |
| `workflow_signal` | `{name: "cancel"}` | `{ok: true}` |
| `workflow_query` | `{name: "status"}` | `{task_id, status, progress}` |

### MemoryActor

| Op | Payload | Response |
|----|---------|----------|
| `store_memory` | `{scope, scope_id, key, value}` | `{status: "ok", key, scope}` |
| `recall_memory` | `{scope, scope_id, query}` | `{memories: [...], count, query}` |
| `list_memories` | `{scope, scope_id}` | `{memories: [...], count}` |
| `delete_memory` | `{scope, scope_id, key}` | `{status: "ok", key}` |

### AuditEventActor

| Op | Payload | Response |
|----|---------|----------|
| `log_event` | `{event_type, detail}` | `{ok: true, seq}` |
| `query_events` | `{event_type?, limit?}` | `{events: [...], count}` |
| `poll_events` | `{consumer_id, limit?}` | `{events: [...], count, cursor, watermark}` |
| `get_stats` | `{}` | `{events_logged, last_event_type, watermark}` |

### AgentStateFSM

| Op | Payload | Response |
|----|---------|----------|
| `transition` | `{to: "processing"\|"tool_executing"\|"responding"\|"idle"\|"error"}` | `{status: "ok", from, to, state}` or `{error}` |
| `get_state` | `{}` | `{status: "ok", state, transition_count}` |

**Valid transitions:**
```
idle → processing
processing → tool_executing | responding
tool_executing → processing
responding → idle
any → error
error → idle
```

### TaskQueueActor

| Op | Payload | Response |
|----|---------|----------|
| `enqueue` | `{task_type?, payload?}` | `{status: "ok", seq}` |
| `dequeue` | `{limit?}` | `{tasks: [...], count}` — tasks marked `in_flight` |
| `ack` | `{seq}` | `{status: "ok", seq}` — deletes task, increments Completed |
| `nack` | `{seq}` | `{status: "ok", seq}` — resets task to `pending`, increments Failed |
| `get_stats` | `{}` | `{enqueued, completed, failed, depth}` |

### HealthMonitorActor

| Op | Payload | Response |
|----|---------|----------|
| `poll_tick` | `{}` | `{status: "ok", poll_count}` — internal; also called by SendAfter |
| `get_health` | `{}` | `{group_health, healthy, degraded: [...], last_poll_ms}` |
| `get_stats` | `{}` | `{poll_count, last_poll_ms, group_health}` |

**Monitored process groups:**
`svc:llm_router`, `svc:tool_registry`, `svc:agent`, `svc:session_manager`,
`svc:memory`, `svc:audit`, `svc:agent_fsm`, `svc:task_queue`

## Process Groups

| PG Name | Joined By | Used By |
|---------|-----------|---------|
| `svc:llm_router` | LLMRouterActor | AgentActor |
| `svc:tool_registry` | ToolRegistryActor | AgentActor |
| `svc:agent` | AgentActor | OrchestratorActor |
| `svc:session_manager` | SessionManagerActor | (external discovery) |
| `svc:memory` | MemoryActor | ToolRegistryActor (memory_search) |
| `svc:audit` | AuditEventActor | All actors (fireAudit helper) |
| `svc:agent_fsm` | AgentStateFSM | AgentActor |
| `svc:task_queue` | TaskQueueActor | OrchestratorActor (optional enqueue) |
| `svc:health_monitor` | HealthMonitorActor | HealthMonitorActor itself polls all other PGs |

## References

- [PlexSpaces Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [A2A Multi-Agent Example](../a2a_multi_agent/README.md) — primary pattern reference
- [Agentic RAG Pipeline](../agentic_rag_pipeline/README.md) — circuit breaker, GenEvent, GenFSM patterns
- [MiniClaw Blog Post](../../../../archived_docs/miniclaw-secure-ai-agents-with-actors.md)
- [All Examples](../../../README.md)
