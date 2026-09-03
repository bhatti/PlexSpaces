# Multi-Agent Coordination (Python WASM)

Ten coordination patterns for multi-agent AI systems, inspired by the METR/HuggingFace incident where ~1,200 agents self-organized via shared state and invented coordination protocols.

## Coordination Patterns

| # | Pattern | PlexSpaces API | Actor |
|---|---------|---------------|-------|
| 1 | Blackboard / Shared State | `host.ts.write/read/take/read_all` | ResearchAgent, AnalysisAgent |
| 2 | Scatter-Gather | `host.scatter_gather()` | CoordinatorWorkflow |
| 3 | Generator-Verifier | `host.ask()` loop | CoordinatorWorkflow, VerifierAgent |
| 4 | Pipeline / Sequential | Chained `host.ask()` | CoordinatorWorkflow |
| 5 | Pub-Sub / Event Bus | `host.process_groups.broadcast()` | All agents → AuditEventActor |
| 6 | Consensus / Voting | TupleSpace vote tuples | VerifierAgent |
| 7 | Dynamic Task Delegation | `host.ts.write` + `host.ts.take` | ResearchAgent |
| 8 | Veto Protocol | TupleSpace veto tuples | VerifierAgent, SynthesizerAgent |
| 9 | Two-Phase Commit / Barrier | Ready signals + commit tuple | BenchmarkAgent |
| 10 | Capability Discovery | TupleSpace service tuples | All agents |

## Actors

| Actor | Behavior | Role | Description |
|-------|----------|------|-------------|
| CoordinatorWorkflow | Workflow | coordinator | Orchestrates full pipeline |
| ResearchAgent | GenServer | research | Generates findings, claims tasks |
| AnalysisAgent | GenServer | analysis | Cross-references findings |
| VerifierAgent | GenServer | verifier | Validates, votes, vetoes |
| SynthesizerAgent | GenServer | synthesizer | Produces final reports |
| BenchmarkAgent | GenServer | benchmark | Pattern micro-benchmarks |
| AuditEventActor | GenEvent | audit | Event logging |
| CoordinationFSM | GenFSM | coordination_fsm | State tracking |

## Architecture

```
                    ┌─────────────────────┐
                    │  CoordinatorWorkflow │
                    │   (Workflow Actor)   │
                    └──────────┬──────────┘
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                     │
    ┌─────▼─────┐       ┌─────▼─────┐        ┌─────▼──────┐
    │  Research  │       │  Analysis │        │  Verifier  │
    │  Agent     │       │  Agent    │        │  Agent     │
    └─────┬─────┘       └─────┬─────┘        └─────┬──────┘
          │                    │                     │
          └────────────────────┼─────────────────────┘
                               │
                    ┌──────────▼──────────┐
                    │     TupleSpace      │
                    │  (Shared Blackboard)│
                    │  findings, votes,   │
                    │  vetoes, tasks, svc  │
                    └──────────┬──────────┘
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                     │
    ┌─────▼──────┐      ┌─────▼──────┐       ┌─────▼─────┐
    │Synthesizer │      │ Benchmark  │       │   Audit   │
    │  Agent     │      │  Agent     │       │  (Event)  │
    └────────────┘      └────────────┘       └───────────┘
```

## Build

```bash
./build.sh
```

## Test

```bash
# Start PlexSpaces node first, then:
./test.sh [HTTP_PORT]  # default: 8091
```

All 15 test steps verify the 10 coordination patterns end-to-end.

## References

- [Blog: When 700 AI Agents Self-Organized](../../../archived_docs/blog-multi-agent-coordination.md)
- [PlexSpaces Architecture](../../../docs/architecture.md)
- [Getting Started](../../../docs/getting-started.md)
- [METR Incident Report](https://metr.org/hugging-face-incident-report-aug-2026.pdf)

## License

AGPL-3.0-or-later
