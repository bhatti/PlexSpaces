# Storefront API – E-commerce Backend on Host KV

> **Status: buggy/incomplete.** This example depends on the node’s host keyvalue store (SQLite `sql-backend`). If the node is not built with `plexspaces-keyvalue` feature `sql-backend`, or KV is not configured, handlers may return "config set failed". See PROJECT_TRACKER.md and node Cargo.toml.

One WASM actor implementing a **storefront backend**: store configuration, shopping carts, and checkout rate limiting. All state lives in the framework’s host keyvalue store (Redis/SQLite), so it is durable, shared across instances, and survives restarts.

## Why This Example

| Capability | What this example does |
|------------|------------------------|
| **Durable, shared storage** | Store config, carts, and rate-limit counters go through `host.kv_get` / `host.kv_put` / `host.kv_delete` / `host.kv_list`. The node’s KV backend is the single source of truth. |
| **Namespace isolation** | The host applies tenant/namespace to keys. Different apps or tenants get isolated key spaces. |
| **One actor, full API** | Config, carts, and checkout limits use the same KV with prefixes (`config:`, `cart:`, `ratelimit:`). One deployment serves the full storefront API. |
| **HTTP requests** | Use `POST /api/v1/actors/{namespace}/StorefrontService` for tell or `POST /api/v1/actors/{namespace}/StorefrontService/ask` for request-reply with JSON payload. |
| **Portable WASM** | Same component runs wherever the node runs (server, edge, FaaS). |

## Use Cases

- **Store config**: Free-shipping threshold, tax rate, currency, feature flags; change once, read everywhere.
- **Shopping cart**: Create/get/update/destroy cart per session; list carts by user; scale without sticky sessions.
- **Checkout rate limit**: Throttle checkout attempts per user or API key (e.g. 5 per minute).

## Host KV API Used

| WIT (simple-actor) | Purpose |
|--------------------|--------|
| `host.kv-get(key)` | Get value; empty or `ERROR:...` on failure. |
| `host.kv-put(key, value)` | Set value; empty on success. |
| `host.kv-delete(key)` | Delete key; empty on success. |
| `host.kv-list(prefix)` | List keys with prefix; returns JSON array or `ERROR:...`. |

Keys are namespaced by the runtime (per tenant/actor). This example uses logical prefixes: `config:...`, `cart:...`, `ratelimit:checkout:...`.

## Operations

| Handler | Payload | Description |
|---------|---------|-------------|
| **Store config** | | |
| `set_store_config` | `key`, `value` | Set store config (e.g. free_shipping_threshold, tax_rate). |
| `get_store_config` | `key` | Get store config value. |
| `list_store_config` | `prefix` | List config keys (optional prefix). |
| **Cart** | | |
| `create_cart` | `cart_id`, `user_id`, `items` (JSON array) | Create or overwrite cart. |
| `get_cart` | `cart_id` | Get cart by id. |
| `update_cart` | `cart_id`, `items` (JSON array) | Update cart items. |
| `destroy_cart` | `cart_id` | Destroy cart (e.g. after checkout). |
| `list_carts` | `prefix` | List cart ids (optional prefix). |
| **Checkout** | | |
| `checkout_allowed` | `identity`, `window_sec` (default 60), `max_requests` (default 5) | Check and consume one checkout; returns `allowed`, `remaining`, `reset_at`. |

## Example Flow

```bash
# Start node (with keyvalue backend)
./scripts/server.sh

# Build and test (shows stored/retrieved data in output)
cd examples/python/apps/storefront
./build.sh
./test.sh
```

The test script prints what each step does and the **data stored and retrieved** (API responses). When the runtime returns handler payloads, you see the config values, cart content, and rate-limit `allowed`/`remaining` in the output. Optional **Step 6** runs a SQLite query so you can verify the same data in the keyvalue backend.

## Verifying data in SQLite

When the node uses the SQLite keyvalue backend, all storefront data lives in the `kv_store` table. The test script looks for the DB at:

- `./app/data/keyvalue.db`
- `./data/keyvalue.db`
- or `$PLEXSPACES_KV_SQLITE_PATH`

If found and `sqlite3` is installed, it runs:

```sql
SELECT tenant_id, namespace, key, value FROM kv_store;
```

To verify manually (set the path to your node’s keyvalue DB, e.g. from config `keyvalue_backend.sql.connection_string`):

```bash
# Example: if node uses sqlite:///app/data/keyvalue.db
sqlite3 app/data/keyvalue.db "SELECT tenant_id, namespace, key, value FROM kv_store;"

# Or with a readable value column (value is stored as BLOB)
sqlite3 app/data/keyvalue.db "SELECT key, length(value) AS value_len FROM kv_store;"
```

Schema: `kv_store(tenant_id, namespace, key, value, expires_at, created_at, updated_at)`. Keys from this example look like `config:free_shipping_threshold`, `cart:cart-001`, `ratelimit:checkout:user-alice:<bucket>`.

Example HTTP calls:

```bash
# Store config
curl -s -X POST "http://localhost:8092/api/v1/actors/storefront-test/StorefrontService" \
  -H "Content-Type: application/json" \
  -d '{"msg_type":"set_store_config","payload":{"key":"free_shipping_threshold","value":"50"}}'

# Create cart
curl -s -X POST "http://localhost:8092/api/v1/actors/storefront-test/StorefrontService" \
  -H "Content-Type: application/json" \
  -d '{"msg_type":"create_cart","payload":{"cart_id":"cart-1","user_id":"alice","items":"[{\"sku\":\"WIDGET\",\"qty\":2,\"price\":\"9.99\"}]"}}'

# Checkout rate limit (call repeatedly to see allowed → denied)
curl -s -X POST "http://localhost:8092/api/v1/actors/storefront-test/StorefrontService" \
  -H "Content-Type: application/json" \
  -d '{"msg_type":"checkout_allowed","payload":{"identity":"user-alice","max_requests":5}}'
```

## Files

| File | Description |
|------|-------------|
| `storefront_actor.py` | Actor: store config, cart, checkout rate limit via host KV. |
| `build.sh` | Build WASM with plexspaces-py. |
| `test.sh` | Deploy and run storefront API tests. |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
