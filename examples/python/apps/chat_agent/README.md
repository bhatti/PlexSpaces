# Chat Agent — Python WASM

<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

A minimal chat agent demonstrating **Cloudflare Agents SDK equivalence** in PlexSpaces Python. Conversation history lives in KV, LLM calls use a named service link, and a durable alarm triggers periodic summarization.

## Cloudflare Agents SDK vs PlexSpaces Python

| Cloudflare Agents SDK | PlexSpaces Python |
|---|---|
| `this.env.AI.run(model, {messages})` | `ServiceHttpClient("llm-link").post(...)` |
| `await this.storage.get('history')` | `host.kv.get_json("history")` |
| `await this.storage.put('history', v)` | `host.kv.put_json("history", v)` |
| `await this.state.storage.setAlarm(ts)` | `host.alarm.set(ts)` |
| `async onAlarm() { ... }` | `@handler("__alarm__")` (reminder facet) |
| `connection.send(reply)` | `return dict` (sync response) |
| `env.AI` binding in wrangler.toml | `[service_links.llm-link]` in app-config.toml |
| Durable Object per-agent | `virtual_actor` + `reminder` facets |

## Features

- **Conversation state** — history stored in KV per-agent via `host.kv.get_json/put_json`
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

- Python 3.11+ with PlexSpaces Python SDK
- `plexspaces-py` CLI for WASM build
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
