# Weather Actor (Python) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** combined with **KV-based caching** in a Python WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `ServiceHttpClient` | Ergonomic HTTP client backed by `"weather-api"` service link |
| KV caching | `host.kv_get` / `host.kv_put` to cache results for 5 minutes |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Required link validation | `[[applications.required_service_links]]` in `app-config.toml` |
| Example harness | `./test.sh` runs contract tests, builds WASM, deploys the actor, exercises cache/API paths, and prints output + metrics |

## Run the example

```bash
cd examples/python/apps/weather_actor
./test.sh            # requires a running node with the weather-api service link
./test.sh 8092       # same, explicit HTTP port
./test.sh --contract-only
```

## Service link fragment

Add to `release.toml`:

```toml
[[runtime.service_links]]
name           = "weather-api"
base_url       = "https://api.open-meteo.com"
transport      = "HTTP"
publish_to_registry = false

[runtime.service_links.retry_policy]
max_attempts = 3
initial_delay_ms = 100
```

## Message API

| Message | Payload | Response |
|---------|---------|----------|
| `get_weather` | `{"op":"get_weather","city":"London"}` | `{"city","temp_c","wind_kph","source"}` |
| `cache_stats` | `{"op":"cache_stats"}` | `{"hits","misses"}` |
| `clear_cache` | `{"op":"clear_cache"}` | `{"cleared":true}` |

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Architecture overview](../../../../../docs/architecture.md)
- [Rust weather actor example](../../../../rust/apps/weather_actor/)
- [Go weather actor example](../../../../go/apps/weather_actor/)
- [TypeScript weather actor example](../../../../typescript/apps/weather_actor/)
