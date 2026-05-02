# Payment Handler - GenServer Microservice (Python WASM with SDK)

Demonstrates a **GenServer-style microservice** for payment processing with idempotency and distributed locking.

**Real-world use cases**:
- Payment gateway integration
- Order payment processing
- Subscription billing
- Refund handling

## GenServer Pattern

```
┌─────────────┐   call    ┌─────────────────┐
│   Client    │ ────────► │ PaymentHandler  │
│  (sync)     │ ◄──────── │  (GenServer)    │
└─────────────┘   reply   └────────┬────────┘
                                   │
                    ┌──────────────┼──────────────┐
                    │              │              │
                    ▼              ▼              ▼
              ┌──────────┐  ┌──────────┐  ┌──────────┐
              │ KV Store │  │  Locks   │  │  State   │
              │(idempot.)│  │(critical)│  │ (durable)│
              └──────────┘  └──────────┘  └──────────┘
```

## APIs Used

| API | Usage | Description |
|-----|-------|-------------|
| `@gen_server_actor` | Request-reply pattern | Synchronous call/response |
| `host.kv_get/kv_put` | Idempotency | Prevent duplicate processing |
| `host.lock_acquire/lock_release` | Critical section | Prevent race conditions |
| `state()` | Transaction log | Durable state persistence |

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
# Terminal 1: Start node
./scripts/server.sh

# Terminal 2: Run tests
cd examples/python/apps/payment_handler
./test.sh 8091
```

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| `process_payment` | `{"payment_id":"pay-1","amount":1000,"customer_id":"cust-1"}` | Process payment (amount in cents) |
| `refund` | `{"refund_id":"ref-1","original_tx_id":"tx-1","amount":500}` | Process refund |
| `get_transaction` | `{"tx_id":"tx-1"}` | Get transaction details |
| `balance` | `{}` | Get total processed balance |
| `list_transactions` | `{"limit":10}` | List recent transactions |

## Idempotency

Payment processing uses KV store for idempotency:

```python
# First call - processes payment
process_payment(payment_id="pay-1", amount=1000)
# → {"status": "completed", "tx_id": "tx-1"}

# Duplicate call - returns cached result
process_payment(payment_id="pay-1", amount=1000)
# → {"status": "completed", "tx_id": "tx-1", "idempotent": true}
```

## Distributed Locking

Refunds use distributed locks to prevent race conditions:

```python
# Concurrent refund requests for same transaction
refund(refund_id="ref-1", original_tx_id="tx-1")  # Acquires lock
refund(refund_id="ref-2", original_tx_id="tx-1")  # Waits or fails
```

## Worker Pool Pattern

In production, deploy multiple PaymentHandler instances behind ElasticPool:

```toml
# app-config.toml
[pool]
min_workers = 2
max_workers = 10
scale_threshold = 0.8
```

1. Pool manages worker lifecycle
2. Load balancer distributes requests
3. Each worker handles one request at a time
4. Failed workers are restarted automatically

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@gen_server_actor(facets=["durability"])` | GenServer with durable state |
| `state()` | transaction_count, total_processed, transactions |
| `@handler()` | process_payment, refund, get_transaction |
| `host.kv_get/kv_put` | Idempotency checks |
| `host.lock_acquire/lock_release` | Refund race protection |

## Files

| File | Description |
|------|-------------|
| `payment_handler_actor.py` | GenServer payment processor |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - GenServer, KV, Locks API reference
- [Bank Account Example](../bank_account/) - Durability example
- [Job Processing Example](../job_processing/) - TupleSpace coordination
