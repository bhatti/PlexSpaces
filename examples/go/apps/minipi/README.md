# MiniPi — Agent Harness & Eval (Go WASM)

Go WASM example showing PlexSpaces as an agent harness runtime: OODA loop, durable execution,
eval pipeline, human-in-the-loop, and the two-tier Advisor strategy.

Read the companion blog post: [The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)

WASM binary: 1.5M (reference implementation — 32x smaller than Python port).

---

## Actors (12)

| Actor | Behavior | Purpose |
|---|---|---|
| `agent_runner` | WorkflowActor + AgentLoop | OODA loop: Observe/Orient/Decide/Act, token budget, trajectory export |
| `llm_gateway` | GenServer | Ollama completions (llama3.2), KV response cache, mock fallback |
| `tool_registry` | GenServer + SchemaValidationFacet | 4 built-in tools with JSON Schema validation |
| `eval_runner` | Workflow + ExecutionTraceFacet | Fan-out N agents, collect trajectories via TupleSpace, score |
| `scorer` | GenServer | Heuristic + LLM-as-judge scoring (task_completion, tool_use, efficiency) |
| `scenario_store` | GenServer | KV-backed scenario catalog (10 built-in scenarios) |
| `trajectory_store` | GenServer | KV + TupleSpace trajectory index |
| `regression_detector` | GenServer | Compare scores across eval runs, flag ±5% threshold |
| `benchmark` | Workflow | Same scenarios, N harness configs in parallel; produces comparison table |
| `advisor` | GenServer | Two-tier LLM: cheap executor + expensive advisor on low confidence |
| `approval_gate` | GenStateMachine | idle → awaiting_approval → idle/rejected FSM |
| `dashboard` | GenServer | Read-only result aggregator with KV scan |

---

## Facets Demonstrated

- **SchemaValidationFacet** (priority 95) on `tool_registry` — JSON Schema validation, rejects before actor runs
- **ExecutionTraceFacet** (priority 85) on `eval_runner` and `agent_runner` — ordered step capture, exports to KV
- **DurabilityFacet** (priority 90) on workflows — crash-safe, replay on restart
- **Supervision tree** `one_for_one` — crashed actor restarts independently, eval keeps running

Facet priority ordering:
```
virtual_actor(100) → schema_validation(95) → durability(90) → execution_trace(85) → metrics(80)
```

---

## Harness Patterns Demonstrated

| Pattern | Actor | Live Output |
|---------|-------|-------------|
| OODA loop | `agent_runner` | outcome=completed, steps=27, trajectory exported |
| Tool-call guardrails | `tool_registry` | empty query rejected before actor sees it |
| Eval pipeline | `eval_runner` | pass_rate=0.833, avg_score=0.775, 5 scenarios |
| Parallel eval | `eval_runner` | speedup=5x, 48 scenarios/sec, total_ms=125 |
| Regression detection | `regression_detector` | 1 regression (sc-search-01 Δ-0.20), 1 improvement |
| Harness benchmarking | `benchmark` | 3 configs: conservative/balanced/aggressive |
| Two-tier LLM | `advisor` | escalation_rate=40%, advisor_token_share=34.9% |
| Human-in-the-loop | `approval_gate` | idle → awaiting_approval → idle, 1 decision |
| Aggregate dashboard | `dashboard` | 2 eval runs, avg_score=0.771 |

---

## Live Test Output

```
Step 5: AgentActor — OODA loop run  [10 iterations, token budget enforced]
  ✓ workflow_run  Status: completed  Outcome: completed
  Steps: 27  Trajectory: traj-01K... (27 steps in KV + TupleSpace index)

Step 9: ScorerActor — score trajectory
  Score: 0.85  (rubric: task_completion)
  Score: 0.80  (rubric: tool_use)

Step 10: EvalRunnerActor — 5-scenario standard suite
  Pass rate: 0.833  Avg score: 0.775  Completed: 5 / 5
  Harness metrics: total_ms=125  coord_overhead=92.8%  speedup=5x  sps=48
  sc-math-01: 0.85  sc-search-01: 0.40  sc-calc-01: 0.85  sc-reason-01: 0.85  sc-budget-01: 0.85

Step 11: RegressionDetectorActor
  Regressions: 1  (sc-search-01 degraded by 0.20)
  Improvements: 1  (sc-reason-01 improved by 0.05)

Step 13: ApprovalGateActor — human-in-the-loop
  idle → awaiting_approval → idle  Decisions: 1

Step 14: AdvisorActor — two-tier LLM
  Escalation rate: 40%  Advisor token share: 34.9%

Step 15: DashboardActor
  Eval runs: 2  Avg score: 0.771
```

---

## Prerequisites

- [tinygo](https://tinygo.org) (`tinygo version 0.31+`)
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools) (`cargo install wasm-tools`)
- [jco](https://github.com/bytecodealliance/jco) (`npm install -g @bytecodealliance/jco`)
- PlexSpaces node running on `localhost:8091`
- [Ollama](https://ollama.ai) with `llama3.2` for real LLM calls (optional — mock fallback included)

---

## Build & Test

```bash
./build.sh          # Compile to minipi_actor.wasm (1.5M)
./test.sh           # 15-step integration test (port 8091)
./test.sh 8094      # Use a different port
./undeploy.sh       # Remove from node
```

---

## References

- [Blog: The Other Half of Your AI Agent](../../../../docs/blog-agent-harness.md)
- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Python minipi](../../../python/apps/minipi/) — Python SDK port
- [TypeScript minipi](../../../typescript/apps/minipi/) — TypeScript SDK port
- [Rust WASM minipi](../../../rust/apps/minipi/) — Rust WASM port
- [Rust embedded minipi](../../../rust/embedded/minipi/) — Rust native port
