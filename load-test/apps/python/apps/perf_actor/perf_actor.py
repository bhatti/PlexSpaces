# SPDX-License-Identifier: AGPL-3.0-or-later
# PerfActor — Python WASM actor for PlexSpaces load testing.
#
# Operations:
#   echo        — return payload unchanged (measures WASM + routing overhead, zero compute)
#   compute     — check if a number is a Mersenne prime (deterministic ~1ms CPU)
#   kv_put      — write key=value via KV host function
#   kv_get      — read key via KV host function
#   pg_broadcast— join a named process group then broadcast one message
#   shard_task  — receive a slice of numbers, compute partial gradient, return result
#   get_stats   — return internal counters for validation

from plexspaces import actor, state, handler, init_handler


def _is_mersenne_prime(p: int) -> bool:
    """Lucas-Lehmer test. Returns True if 2^p - 1 is prime."""
    if p == 2:
        return True
    if p < 2:
        return False
    mp = (1 << p) - 1
    s = 4
    for _ in range(p - 2):
        s = (s * s - 2) % mp
    return s == 0


def _gradient_step(values: list, lr: float = 0.01) -> dict:
    """Single gradient descent step on a list of floats. Returns partial gradient."""
    n = len(values)
    if n == 0:
        return {"gradient": 0.0, "count": 0}
    mean = sum(values) / n
    grad = sum((v - mean) ** 2 for v in values) / n
    updated = [v - lr * (v - mean) for v in values]
    return {"gradient": grad, "count": n, "mean": mean, "sample": updated[:3]}


@actor
class PerfActor:
    """Performance test actor — identical ops across all 5 languages."""

    echo_count: int = state(default=0)
    compute_count: int = state(default=0)
    kv_count: int = state(default=0)
    pg_count: int = state(default=0)
    shard_count: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict):
        self.actor_id = config.get("actor_id", "")

    @handler("echo")
    def echo(self, payload: dict = None) -> dict:
        self.echo_count += 1
        return {"ok": True, "echo": payload, "count": self.echo_count}

    @handler("compute")
    def compute(self, p: int = 7) -> dict:
        result = _is_mersenne_prime(p)
        self.compute_count += 1
        return {"ok": True, "p": p, "is_mersenne_prime": result, "count": self.compute_count}

    @handler("kv_put")
    def kv_put(self, key: str = "perf_key", value: str = "perf_val") -> dict:
        from plexspaces import host
        host.kv.put(key, value)
        self.kv_count += 1
        return {"ok": True, "key": key, "count": self.kv_count}

    @handler("kv_get")
    def kv_get(self, key: str = "perf_key") -> dict:
        from plexspaces import host
        val = host.kv.get(key)
        return {"ok": True, "key": key, "value": val}

    @handler("pg_broadcast")
    def pg_broadcast(self, group: str = "perf-group", message: dict = None) -> dict:
        from plexspaces import host
        host.actor.pg_join(group)
        host.actor.pg_broadcast(group, "perf_event", message or {"event": "ping"})
        self.pg_count += 1
        return {"ok": True, "group": group, "count": self.pg_count}

    @handler("shard_task")
    def shard_task(self, shard_index: int = 0, values: list = None, lr: float = 0.01) -> dict:
        if values is None:
            values = list(range(100))
        result = _gradient_step(values, lr)
        self.shard_count += 1
        return {"ok": True, "shard_index": shard_index, "count": self.shard_count, **result}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "ok": True,
            "echo_count": self.echo_count,
            "compute_count": self.compute_count,
            "kv_count": self.kv_count,
            "pg_count": self.pg_count,
            "shard_count": self.shard_count,
        }
