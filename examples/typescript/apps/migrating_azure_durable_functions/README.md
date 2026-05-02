# Document Processing Workflow (TypeScript WASM) – Azure Durable Functions style

Document processing pipeline: **OCR (fan-out)** → **classify** → **extract** → **store**. Uses **WorkflowActor** with virtual_actor and durability (same pattern as migrating_temporal).

## Purpose

- **WorkflowActor**: run (pipeline steps), signal(cancel), query(status).
- **Virtual actor**: One workflow per job (`document-processing:job-123`) via app-config.
- **Steps**: OCR (fan-out 4 pages, fan-in) → classify → extract → store.

## Quick Start

```bash
# Terminal 1 (repo root)
./scripts/server.sh

# Terminal 2
cd examples/typescript/apps/migrating_azure_durable_functions
./build.sh
./test.sh 8091
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/document-processing:{job_id}` with `{"op":"workflow_run","job_id":"..."}`.
- **Signal**: Same path with `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}`.

## Comparison

| Feature     | Azure Durable Functions | PlexSpaces                    |
|------------|-------------------------|-------------------------------|
| Orchestrator | DurableOrchestrationContext | WorkflowActor run/signal/query |
| Fan-out    | CallActivityAsync (parallel) | In-run fan-out (e.g. OCR pages) |
| Durability | Built-in                | Durability facet              |

## References

- [PLAN.md – migrating_azure_durable_functions](../../../../PLAN.md)
- [Azure Durable Functions](https://learn.microsoft.com/en-us/azure/azure-functions/durable/)
