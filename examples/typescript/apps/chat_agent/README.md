# Chat Agent — TypeScript WASM

<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

A minimal chat agent that demonstrates **Cloudflare Agents SDK equivalence** in PlexSpaces TypeScript. Conversation history lives in KV, LLM calls go through a named service link, and a durable alarm triggers periodic summarization.

## Cloudflare Agents SDK vs PlexSpaces

| Cloudflare Agents SDK | PlexSpaces TypeScript |
|---|---|
| `this.env.AI.run(model, {messages})` | `host.httpClient("llm-link").fetch(...)` |
| `await this.storage.get('history')` | `host.kv.get("history")` |
| `await this.storage.put('history', v)` | `host.kv.put("history", JSON.stringify(v))` |
| `await this.state.storage.setAlarm(ts)` | `host.alarm.set(ts)` |
| `async onAlarm() { ... }` | `on__alarm__()` handler (reminder facet) |
| `connection.send(reply)` | `return { reply }` (sync response) |
| `Agent.schedule(cron, ...)` | `host.alarm.set(nextRunMs)` inside `__alarm__` |
| `env.AI` binding in wrangler.toml | `[service_links.llm-link]` in app-config.toml |
| Durable Object per-agent | `virtual_actor` + `reminder` facets |

## Features

- **Conversation state** — history stored in KV per-agent, survives restarts
- **LLM integration** — calls Anthropic (or any OpenAI-compatible endpoint) via service link
- **Durable alarm** — after 10 messages, schedules a summarization alarm 5 minutes out
- **Summarization** — `__alarm__` handler compresses history into a `summary` KV key

## Handlers

| Op | Equivalent | Description |
|---|---|---|
| `chat` | `onMessage()` | Append user message, call LLM, store response |
| `get_history` | `storage.get('history')` | Return full conversation history |
| `clear` | `storage.delete` + `deleteAlarm` | Clear history, summary, and pending alarm |
| `__alarm__` | `onAlarm()` | Summarize and clear history |

## Requirements

- PlexSpaces runtime
- Node.js + npm (for build)
- `ANTHROPIC_API_KEY` environment variable (for real LLM calls)

## Build

```bash
./build.sh
```

## Test

```bash
# With a running PlexSpaces node (LLM key optional — test uses placeholder)
./test.sh [PORT]
```

> LLM calls require a real API key configured in `service_links`. `test.sh` tests state and alarm logic only and passes without an API key.

## References

- [Architecture](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Cloudflare Agents SDK migration guide](../../../../docs/detailed-design.md)
