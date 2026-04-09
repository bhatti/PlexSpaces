# Erlang/OTP Rate Limiter - Sliding Window Rate Limiting

Demonstrates **Erlang/OTP GenServer** pattern for API gateway rate limiting.

**Real-world use case**: API rate limiting (NGINX, Kong, Envoy, Cloudflare) where
each client gets an independent sliding window with configurable request limits.

## Architecture

```
              ┌──────────────────────────────┐
              │   SlidingWindowLimiter       │
              │   (GenServer equivalent)      │
              │                              │
              │  ┌─────────────────────────┐ │
              │  │  Per-Client Windows     │ │
              │  │  client-1: [t1,t2,...tN] │ │
              │  │  client-2: [t1,t2,...tN] │ │
              │  │  client-3: [t1,t2,...tN] │ │
              │  └─────────────────────────┘ │
              └──────────────────────────────┘
                    │           │
          check_rate(client)  stats()
                    │           │
              ┌─────▼───────────▼──────┐
              │   API Gateway / Test   │
              └────────────────────────┘
```

**Rate limiting algorithm** (sliding window):
1. On each request, remove timestamps older than `window_size_ms`
2. If count < `max_requests`, allow and record timestamp
3. Otherwise deny with `retry_after_ms` header

## Quick Start

```bash
# Terminal 1: Start PlexSpaces node
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Terminal 2: Build and run
cd examples/go/apps/migrating_erlang_otp
./build.sh        # Builds rate_limiter.wasm (requires tinygo)
./test.sh 7993    # Deploy + test rate limiting + benchmarks
```

**Deploy `application_id` and `name`**: Use the same value for both. For WASM, the runtime derives the actor registry namespace from `ApplicationSpec.namespace`, then request namespace, then deploy `name` — not from `application_id` when the first two are empty. The HTTP path `/api/v1/actors/{namespace}/...` must use that namespace, so **app-id should map to namespace** by setting `name` equal to `application_id` (as `test.sh` does).

**Actor HTTP paths:** `/api/v1/actors/{namespace}/rate-limiter/ask` must use the same namespace WASM actors register under—the deploy multipart field `name` (see `APP_SPEC_NAME` in `test.sh`), not `application_id`.

## PlexSpaces SDK Features

| Feature | How Used |
|---------|----------|
| `BaseActor` | Go actor base with JSON state serialization |
| `Host.NowMs()` | Timestamps for sliding window + benchmarks |
| `Host.Info()` | Structured logging |
| `Init()` | Initialize from framework config (child_spec args) |
| `Handle()` | Routes `check_rate`, `stats`, `check_rate_batch` |
| `GetState()/SetState()` | Checkpoint-based state persistence |

## Comparison: Erlang/OTP vs PlexSpaces

| Feature | Erlang/OTP | PlexSpaces Go |
|---------|-----------|---------------|
| Actor model | `gen_server:start_link/3` | Supervisor ApplicationSpec |
| State | `#state{}` record | Go struct with JSON tags |
| Call | `gen_server:call(Pid, Msg)` | `host.Ask(actorID, msgType, data)` |
| Cast | `gen_server:cast(Pid, Msg)` | `host.Send(actorID, msgType, data)` |
| Supervision | `supervisor:start_link/2` | `app-config.toml` (one_for_one) |
| Hot code reload | `code_change/3` | Redeploy WASM module |
| Language | Erlang-only | Go, Python, TypeScript, Rust |
| Distribution | Erlang distribution | HTTP/gRPC (multi-cloud) |

## Files

| File | Description |
|------|-------------|
| `rate_limiter.go` | SlidingWindowLimiter actor |
| `go.mod` | Go module (references PlexSpaces SDK) |
| `app-config.toml` | ApplicationSpec (supervisor + rate limiter) |
| `build.sh` | Build WASM module via TinyGo |
| `test.sh` | Deploy + test + benchmarks |
| `native/rate_limiter.erl` | Native Erlang/OTP reference |
