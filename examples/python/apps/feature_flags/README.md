# Feature Flags Service (Python WASM with SDK)

A feature flag management service for controlling feature rollouts, A/B testing, and kill switches.

**Real-world use case**: SaaS feature management (like LaunchDarkly, Split.io, Unleash).

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, state, handler

@actor
class FeatureFlagsService:
    flags: dict = state(default_factory=dict)
    
    @handler("create")
    def create_flag(self, flag: str = "") -> dict:
        self.flags[flag] = {"enabled": False, "rollout": 100}
        return {"status": "ok"}
```

**Before SDK**: 135 lines with manual WIT interface  
**After SDK**: 95 lines with decorators

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| create | `{"flag": "dark-mode"}` | Create flag |
| enable | `{"flag": "dark-mode"}` | Enable flag |
| disable | `{"flag": "dark-mode"}` | Disable flag |
| rollout | `{"flag": "new-ui", "pct": 25}` | Set rollout % |
| check | `{"flag": "new-ui", "user": "alice"}` | Check for user |
| list | `{}` | List all flags |
| delete | `{"flag": "dark-mode"}` | Delete flag |

## Gradual Rollout

Uses consistent hashing so the same user always gets the same result:

```python
# User "alice" checking "new-ui" at 25% rollout
h = hash(flag + user) % 100  # 0-99
enabled = h < 25  # True if bucket < rollout
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `FeatureFlagsService` as actor |
| `state()` | Defines `flags` as persistent dict |
| `@handler()` | Routes create, enable, check, etc. |

## Files

| File | Description |
|------|-------------|
| `feature_flags_actor.py` | Feature flags using SDK |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [Receipt Storage Example](../receipt_storage/) - Similar CRUD pattern
