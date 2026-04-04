# Weather Actor (TypeScript) — Outbound HTTP + KV Cache

Demonstrates **outbound HTTP via a named service link** combined with **KV-based caching** in a TypeScript WASM actor.

## What it shows

| Feature | Details |
|---------|---------|
| `ServiceHttpClient` | Ergonomic HTTP client backed by `"weather-api"` service link |
| KV caching | `host.kvGet` / `host.kvPut` to cache results for 5 minutes |
| Service link config | `[[runtime.service_links]]` in `release.toml` |
| Example harness | `./test.sh` runs contract tests, builds WASM, deploys the actor, exercises cache/API paths, and prints output + metrics |

## Run the example

```bash
cd examples/typescript/apps/weather_actor
./build.sh           # builds target/examples/typescript/weather_actor/weather_actor.wasm
./test.sh            # requires a running node with the weather-api service link
./test.sh 8092       # same, explicit HTTP port
./test.sh --contract-only
```

## Message API

| Message | Payload | Response |
|---------|---------|----------|
| `get_weather` | `{"city":"London"}` | `{"city","temp_c","wind_kph","source"}` |
| `cache_stats` | `{}` | `{"hits","misses"}` |
| `clear_cache` | `{}` | `{"cleared":true}` |

## Further reading

- [Service links reference](../../../../../docs/services.md)
- [Python weather actor](../../../../python/apps/weather_actor/)
- [Rust weather actor](../../../../rust/apps/weather_actor/)
- [Go weather actor](../../../../go/apps/weather_actor/)
