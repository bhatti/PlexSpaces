# Agentic RAG Pipeline (Go WASM)

A multi-actor PlexSpaces example demonstrating **Agentic RAG** (Retrieval-Augmented Generation), **Trustworthy Generation**, **Deep Search**, and **Exception Handling** patterns compiled to WebAssembly.

## What It Demonstrates

| Pattern | Actor | Description |
|---------|-------|-------------|
| **Agentic RAG** | `rag_workflow` | Orchestrates the full index→retrieve→generate→validate loop with automatic retry |
| **Deep Search** | `retriever` | Multi-hop keyword search that broadens to individual query words when initial results are sparse |
| **Trustworthy Generation** | `validator` | Three-check answer quality gate: length, source grounding, and safety screening |
| **Exception Handling** | `generator` | Retry logic with a circuit breaker that opens after 3 consecutive LLM failures |

## Actors

- **`indexer`** — Splits documents into fixed-size chunks and stores them in KV store.
- **`retriever`** — Keyword-based chunk retrieval supporting `single` and `deep` search modes.
- **`generator`** — Simulated LLM generation with configurable retry and a circuit breaker.
- **`validator`** — Validates answers for length, source grounding, and safety.
- **`rag_workflow`** — Workflow actor that coordinates the full RAG pipeline with retry-on-validation-failure.

## Usage

```bash
# Build the WASM module
./build.sh

# Run against a local PlexSpaces node (default: localhost:8092)
./test.sh

# Run against a specific port
./test.sh 8092

# Run against multiple nodes
./test.sh localhost:8092 localhost:8094
```

## Test Scenarios

1. Index 3 documents (actor framework, distributed systems, WASM deployment)
2. Verify indexer document count
3. Retrieve chunks with a simple keyword query
4. Retrieve with deep multi-hop search mode
5. Generate an answer from provided context chunks
6. Validate a well-formed answer (expects pass)
7. Validate a too-short answer (expects fail)
8. Run the full RAG workflow end-to-end
9. Query workflow status via `workflow_query:status`
10. Check validator statistics

## References

- [PlexSpaces Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Detailed Design](../../../../docs/detailed-design.md)
