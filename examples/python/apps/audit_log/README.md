# Audit Log Example (Python WASM – Event Handler)

Fire-and-forget audit events (action, actor_id, resource, details, ts). Uses **@event_actor** (EventHandler) and **host.log** only. Storage (ts_write/kv) can be added when needed; the runtime re-instantiates after each handle() so multiple sequential events work.

## Overview

This example demonstrates:

- **EventHandler / GenEvent**: `@event_actor` with fire-and-forget `log` handler; no request-reply required for logging.
- **Log-only by default**: Each event is logged via `host.log("info", msg)`. Multiple sequential events work (runtime re-instantiates per message). TupleSpace/keyvalue can be added when needed.
- **Deploy with behavior_kind**: Deploy with `behavior_kind=GenEvent` so logs show `EventHandler`.

## Use Cases

- Security and compliance: who did what, when (log output)
- Fire-and-forget event ingestion (no client waiting on response)
- Verifying event-handler (GenEvent) deployment and messaging

## SDK Features Used

- **plexspaces-actor** WIT: `host.log`, `host.now_ms`, `host.send`.
- **@event_actor**: GenEvent-style; handlers return `str` only for WASM boundary.

## Build

```bash
./build.sh
```

## Test

From repo root:

```bash
# Terminal 1: start server (gRPC 8091, HTTP 8091)
cd /path/to/tspaces
make build
./scripts/server.sh
# Wait until "Server is ready!"

# Terminal 2: build and run audit_log test
cd examples/python/apps/audit_log
source ~/venv/bin/activate
./build.sh
./test.sh 8091
```

Check the node log for lines like `audit action=login actor_id=user-1 resource=/api/session ...`.

## Deploy (event-handler logging)

Deploy with `behavior_kind=GenEvent` so registry and process_message logs show `EventHandler`:

```bash
curl -s -X POST "http://localhost:8091/api/v1/applications/deploy" \
  -F "application_id=audit-log-test" \
  -F "name=audit-log-test" \
  -F "version=1.0.0" \
  -F "behavior_kind=GenEvent" \
  -F "wasm_file=@audit_log_actor.wasm;type=application/wasm"
```

## API

Use `POST /api/v1/actors/{namespace}/{actor_type}` where `namespace` and `actor_type` match the deploy multipart `name` (default supervisor child id), e.g. `audit-log-test` for both when `name=audit-log-test`.

| Handler | HTTP   | Description |
|--------|--------|-------------|
| `log`  | POST   | Log one audit event (action, actor_id, resource, details, ts) via host.log. Fire-and-forget. |

## Related

- [Bank Account Example](../bank_account/) – durable state with SDK
- [Chat Room Example](../chat_room/) – messaging patterns
- [SDK Guide](../../../../docs/sdk.md) – host.log and event-handler patterns
