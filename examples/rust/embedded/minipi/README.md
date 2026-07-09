# MiniPi — Agent Harness & Eval Example (Rust)

Faithful Rust port of the Python `minipi` example. Demonstrates the full OODA-loop
agent eval pipeline using the PlexSpaces Rust SDK.

## What it demonstrates

| Feature | Detail |
|---|---|
| `AgentLoop` (OODA) | Observe → Orient → Decide → Act with step recording |
| `SchemaValidationFacet` | Priority 95 — validates tool inputs before actor sees them |
| `ExecutionTraceFacet` | Priority 85 — ordered OODA step capture, exports to KV |
| `DurabilityFacet` | Priority 90 — journals every step for crash recovery |
| Supervision tree | `one_for_one` — crashed actor restarts, orchestrator keeps running |
| Human-in-the-loop | `ApprovalGateActor` FSM: idle → awaiting_approval → idle |
| Ollama integration | `provider = "ollama"`, `model = "llama3.2"`, `http://localhost:11434` |
| Regression detection | Compare scores across runs, flag ±5% threshold |
| Benchmark | Same scenario, N harness configs, comparison table |

## Architecture

```
                    ┌─────────────────────────────────────────────┐
                    │  Supervision tree (one_for_one)              │
                    │                                               │
                    │  LLMGatewayActor ──────────────────────────  │
                    │  ToolRegistryActor (SchemaValidationFacet)   │
                    │  AgentActor (DurabilityFacet+ExecutionTrace) │
                    │  EvalRunnerActor                             │
                    │  ScenarioStoreActor (5 built-in scenarios)   │
                    │  ScorerActor                                 │
                    │  TrajectoryStoreActor                        │
                    │  RegressionDetectorActor                     │
                    │  BenchmarkActor                              │
                    │  ApprovalGateActor (FSM)                     │
                    │  DashboardActor                              │
                    └─────────────────────────────────────────────┘
```

## Actors (11 total)

1. **LLMGatewayActor** — Ollama integration with mock fallback, SHA-256 KV cache
2. **ToolRegistryActor** — `web_search`, `calculator`, `kv_read`, `kv_write`
3. **AgentActor** — OODA loop via `AgentLoop`, token budget, trajectory export
4. **EvalRunnerActor** — orchestrates eval suites, fan-out per scenario
5. **ScenarioStoreActor** — 5 built-in scenarios (math, search, multi-step, budget, contract)
6. **ScorerActor** — `task_completion`, `tool_use`, `efficiency`, `llm_judge` rubrics
7. **TrajectoryStoreActor** — persists and indexes trajectory records
8. **RegressionDetectorActor** — baseline comparison, ±5% threshold, step-level diff
9. **BenchmarkActor** — fan-out N eval runs with different configs, comparison table
10. **ApprovalGateActor** — FSM human-in-the-loop (idle → awaiting_approval → idle)
11. **DashboardActor** — read-only aggregator for eval results

## Built-in scenarios

| ID | Name | Rubric | Difficulty |
|---|---|---|---|
| `sc-math-01` | Basic arithmetic | task_completion | easy |
| `sc-search-01` | Web search intent | tool_use | medium |
| `sc-multi-01` | Multi-step research | efficiency | hard |
| `sc-budget-01` | Budget enforcement | task_completion | medium |
| `sc-contract-01` | Contract violation recovery | tool_use | medium |

## Prerequisites

- Rust toolchain (stable)
- A running PlexSpaces node on port 8007 (or adjust `GRPC_PORT` in `main.rs`)
- (Optional) [Ollama](https://ollama.ai) running on `http://localhost:11434` with `llama3.2` pulled.
  If Ollama is unavailable, the LLM gateway falls back to a deterministic mock provider.

## Running

```bash
# Run the embedded self-test (starts its own node):
./test.sh

# Or run directly:
CARGO_TARGET_DIR=../../../../target cargo run
```

## Facet priority chain

```
virtual_actor(100) → schema_validation(95) → durability(90) →
execution_trace(85) → metrics(80)
```

## Live Test Output

Runs entirely in-process — no WASM compilation, no HTTP gateway. Completes in seconds:

```
  Node started:                    ✅
  All 12 actors spawned:           ✅
  Scenario store seeded:           ✅
  AgentActor OODA completed:       ✅  (outcome=completed, iterations=10, steps=40)
  Regression detection correct:    ✅
  Benchmark 2-config comparison:   ✅
  Example completed:               ✅

All validations passed.
```

## References

- [Blog: The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Go minipi](../../../go/apps/minipi/) — reference WASM implementation
- [Python minipi](../../../python/apps/minipi/) — Python SDK port
- [TypeScript minipi](../../../typescript/apps/minipi/) — TypeScript port
- [Rust WASM minipi](../../apps/minipi/) — Rust WASM port
- [SDK Agent API](../../../../sdks/rust/plexspaces-sdk/src/agent.rs)
