# Webhook Handler Example (FaaS-style HTTP actor)

**Purpose**: Webhook handler actor invoked via HTTP: receive webhooks at a stable URL, store recent deliveries, list via GET. Uses correct PlexSpaces APIs and explicit tenant/namespace (no internal context).

This embedded example disables auth for local testing. When auth is disabled, the HTTP gateway still needs `x-tenant-id` so requests resolve against the correct tenant namespace.

## Overview

- **POST** `/api/v1/actors/{namespace}/webhook_handler` — Deliver a webhook (body = payload). Returns `{ id, received_at, action: "delivered" }`.
- **GET** `/api/v1/actors/{namespace}/webhook_handler?action=list` — List recent deliveries and total count.

## APIs used

- **NodeBuilder** — Build node with `with_listen_addr`, `with_in_memory_backends`, `with_auth_disabled`, and `build_started().await` so release config, unified migrations, service initialization, and runtime startup follow the same path as the server.
- **RequestContext::new_without_auth(tenant_id, namespace)** — Explicit tenant/namespace (e.g. `"acme-corp"`, `"webhooks"`).
- **plexspaces_sdk::spawn(...)** — Spawn the annotated actor with a unique actor name; the framework constructs the structured actor ID for type `webhook_handler`.
- **GenServer SDK annotations** — `#[gen_server_actor]`, `#[plexspaces_handlers]`, `#[handler(...)]` drive request/reply dispatch.

## Running

Examples use the **workspace shared target directory** (`<workspace>/target`). See `.cargo/config.toml` and CLAUDE.md.

```bash
cd examples/rust/embedded/webhook_handler
cargo run
```

Or run the test script:

```bash
./test.sh
```

## HTTP testing

```bash
# List (empty at start)
curl -s \
  -H "x-tenant-id: acme-corp" \
  "http://127.0.0.1:8002/api/v1/actors/webhooks/webhook_handler?action=list"

# Deliver a webhook
curl -s -X POST -H "Content-Type: application/json" \
  -H "x-tenant-id: acme-corp" \
  -d '{"action":"deliver","type":"github.push","repo":"acme/backend","commits":3}' \
  "http://127.0.0.1:8002/api/v1/actors/webhooks/webhook_handler/ask"
```

## Use cases

- **Webhook receivers**: Stable URL per endpoint (e.g. GitHub, Stripe, Slack) backed by one actor per tenant/endpoint.
- **Audit / replay**: Store recent deliveries (e.g. last 100) for debugging or idempotency.

## See also

- [Architecture](../../../../docs/architecture.md)
- [Examples](../../../README.md)
