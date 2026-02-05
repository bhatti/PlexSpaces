# Bank Account - Durability Example (TypeScript)

Same **real-world use case** as the [Python bank_account](../../../python/apps/bank_account/README.md): durable bank account actor with balance, deposit, withdraw, transaction history, and replay. Use case: banking, wallets, financial ledgers.

This example mirrors the Python app: same API and operations. **Verification** runs in Node (no server) via `verify.mjs`. **E2E** runs TypeScript-only: build WASM with `jco componentize` (no Python), deploy to a PlexSpaces node, then HTTP operations.

## Overview

- **account_actor.ts** – Uses [@plexspaces/sdk](../../../../sdks/typescript/README.md): extends `PlexSpacesActor`, implements `onDeposit`, `onWithdraw`, `onBalance`, etc. Same API as Python.
- **app-config.toml** – Same supervisor layout as Python (3 accounts).
- **verify.mjs** – In-process Node script that runs the actor logic and asserts results (no WASM, no server).
- **scripts/build.sh** – tsc → account_actor.js; esbuild bundle → account_actor_bundle.mjs; jco componentize bundle → account_actor.wasm (--disable all).
- **test.sh** – Full E2E: start node → build TypeScript WASM → deploy → HTTP operations (no Python).

## Prerequisites

- **Node.js** (v18+) and **npm** (TypeScript and jco; run `npm install` in this directory).
- **PlexSpaces repo** for E2E: `scripts/server.sh` starts a node (HTTP 8092, gRPC 8091). Run `make build` at repo root first.

## Quick Start (full E2E)

From the **repo root** (so `scripts/server.sh` and `make build` are available):

```bash
cd examples/typescript/apps/bank_account
./test.sh
```

**test.sh** runs: start PlexSpaces node → build TypeScript→WASM (jco) → deploy → HTTP banking operations → cleanup. No Python required.

## Operations (same as Python)

| Operation   | Payload                          | Response |
|------------|-----------------------------------|----------|
| Deposit    | `{"op":"deposit","amount":1000}`  | `{"status":"ok","balance":1000}` |
| Withdraw   | `{"op":"withdraw","amount":200}` | `{"status":"ok","balance":800}` |
| Balance    | `{"op":"balance"}`               | `{"account":"...","balance":800}` |
| History    | `{"op":"history","count":5}`       | `{"transactions":[...]}` |
| Replay     | `{"op":"replay"}`                | `{"replayed":N,"rebuilt_balance":...}` |

## Scripts (pattern from nbody_wasm)

| Script | Description |
|--------|-------------|
| **test.sh** | Full E2E: start node (scripts/server.sh), build WASM, deploy, HTTP tests |
| scripts/start_server.sh | Start empty node via repo `scripts/server.sh` (gRPC 8091, HTTP 8092) |
| scripts/build.sh | TypeScript → JS (tsc) then jco componentize → account_actor.wasm (no Python) |
| scripts/deploy.sh | Deploy WASM to running node (curl POST) |
| scripts/test_e2e.sh | Full E2E flow (start, build, deploy, HTTP ops, cleanup) |
| verify.mjs | In-process verification only (no node) |

## Files

| File             | Description |
|------------------|-------------|
| account_actor.ts | Bank account logic via @plexspaces/sdk (extends PlexSpacesActor) |
| account_actor_bundle.mjs | ESM bundle (actor + SDK) for jco componentize |
| build-bundle.mjs | Script to produce account_actor_bundle.mjs (esbuild) |
| app-config.toml | Supervisor config for 3 accounts |
| test.sh         | Runs scripts/test_e2e.sh (full E2E) |
| verify.mjs      | Node script that asserts actor behavior (no server) |

## See Also

- [TypeScript SDK](../../../../sdks/typescript/README.md) – Inheritance-based actor base class
- [Python bank_account](../../../python/apps/bank_account/README.md) – Same use case with Python SDK
- [PlexSpaces Python SDK](../../../../sdks/python/README.md)
- [docs/sdk.md](../../../../docs/sdk.md) – SDK reference

## License

LGPL-2.1-or-later
