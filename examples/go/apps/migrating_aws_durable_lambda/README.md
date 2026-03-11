# AWS Durable Lambda → PlexSpaces: Serverless Webhook Processor (Go WASM)

Exactly-once webhook handling with deduplication: requests with the same `idempotency_key` (or `event_id`) return the cached response; first request is processed and stored.

## Use Case

Serverless webhook ingestion (e.g. Stripe, GitHub): duplicate deliveries must be idempotent. One actor holds the idempotency store; durability facet persists it across restarts.

## Abstractions

| Abstraction | Usage |
|-------------|--------|
| **GenServer** | Handle(webhook, …) and Handle(status, …) |
| **VirtualActor + Durability** | Lazy activation; state (processed keys → response) checkpointed |

## Quick Start

```bash
# From repo root: start node
./scripts/server.sh

# In another terminal: build and test
cd examples/go/apps/migrating_aws_durable_lambda
./build.sh
./test.sh 8092
```

## API

- **webhook**: `{"op": "webhook", "idempotency_key": "key-1", "body": {...}}` or `event_id` instead of `idempotency_key`. First time: process and cache response. Duplicate key: return cached response (dedup hit).
- **status**: `{"op": "status"}` — Returns total_processed, total_dedup_hits, keys_stored, total_compute_ms, total_coord_ms.

## Metrics

- **total_processed**: New requests processed.
- **total_dedup_hits**: Duplicate keys (cached response returned).
- **keys_stored**: Number of idempotency keys in state.
- **total_compute_ms / total_coord_ms**: Time breakdown.

## Comparison: AWS Durable Lambda vs PlexSpaces

| Feature | AWS (Lambda + DDB) | PlexSpaces |
|---------|--------------------|------------|
| Idempotency | DDB key by idempotency key | In-actor state (Processed map) + durability |
| Dedup | Conditional write, return existing | Lookup in Processed; return cached or process |
| Scale | Per partition / key | Virtual actor per instance (e.g. webhook:default) |

See [native/aws_durable_lambda_ref.md](native/aws_durable_lambda_ref.md) for AWS reference and mapping.

## References

- [PlexSpaces GenServer](../../../../docs/detailed-design.md#behaviors)
- [Getting Started](../../../../docs/getting-started.md)
