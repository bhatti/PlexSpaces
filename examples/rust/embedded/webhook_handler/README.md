# Webhook Handler Example (FaaS-style HTTP actor)

**Purpose**: Webhook handler actor invoked via HTTP: receive webhooks at a stable URL, store recent deliveries, list via GET. Uses correct PlexSpaces APIs and explicit tenant/namespace (no internal context).

## Overview

- **POST** `/api/v1/actors/{tenant_id}/{namespace}/webhook_handler` — Deliver a webhook (body = payload). Returns `{ id, received_at, action: "delivered" }`.
- **GET** `/api/v1/actors/{tenant_id}/{namespace}/webhook_handler?action=list` — List recent deliveries and total count.

## APIs used

- **NodeBuilder** — Build node with `with_listen_addr`, `with_in_memory_backends`, `build().await`, then `start().await`.
- **RequestContext::new_without_auth(tenant_id, namespace)** — Explicit tenant/namespace (e.g. `"acme-corp"`, `"webhooks"`).
- **ActorBuilder::new(behavior).with_id(...).with_namespace(...).spawn(&ctx, service_locator)** — Spawn actor; type `BehaviorType::Custom("webhook_handler")` for HTTP path routing.
- **GenServer** — Request/reply; `action=list` vs deliver; reply via `ctx.send_reply(...)`.

## Running

Examples use the **workspace shared target directory** (`<workspace>/target`). See `.cargo/config.toml` and CLAUDE.md.

```bash
cd examples/rust/embedded/webhook_handler
cargo run --release
```

Or run the test script:

```bash
./test.sh
```

## HTTP testing

```bash
# List (empty at start)
curl -s -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:8002/api/v1/actors/acme-corp/webhooks/webhook_handler?action=list"

# Deliver a webhook
curl -s -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"type":"github.push","repo":"acme/backend","commits":3}' \
  "http://127.0.0.1:8002/api/v1/actors/acme-corp/webhooks/webhook_handler"
```

## Use cases

- **Webhook receivers**: Stable URL per endpoint (e.g. GitHub, Stripe, Slack) backed by one actor per tenant/endpoint.
- **Audit / replay**: Store recent deliveries (e.g. last 100) for debugging or idempotency.

## See also

- [Architecture](../../../../docs/architecture.md)
- [Examples](../../../../docs/examples.md)
