# Weather Actor (Go) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** combined with **KV-based caching** in a Go WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `ServiceHTTPClient` | Ergonomic HTTP client backed by `"weather-api"` service link and shared protobuf HTTP models |
| KV caching | `host.KVGet` / `host.KVPut` cache protobuf-backed weather entries for 5 minutes |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Example harness | `./test.sh` runs contract tests, builds WASM, deploys the actor, exercises cache/API paths, and prints output + metrics |

## Run the example

```bash
cd examples/go/apps/weather_actor
./build.sh           # builds target/examples/go/weather_actor/weather_actor.wasm
./test.sh            # requires a running node with the weather-api service link
./test.sh 8092       # same, explicit HTTP port
./test.sh --contract-only
```

## Message API

| Message | Payload | Response |
|---------|---------|----------|
| `get_weather` | `WeatherRequest { city }` | `WeatherReply { city, temp_c, wind_kph, fetched_at_ms, source, error }` |
| `cache_stats` | empty payload | `CacheStats { hits, misses }` |
| `clear_cache` | empty payload | `ClearCacheReply { cleared }` |

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Python weather actor](../../../../python/apps/weather_actor/)
- [Rust weather actor](../../../../rust/apps/weather_actor/)
- [TypeScript weather actor](../../../../typescript/apps/weather_actor/)
