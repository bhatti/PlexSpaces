# A2A Multi-Agent Collaboration (Go WASM)

Demonstrates Agent-to-Agent (A2A) communication and multi-agent collaboration patterns using PlexSpaces Go WASM actors.

## Overview

Five actors collaborate to decompose a high-level task into specialist sub-tasks:

- **AgentRegistryActor** — Distributed capability registry. Agents register their capabilities on startup; orchestrators query it to discover specialists.
- **ResearchAgent** — Looks up facts from a built-in knowledge base stored in KV. Supports topic-based fact retrieval.
- **AnalysisAgent** — Analyses data arrays and produces structured key-point summaries with confidence scores.
- **WriterAgent** — Composes formatted documents (professional or casual style) from analysis results.
- **OrchestratorAgent** (WorkflowActor) — Decomposes a task into research → analysis → writing steps, delegates each to the appropriate specialist via `host.Ask`, and aggregates intermediate results through TupleSpace tuples before returning the final assembled output.

## A2A Patterns Demonstrated

| Pattern | Where Used |
|---------|-----------|
| Capability registry | `AgentRegistryActor.register/discover` |
| KV-backed capability index | `"cap:{capability}:{agent_id}"` keys |
| Process group membership | All agents join `"agents"` group on init |
| Request-reply delegation | `host.Ask(agentID, op, payload, timeout)` |
| TupleSpace coordination | Orchestrator writes `["task", taskID, "step", name, result]` tuples |
| Workflow Run/Signal/Query | `OrchestratorAgent` implements `WorkflowActor` |

## Usage

```bash
# Start a PlexSpaces node (HTTP gateway on 8092, gRPC on 8091)
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start \
  --node-id test-node --listen-addr 0.0.0.0:8091

# Build and test
./build.sh
./test.sh 8092
```

## Test Scenarios

1. List all registered agents (registry `list_all`)
2. Discover agents by capability `research`
3. Discover agents by capability `analysis`
4. Get agent card for a specific agent
5. Research a topic directly via `research_agent`
6. Analyze data directly via `analysis_agent`
7. Write a document directly via `writer_agent`
8. Run full orchestrator workflow (delegates to all 3 specialists, aggregates via TupleSpace)
9. Query orchestrator `status` via `workflow_query`

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [PlexSpaces Go SDK](../../../../sdks/go/plexspaces/)
