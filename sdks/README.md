# PlexSpaces SDKs

Language SDKs for building PlexSpaces actors with minimal boilerplate.

## Available SDKs

| Language | Status | Documentation |
|----------|--------|---------------|
| **Python** | ✅ Available | [sdks/python/README.md](python/README.md) |
| **TypeScript** | ✅ Available | [sdks/typescript/README.md](typescript/README.md) |
| Go | 📋 Planned | - |

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
├── python/             # Python SDK
│   ├── plexspaces/     # SDK package
│   ├── plexspaces_cli/ # CLI tools
│   ├── examples/       # Example actors
│   ├── tests/          # Unit tests
│   └── README.md       # Python SDK docs
├── typescript/         # TypeScript SDK (inheritance-based)
└── go/                 # Go SDK (planned)
```

## Contributing

See [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

LGPL-2.1-or-later
