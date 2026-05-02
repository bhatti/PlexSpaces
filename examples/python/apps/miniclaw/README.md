# MiniClaw — Mini Agent Framework (Python WASM)

MiniClaw is a self-contained multi-agent framework that demonstrates all major PlexSpaces actor patterns in Python. Ten actors collaborate to handle AI chat, tool calling, durable orchestration, session management, and more — all compiled to a single WASM binary.

## Actors

| File | Actor | Behavior | Demonstrates |
|------|-------|----------|--------------|
| `llm_router.py` | `LLMRouterActor` | GenServer | Simulated LLM with tool-call routing |
| `tool_registry.py` | `ToolRegistryActor` | GenServer | Tool catalog with execution |
| `agent.py` | `AgentActor` | GenServer | Core agent loop (chat → LLM → tools → repeat) |
| `agent.py` | `SessionManagerActor` | GenServer | Session lifecycle via KV |
| `orchestrator.py` | `OrchestratorActor` | Workflow | Durable task decomposition and aggregation |
| `memory.py` | `MemoryActor` | GenServer | Scoped KV + TupleSpace memory |
| `memory.py` | `AuditEventActor` | GenEvent | Fire-and-forget audit trail |
| `memory.py` | `AgentStateFSM` | GenFSM | Agent lifecycle state machine |
| `infra.py` | `TaskQueueActor` | GenServer | Channel-backed durable task queue |
| `infra.py` | `HealthMonitorActor` | GenServer | Periodic process group polling |

## Architecture

```mermaid
graph TB
    User([User]) -->|ask| Agent
    User -->|ask| Session
    User -->|ask| Orch
    User -->|ask| Memory

    subgraph Core["Core Agent Loop"]
        Agent[AgentActor<br/>GenServer]
        LLM[LLMRouterActor<br/>GenServer]
        Tools[ToolRegistryActor<br/>GenServer]
    end

    subgraph Support["Support Actors"]
        Session[SessionManagerActor<br/>GenServer]
        Memory[MemoryActor<br/>GenServer]
        TaskQ[TaskQueueActor<br/>GenServer]
        Health[HealthMonitorActor<br/>GenServer]
    end

    subgraph Specialized["Specialized Behaviors"]
        Orch[OrchestratorActor<br/>Workflow]
        Audit[AuditEventActor<br/>GenEvent]
        FSM[AgentStateFSM<br/>GenStateMachine]
    end

    Agent -->|ask| LLM
    Agent -->|ask| Tools
    Agent -->|send| Audit
    Agent -->|send| FSM
    Tools -->|ask| Memory
    Orch -->|ask| Agent
    Health -.->|poll every 5s| PG[(Process Groups)]
    Health -.->|snapshot| TS[(TupleSpace)]
    Memory -.->|dual-write| TS
    Orch -.->|checkpoint| TS

    style User fill:#F57C00,color:#fff,stroke:#E65100,stroke-width:2px
    style Agent fill:#1565C0,color:#fff,stroke:#0D47A1,stroke-width:2px
    style LLM fill:#6A1B9A,color:#fff,stroke:#4A148C,stroke-width:2px
    style Tools fill:#2E7D32,color:#fff,stroke:#1B5E20,stroke-width:2px
    style Session fill:#283593,color:#fff,stroke:#1A237E,stroke-width:2px
    style Memory fill:#00838F,color:#fff,stroke:#006064,stroke-width:2px
    style TaskQ fill:#558B2F,color:#fff,stroke:#33691E,stroke-width:2px
    style Health fill:#E65100,color:#fff,stroke:#BF360C,stroke-width:2px
    style Orch fill:#1565C0,color:#fff,stroke:#0D47A1,stroke-width:2px
    style Audit fill:#AD1457,color:#fff,stroke:#880E4F,stroke-width:2px
    style FSM fill:#4E342E,color:#fff,stroke:#3E2723,stroke-width:2px
    style PG fill:#37474F,color:#fff,stroke:#263238,stroke-width:1px
    style TS fill:#37474F,color:#fff,stroke:#263238,stroke-width:1px
    style Core fill:#E3F2FD,stroke:#1565C0,stroke-width:2px
    style Support fill:#E8F5E9,stroke:#2E7D32,stroke-width:2px
    style Specialized fill:#FCE4EC,stroke:#AD1457,stroke-width:2px
```

## Patterns

- **Agent loop** — chat → LLM → `tool_use` → execute tools → feed results back → loop until `end_turn`
- **Channel as Message Queue** — `TaskQueueActor` uses `host.channel.send/receive/ack/nack` for at-least-once delivery
- **Process Groups for discovery** — actors find each other via `pg_first("svc:xxx")` without hardcoded IDs
- **KV persistence** — session metadata and memory entries survive actor restarts
- **TupleSpace coordination** — orchestrator writes results; health monitor writes snapshots; memory stores queryable tuples
- **send_after polling** — `HealthMonitorActor` reschedules itself instead of subscribing to events
- **Durable workflow** — `OrchestratorActor` uses `@run_handler`, `@signal_handler`, `@query_handler`
- **GenFSM** — `AgentStateFSM` guards transitions with an explicit allowed-transitions table

## Usage

```bash
# Build (requires Python SDK and componentize-py)
./build.sh

# Run tests against a running node
./test.sh [HTTP_PORT]   # default: 8091
```

## Project structure

```
miniclaw/
├── miniclaw_actor.py   # entry point: imports all actors + ACTOR_REGISTRY
├── llm_router.py       # LLMRouterActor
├── tool_registry.py    # ToolRegistryActor
├── agent.py            # AgentActor, SessionManagerActor
├── orchestrator.py     # OrchestratorActor (Workflow)
├── memory.py           # MemoryActor, AuditEventActor, AgentStateFSM
├── infra.py            # TaskQueueActor, HealthMonitorActor
├── helpers.py          # pg_first, fire_audit, write_actor_info, ask
├── app-config.toml     # supervisor tree + facet configuration
├── build.sh
└── test.sh
```

## Related documentation

- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
- [Python SDK](../../../../sdks/python/README.md)
- [Go MiniClaw](../../../go/apps/miniclaw/README.md)
