# Feature Flags Service (Python WASM)

A feature flag management service for controlling feature rollouts, A/B testing, and kill switches.

**Real-world use case**: SaaS feature management (like LaunchDarkly, Split.io, Unleash).

## Supervisor Support

This actor is deployed with a **one-for-one supervisor** (built-in):
- **Strategy**: One-for-one (only restart the failed actor)
- **Max Restarts**: 5 restarts within the time window
- **Behavior**: If the actor crashes, the supervisor automatically restarts it

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

## What It Demonstrates

1. **Feature Toggles** - Enable/disable features at runtime
2. **Gradual Rollout** - Roll out features to X% of users
3. **Consistent Hashing** - Same user always gets same result
4. **Hierarchical Keys** - Organize flags by team/area (`checkout.new-flow`)
5. **Kill Switches** - Instantly disable problematic features

## Operations

### Create a Flag
```json
{"op": "create", "flag": "dark-mode"}
{"op": "create", "flag": "new-ui", "enabled": true, "rollout": 25}
```

### Enable/Disable
```json
{"op": "enable", "flag": "dark-mode"}
{"op": "disable", "flag": "dark-mode"}
```

### Set Rollout Percentage
```json
{"op": "rollout", "flag": "new-ui", "pct": 25}
```

### Check Flag for User
```json
{"op": "check", "flag": "new-ui", "user": "alice"}
```
Response:
```json
{"flag": "new-ui", "enabled": true, "bucket": 17, "rollout": 25}
```

### List Flags
```json
{"op": "list"}
{"op": "list", "prefix": "checkout"}
```

## Gradual Rollout Algorithm

Uses consistent hashing so the same user always gets the same result:

```python
# Simple hash (hashlib.md5 crashes in WASM - see Known Issues below)
h = 0
for c in (flag_name + user):
    h = (h + ord(c)) % 100
is_enabled = h < rollout_pct
```

## Example Scenario

```
1. Create flags for new checkout flow
2. Enable at 10% rollout (test with small group)
3. Monitor metrics, no issues
4. Increase to 25%, then 50%, then 100%
5. If issues arise, disable instantly (kill switch)
```

## Files

| File | Description |
|------|-------------|
| `feature_flags_actor.py` | Feature flags implementation |
| `build.sh` | Build with componentize-py |
| `test.sh` | Integration test |

## Known Issues: componentize-py Memory Bugs

The Python 3.14 runtime in componentize-py has memory management bugs that can cause
WASM actors to crash with errors like `tuple_dealloc`, `match_dealloc`, or `func_dealloc`.

### Workarounds Used in This Code

| Issue | Symptom | Workaround |
|-------|---------|------------|
| `hashlib.md5()` | `match_dealloc` crash | Use simple inline hash: `h = (h + ord(c)) % 100` |
| `json.dumps()` for simple values | `tuple_dealloc` crash | Use string literals: `'{"status":"ok"}'` |
| Helper functions | `func_dealloc` crash | Inline calculations instead of function calls |
| Nested try-except | Deallocation errors | Use flat if statements |
| Complex return values | Deallocation crash | Keep returned data simple and flat |

### Example of What to Avoid

```python
# ❌ CRASHES - hashlib causes match_dealloc
import hashlib
h = hashlib.md5((flag + user).encode()).hexdigest()

# ✅ WORKS - simple inline hash
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100

# ❌ MAY CRASH - json.dumps with complex nested data
return json.dumps({"status": "ok", "data": {"nested": "value"}})

# ✅ WORKS - string literal for simple responses
return '{"status":"ok"}'

# ❌ MAY CRASH - helper function call
def my_hash(s):
    return sum(ord(c) for c in s) % 100
h = my_hash(flag + user)

# ✅ WORKS - inline the logic
h = 0
for c in (flag + user):
    h = (h + ord(c)) % 100
```

### Status

These issues are related to componentize-py's Python 3.14 WASM runtime. Future versions
may fix these bugs. For now, follow the workarounds above for stable operation.

## See Also

- [Python WASM Guide](../../README.md)
- [Receipt Storage Example](../receipt_storage/) - Similar CRUD pattern
