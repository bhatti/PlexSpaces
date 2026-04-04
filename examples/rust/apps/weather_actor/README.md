# Weather Actor (Rust) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** combined with **KV-based caching** in a Rust WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `host::http_fetch` | Raw WIT host function for outbound HTTP via `"weather-api"` link |
| KV caching | `host::kv_get` / `host::kv_put` to cache results for 5 minutes |
| TTL expiry | Cache entries expire after 5 minutes (checked via `host::now_ms`) |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Example harness | `./test.sh` runs contract tests, builds WASM, deploys the actor, exercises cache/API paths, and prints output + metrics |

## Run the example

```bash
cd examples/rust/apps/weather_actor
./test.sh            # requires a running node with the weather-api service link
./test.sh 8092       # same, explicit HTTP port
./test.sh --contract-only
```

## Message API

| Message | Payload | Response |
|---------|---------|----------|
| `get_weather` | `{"city":"London"}` | `{"city","temp_c","wind_kph","fetched_at_ms","source"}` |
| `cache_stats` | `{}` | `{"hits","misses"}` |
| `clear_cache` | `{}` | `{"cleared":true}` |

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Python weather actor](../../../../python/apps/weather_actor/)
- [Go weather actor](../../../../go/apps/weather_actor/)
- [TypeScript weather actor](../../../../typescript/apps/weather_actor/)
