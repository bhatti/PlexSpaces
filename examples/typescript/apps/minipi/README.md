# MiniPi — Agent Harness & Eval (TypeScript WASM)

TypeScript port of [examples/python/apps/minipi](../../python/apps/minipi/), demonstrating the full PlexSpaces agent harness and eval pipeline compiled to a single WASM component.

## What it demonstrates

| Pattern | Actor | Details |
|---|---|---|
| OODA loop | `AgentActor` | Observe → Orient → Decide → Act via `AgentLoop` from `@plexspaces/sdk` |
| `SchemaValidationFacet` (priority 95) | `ToolRegistryActor` | Validates tool-call method inputs before actor sees them |
| `ExecutionTraceFacet` (priority 85) | `EvalRunnerActor` | Captures ordered steps, exports to KV on completion |
| `DurabilityFacet` (priority 90) | `EvalRunnerActor`, `BenchmarkActor`, `ApprovalGateActor` | Crash-safe workflow with checkpoint/replay |
| Supervision trees | `app-config.toml` | `one_for_one` restart strategy across 11 actors |
| Parallel eval fan-out | `EvalRunnerActor` | Spawns N `AgentActor` instances, collects via TupleSpace |
| Regression detection | `RegressionDetectorActor` | Diffs scores across eval runs, flags ±5% threshold |
| Config benchmarking | `BenchmarkActor` | Same scenarios, different harness configs, parallel eval |
| Human-in-the-loop | `ApprovalGateActor` | GenFSM: idle → awaiting\_approval → idle |
| Read-only aggregator | `DashboardActor` | Aggregates eval reports and trajectories from KV |

## Actors (all in `minipi_actors.ts`)

1. **AgentActor** — Workflow, OODA loop with `AgentLoop`
2. **LLMGatewayActor** — GenServer, Ollama at `http://localhost:11434`, KV response cache
3. **ToolRegistryActor** — GenServer, 4 built-in tools, registers schemas for `SchemaValidationFacet`
4. **EvalRunnerActor** — Workflow, fan-out/collect eval orchestration
5. **ScenarioStoreActor** — GenServer, 5 built-in scenarios seeded in KV
6. **ScorerActor** — GenServer, heuristic + llm\_judge rubrics
7. **TrajectoryStoreActor** — GenServer, KV + TupleSpace trajectory index
8. **RegressionDetectorActor** — GenServer, score diff with 5% threshold
9. **BenchmarkActor** — Workflow, parallel config comparison
10. **ApprovalGateActor** — FSM, human-in-the-loop approval gate
11. **DashboardActor** — GenServer, read-only aggregator

## Requirements

- Node.js 18+
- Ollama running at `http://localhost:11434` with model `llama3.2` pulled
- PlexSpaces node running (default port 8091)

## Live Test Output

WASM binary: 13M. The TypeScript implementation is the only one that tracks real token counts and estimated cost per scenario:

```
Step 10: EvalRunnerActor — 5-scenario standard suite
  Pass rate: 0.4  Avg score: 0.818  Completed: 5 / 5
  Tokens: 311 in / 223 out  (est. cost: $0.00018)
  sc-math-01:   0.92  (53in/41out)
  sc-search-01: 0.92  (63in/45out)
  sc-calc-01:   0.76  (60in/44out)
  sc-reason-01: 0.79  (62in/44out)
  sc-budget-01: 0.70  (73in/49out)

Step 11: RegressionDetectorActor
  Regressions: 1  Improvements: 1

Step 13: ApprovalGateActor
  idle → awaiting_approval → idle  Decisions: 1

Step 15: DashboardActor
  Eval runs: 2  Avg score: 0.803
```

## Build & Run

```bash
./build.sh                # compile to minipi_actor.wasm (13M)
./test.sh [HTTP_PORT]     # deploy + run 15-step integration test (default: 8091)
./undeploy.sh             # remove from node
```

## References

- [Blog: The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [TypeScript SDK](../../../../sdks/typescript/)
- [Go minipi](../../../go/apps/minipi/) — reference implementation
- [Python minipi](../../../python/apps/minipi/) — Python SDK port
- [Rust WASM minipi](../../../rust/apps/minipi/) — Rust WASM port
- [Rust embedded minipi](../../../rust/embedded/minipi/) — Rust native port
