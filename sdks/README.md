# PlexSpaces SDKs

Language SDKs for building PlexSpaces actors with minimal boilerplate.

## Available SDKs

| Language | Status | Documentation |
|----------|--------|---------------|
| **Python** | ✅ Available | [sdks/python/README.md](python/README.md) |
| **TypeScript** | ✅ Available | [sdks/typescript/README.md](typescript/README.md) |
| **Rust** | ✅ Available | [sdks/rust/plexspaces-sdk/README.md](rust/plexspaces-sdk/README.md) (workspace crate; native + annotations) |
| **Go** | ✅ Available | [sdks/go/README.md](go/README.md) |

## Proto code generation

SDK typed models are generated from the same `proto/` tree as the Rust crates. From the repository root:

- **`make proto`** — Rust + Python + TypeScript + Go
- **`make proto-polyglot`** — Python, TypeScript, Go only
- **`make proto-install-deps`** — one-time local plugins (Python venv, `ts-proto`, `protoc-gen-go`)

Details: [docs/sdk.md](../docs/sdk.md) (sections *Proto generation and typed SDK models* and *Virtual actor type registration*).

## Tests

Repository root **`make test`** runs the full Cargo workspace **and** `go test ./...` (under `sdks/go`), **`npm test`** (under `sdks/typescript`), and **`pytest`** for `sdks/python` when a suitable Python environment is configured (`VENV_PATH` / system pytest). See [docs/testing.md](../docs/testing.md).

## Design Philosophy

Inspired by industry-leading frameworks:

- **[Ray](https://docs.ray.io/en/latest/ray-core/api/doc/ray.remote.html)** - `@ray.remote` decorator
- **[Temporal](https://docs.temporal.io/)** - `@workflow.defn`, `@activity.defn`
- **[Orleans](https://learn.microsoft.com/en-us/dotnet/orleans/)** - Grain interfaces
- **[Dapr](https://docs.dapr.io/)** - Actor building blocks

Our goal: **Write business logic, not boilerplate.**

## Python SDK

```python
from plexspaces import actor, state, handler

@actor
class Counter:
    count: int = state(default=0)
    
    @handler("increment")
    def increment(self, amount: int = 1) -> dict:
        self.count += amount
        return {"count": self.count}
```

Build:
```bash
plexspaces-py build counter.py -o counter_actor.wasm
```

See [Python SDK README](python/README.md) for full documentation.

## Directory Structure

```
sdks/
├── README.md           # This file
├── python/             # Python SDK (pytest; generated protos under plexspaces/generated/)
├── typescript/         # TypeScript SDK (npm test; generated protos under src/generated/proto/)
├── rust/               # Rust SDK (cargo, workspace members plexspaces-sdk + macros)
└── go/                 # Go SDK (go test; generated protos under plexspaces/proto/)
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

AGPL-3.0-or-later
