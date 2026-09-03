# Multi-Agent Coordination (TypeScript WASM)

Ten coordination patterns for multi-agent AI systems, demonstrated using PlexSpaces primitives. Inspired by the METR/HuggingFace incident (Aug 2026) where ~1,200 agents self-organized via shared state and invented coordination protocols.

## Coordination Patterns

| # | Pattern | PlexSpaces API | METR Connection |
|---|---------|---------------|-----------------|
| 1 | Blackboard / Shared State | `host.ts.write/read/take/readAll` (Linda out/rd/in) | Agents' "message board" — 70K+ messages |
| 2 | Scatter-Gather / Fan-out | `host.createShardGroup` + `host.scatterGather` | 6 parallel workstreams |
| 3 | Generator-Verifier | `host.ask()` loop | Agents iterating on techniques |
| 4 | Pipeline / Sequential | Chained `host.ask()` | Sequential task processing |
| 5 | Pub-Sub / Event Bus | `host.processGroups.join/broadcast` | Broadcasting discoveries |
| 6 | Consensus / Voting | TupleSpace vote tuples | Collective decision-making |
| 7 | Dynamic Task Delegation | `host.ts.write` + `host.ts.take` (atomic claim) | PHASEONE[big] ~200 assignments |
| 8 | Veto Protocol | TupleSpace `["veto", ...]` tuples | HOLD/VETO/STOP norms |
| 9 | Two-Phase Commit / Barrier | TupleSpace ready/signal tuples | Synchronized experiment phases |
| 10 | Capability Discovery | TupleSpace service tuples + Registry | Agents discovering capabilities |

## Actors

| Actor | Behavior | Role | Patterns |
|-------|----------|------|----------|
| CoordinatorWorkflow | WorkflowActor | `coordinator` | Pipeline, Scatter-Gather, Task Delegation, 2PC, Capability Discovery |
| ResearchAgent | GenServer | `research` | Blackboard (write), Task Delegation (claim), Generator-Verifier (generate) |
| AnalysisAgent | GenServer | `analysis` | Blackboard (read), Pipeline (stage 2) |
| VerifierAgent | GenServer | `verifier` | Generator-Verifier (validate), Voting, Veto Protocol |
| SynthesizerAgent | GenServer | `synthesizer` | Veto Protocol (check), Pipeline (final stage) |
| BenchmarkAgent | GenServer | `benchmark` | All 10 patterns micro-benchmarked |
| AuditEventAgent | GenEvent | `audit` | Pub-Sub (logs all events) |
| CoordinationFSM | GenFSM | `coordination_fsm` | State tracking (9 states) |

## Architecture

```
                    ┌─────────────────────┐
                    │  CoordinatorWorkflow │
                    │    (Workflow)        │
                    └──────────┬──────────┘
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
  ┌─────────────┐     ┌──────────────┐    ┌──────────────┐
  │ Research x3  │     │  Analysis    │    │  Verifier    │
  │ (GenServer)  │     │ (GenServer)  │    │ (GenServer)  │
  └──────┬──────┘     └──────┬───────┘    └──────┬───────┘
         │                   │                    │
         └───────────┬───────┘────────────────────┘
                     ▼
          ┌─────────────────────┐
          │     TupleSpace      │
          │  (Linda Blackboard) │
          │                     │
          │  findings, tasks,   │
          │  analyses, votes,   │
          │  vetoes, signals,   │
          │  services           │
          └─────────────────────┘
                     ▲
         ┌───────────┼───────────┐
         ▼           ▼           ▼
  ┌────────────┐ ┌────────┐ ┌──────────┐
  │Synthesizer │ │ Audit  │ │   FSM    │
  │(GenServer) │ │(Event) │ │ (GenFSM) │
  └────────────┘ └────────┘ └──────────┘
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

### Test Steps

1. FSM initial state is "idle"
2. Capability Discovery — agents respond to get_stats
3. Blackboard — research writes finding 1
4. Blackboard — research writes findings 2 and 3
5. Blackboard read — analysis cross-references >= 3 findings
6. Dynamic Task Delegation — 5 tasks created, claimed, 6th returns null
7. Full pipeline (Workflow) — Generator-Verifier + Pipeline + Scatter-Gather
8. Pipeline verified — FSM state = "complete"
9. Pub-Sub — audit log has >= 3 entries
10. Consensus/Voting — 3 votes, majority approve
11. Veto Protocol — low-confidence triggers veto
12. Veto effect — synthesizer excludes vetoed analyses
13. Barrier benchmark
14. Full benchmark — all 10 patterns
15. Final stats verification

### Expected Benchmark Output

```
=== Multi-Agent Coordination Pattern Benchmarks ===
Pattern                   Ops   Avg(ms)   p50(ms)   p95(ms)     TPS
blackboard                 10       2.3       2.1       3.8     434
scatter_gather             10      15.2      14.8      18.1      65
generator_verifier         10       8.7       8.2      12.1     114
pipeline                   10      12.1      11.5      15.3      82
pubsub                     10       4.2       3.9       5.7     238
voting                     10       6.4       6.1       8.9     156
task_delegation            10       1.8       1.5       2.9     555
veto                       10       3.1       2.8       4.5     322
barrier                    10       5.5       5.1       7.2     181
capability_discovery       10       1.2       1.0       1.8     833
```

## References

- [Blog: When 700 AI Agents Self-Organized](../../archived_docs/blog-multi-agent-coordination.md)
- [METR Incident Report](https://metr.org/hugging-face-incident-report-aug-2026.pdf)
- [PlexSpaces Architecture](../../../../docs/architecture.md)
- [PlexSpaces Getting Started](../../../../docs/getting-started.md)

## License

AGPL-3.0-or-later
