# Chat Agent — Rust WASM

<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

A minimal chat agent demonstrating **Cloudflare Agents SDK equivalence** in PlexSpaces Rust. Conversation history lives in KV, LLM calls use a named service link, and a durable alarm triggers periodic summarization.

## Cloudflare Agents SDK vs PlexSpaces Rust

| Cloudflare Agents SDK | PlexSpaces Rust |
|---|---|
| `this.env.AI.run(model, {messages})` | `host::http_fetch("llm-link", "POST", ...)` |
| `await this.storage.get('history')` | `kv_get_json::<Vec<ChatMessage>>("history")` |
| `await this.storage.put('history', v)` | `kv_put_json("history", &history)` |
| `await this.state.storage.setAlarm(ts)` | `alarm_set(ts)` |
| `async onAlarm() { ... }` | `handle "msg_type == __alarm__"` |
| `connection.send(reply)` | return `Vec<u8>` (JSON-encoded) |
| `env.AI` binding in wrangler.toml | `[service_links.llm-link]` in app-config.toml |
| Durable Object per-agent | `virtual_actor` + `reminder` facets |

## Features

- **Conversation state** — history stored in KV via `kv_get_json` / `kv_put_json`
- **LLM integration** — calls Anthropic (or any OpenAI-compatible API) via service link
- **Durable alarm** — after 10 messages, schedules summarization 5 minutes out
- **Summarization** — `__alarm__` handler compresses history into a `summary` KV key

## Handlers

| Op | Description |
|---|---|
| `chat` | Append user message, call LLM, store response |
| `get_history` | Return full conversation history from KV |
| `clear` | Clear history, summary, and pending alarm |
| `__alarm__` | Summarize history (fired by reminder facet) |

## Requirements

- Rust + wasm32-wasip1 target + wasm-tools
- `ANTHROPIC_API_KEY` environment variable (for real LLM calls)

## Build

```bash
./build.sh
```

## Test

```bash
./test.sh [PORT]
```

> LLM calls require a real API key. `test.sh` validates state/alarm only and passes without one.

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
