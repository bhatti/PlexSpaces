# Distributed Tracing Pipeline (Go)

Production-grade distributed tracing pipeline — Go WASM implementation.

OTLP-style span ingestion, out-of-order trace assembly, tail-based sampling (error-biased, latency-biased, random), service graph construction.

## Architecture

- **LeaderActor**: Generates spans, distributes to workers via shard groups, collects results, builds service graph
- **WorkerActor**: Assembles traces from spans (handling out-of-order arrival), applies tail-based sampling

## Services Simulated

| Service | Operations |
|---------|-----------|
| api-gateway | POST /api/orders, GET /api/users, POST /api/payments, GET /api/inventory |
| user-service | GetUser, ValidateToken, UpdateProfile, ListUsers |
| order-service | CreateOrder, GetOrder, UpdateStatus, CancelOrder |
| payment-service | ProcessPayment, RefundPayment, ValidateCard, GetBalance |
| inventory-service | CheckStock, ReserveItem, ReleaseItem, UpdateQuantity |
| notification-service | SendEmail, SendSMS, SendPush, QueueNotification |

## Sampling Strategy

- **Error-biased**: Keep 100% of traces with errors (status >= 400)
- **Latency-biased**: Keep traces above p99 latency threshold
- **Random**: 1% random sample of normal traces

## PlexSpaces Primitives

| Primitive | Usage |
|-----------|-------|
| Shard Groups | Partition spans by trace_id hash |
| Scatter/Gather | Fan-out to workers, collect assembled traces |
| Process Groups | Leader/worker topology |
| Application Metrics | Compute vs coordination tracking |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Python version](../../../python/apps/tracing_pipeline/) (when available)
- [TypeScript version](../../../typescript/apps/tracing_pipeline/) (when available)
- [Architecture](../../../../docs/architecture.md)
