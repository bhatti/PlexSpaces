# LLM Workflow Orchestrator (TypeScript WASM)

A PlexSpaces TypeScript WASM example demonstrating five foundational agentic LLM patterns implemented as composable actors.

## Patterns Demonstrated

### 1. Routing
`RouterActor` classifies incoming content by inspecting keywords and length, then dispatches to one of four specialist pipelines: `summarize`, `extract`, `analyze`, or `generate`. This mirrors how real LLM orchestration systems route prompts to domain-specific models or prompt templates.

### 2. Prompt Chaining
`ChainActor.onExecute_chain` executes a sequence of discrete transformation steps — `summarize → extract_keywords → format_output` — where each step's output feeds the next. This pattern decomposes complex tasks into auditable, composable sub-tasks, closely matching how multi-step LLM pipelines operate.

### 3. Reflection (Iterative Refinement)
`OrchestratorWorkflow` runs a reflection loop: after an initial chain pass, the `JudgeActor` scores the output. If the score falls below the threshold, the orchestrator refines the content and retries, repeating up to `max_iterations` times. This mirrors self-critique patterns used by ReAct, Reflexion, and similar agentic frameworks.

### 4. LLM-as-Judge
`JudgeActor.onEvaluate` scores generated content on three criteria — relevance (shared vocabulary with the original query), completeness (length heuristic), and clarity (unique-word ratio) — and returns a composite score and per-criterion breakdown. In production, this slot is where a real LLM judge model would be called.

### 5. Evol-Instruct
`ChainActor.onEvolve_instruction` applies `N` rounds of mutation to a prompt: adding detail-request prefixes, example-request suffixes, and synonym substitution. This is the pattern used by WizardLM's Evol-Instruct dataset augmentation technique.

## Actors

| Actor | Type | Role |
|---|---|---|
| `RouterActor` | `GenServer` | Classifies input and selects pipeline route |
| `ChainActor` | `GenServer` | Executes multi-step transforms and instruction evolution |
| `JudgeActor` | `GenServer` | Scores content on relevance/completeness/clarity |
| `OrchestratorWorkflow` | `Workflow` | Coordinates route → chain → judge with reflection loop |

## Build

```bash
./build.sh
```

Requires Node.js, npm, and `jco` (installed automatically if missing).

The WASM artifact is written to `<repo_root>/target/examples/typescript/llm_workflow_orchestrator/llm_workflow_orchestrator_actor.wasm`.

## Test

```bash
./test.sh [HTTP_PORT]   # default port: 8091
```

Requires a running PlexSpaces node (`./scripts/server.sh`).

The test script covers:
1. RouterActor: `"summarize this"` routes to `summarize`
2. RouterActor: long analysis query routes to `analyze`
3. ChainActor: `execute_chain` returns `steps_completed=3`
4. JudgeActor: `evaluate` returns `score` in [0, 10]
5. ChainActor: `evolve_instruction` produces output different from input
6. OrchestratorWorkflow: `workflow_run` returns `status=completed`
7. OrchestratorWorkflow: `workflow_signal:feedback` accepted
8. OrchestratorWorkflow: `workflow_query:progress` returns current status
9. Stats from all three GenServer actors

## Message Reference

### RouterActor

```json
{ "op": "route", "content": "summarize this report" }
// → { "route": "summarize", "task_type": "summarize", "content": "...", "routing_id": 1234 }

{ "op": "get_stats" }
// → { "routing_decisions": 5, "last_route": "summarize", "routes": { "summarize": 3, "analyze": 2 } }
```

### ChainActor

```json
{ "op": "execute_chain", "content": "...", "steps": ["summarize", "extract_keywords", "format_output"] }
// → { "chain_id": 1234, "steps_completed": 3, "results": [...], "final_output": "...", "latency_ms": 1 }

{ "op": "evolve_instruction", "instruction": "Explain recursion", "mutations": 2 }
// → { "original": "Explain recursion", "evolved": "Please explain in detail: Explain recursion Provide examples.", "mutations_applied": 2 }

{ "op": "get_stats" }
// → { "steps_completed": 9, "current_chain": "summarize→extract_keywords→format_output", "chains_run": 3 }
```

### JudgeActor

```json
{ "op": "evaluate", "content": "...", "original_query": "What is ML?", "criteria": ["relevance", "completeness", "clarity"] }
// → { "score": 7.3, "criteria_scores": { "relevance": 6.5, "completeness": 9, "clarity": 6.5 }, "passed": true, "feedback": "Score: 7.3/10" }

{ "op": "get_stats" }
// → { "evaluations_run": 4, "avg_score": 7.1, "score_history": [7.3, 6.8, ...] }
```

### OrchestratorWorkflow

```json
{ "op": "workflow_run", "task": "summarize", "content": "...", "max_iterations": 3, "score_threshold": 6.0 }
// → { "task_id": "1234", "status": "completed", "iterations": 1, "final_score": 7.2, "result": "...", "route": "summarize" }

{ "op": "workflow_signal:feedback", "content": "Accepted." }

{ "op": "workflow_signal:reset" }

{ "op": "workflow_query:progress" }
// → { "task_id": "1234", "status": "completed", "current_step": "done", "iteration_count": 1, "final_score": 7.2 }

{ "op": "workflow_query:history" }
// → { "signals": ["Accepted."], "iteration_count": 1 }
```

## Architecture Context

- [PlexSpaces Architecture](../../../../docs/architecture.md)
- [Detailed Design](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Examples Gallery](../../README.md)
