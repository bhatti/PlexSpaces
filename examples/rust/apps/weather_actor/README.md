# Weather Actor (Rust) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** with a deterministic test mode in a Rust WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `host::http_fetch` | Protobuf `HttpFetchRequest` / `HttpFetchResponse` over the `"weather-api"` service link when `offline_mode=false` |
| Deterministic test mode | `args.offline_mode = "true"` in `app-config.toml` removes the public internet dependency from `./test.sh` |
| TTL expiry | Cached entries in actor state expire after 5 minutes (checked via `host::now_ms`) |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Example harness | `cargo test --lib` covers actor logic without a live node; `./test.sh` builds, deploys, and exercises the deterministic offline path end-to-end |

## Run the example

```bash
cd examples/rust/apps/weather_actor
./test.sh            # deterministic offline mode by default
./test.sh 8091       # same, explicit HTTP port
./test.sh --contract-only
```

To exercise the live outbound HTTP path, change `args.offline_mode` in [`app-config.toml`](/Users/shahzadbhatti/workspace/myspaces/examples/rust/apps/weather_actor/app-config.toml) to `"false"` and run against a node whose `weather-api` service link is configured.

## Message API

| Message | Payload | Response |
|---------|---------|----------|
| `get_weather` | JSON `{ "op":"get_weather","city":"..." }` | JSON `{ city, temp_c, wind_kph, fetched_at_ms, source, error }` |
| `cache_stats` | JSON `{ "op":"cache_stats" }` | JSON `{ hits, misses }` |
| `clear_cache` | JSON `{ "op":"clear_cache" }` | JSON `{ cleared }` |
| `get_metrics` | JSON `{ "op":"get_metrics" }` | JSON `ApplicationMetrics` snapshot for the local node |

## Metrics

The actor updates framework application metrics through `application_metrics_add` on each handled
operation, and the example reads them back through `application_get_metrics` via the `get_metrics`
message. That keeps the Rust example aligned with the newer metrics architecture used by the other
language SDK examples instead of scraping application-list output.

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Python weather actor](../../../../python/apps/weather_actor/)
- [Go weather actor](../../../../go/apps/weather_actor/)
- [TypeScript weather actor](../../../../typescript/apps/weather_actor/)
