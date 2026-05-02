# Weather Actor (Python) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** combined with **KV-based caching** in a Python WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `ServiceHttpClient` | Ergonomic HTTP client backed by `"weather-api"` service link and shared protobuf HTTP models |
| KV caching | `host.kv_get` / `host.kv_put` cache protobuf-backed weather entries for 5 minutes |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Required link validation | `[[applications.required_service_links]]` in `app-config.toml` |
| Example harness | `./test.sh` runs contract tests, builds WASM, deploys the actor, exercises cache/API paths, and prints output + metrics |

## Run the example

```bash
cd examples/python/apps/weather_actor
./test.sh            # requires a running node with the weather-api service link
./test.sh 8091       # same, explicit HTTP port
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
| `get_weather` | `WeatherRequest { city }` | `WeatherReply { city, temp_c, wind_kph, fetched_at_ms, source, error }` |
| `cache_stats` | empty payload | `CacheStats { hits, misses }` |
| `clear_cache` | empty payload | `ClearCacheReply { cleared }` |

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Architecture overview](../../../../../docs/architecture.md)
- [Rust weather actor example](../../../../rust/apps/weather_actor/)
- [Go weather actor example](../../../../go/apps/weather_actor/)
- [TypeScript weather actor example](../../../../typescript/apps/weather_actor/)
