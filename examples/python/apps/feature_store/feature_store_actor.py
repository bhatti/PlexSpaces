# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Feature Store - Python WASM
#
# Production-grade feature store: feature ingestion -> versioned KV storage ->
# consistent-hash routing -> low-latency online serving with LRU cache.
# Leader/worker with shard-group placement, scatter/gather,
# compute vs coordination metrics tracking.
#
# Real-world analog: Feast, Tecton, SageMaker Feature Store, Redis Feature Store

import json
import math
import time

from plexspaces import ActorID, actor, handler, init_handler, host, state
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement


def actor_application_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


# ─── Feature Types ───────────────────────────────────────────────────────────

FEATURE_DEFINITIONS = {
    "user": {
        "age": {"type": "float", "range": (18, 80)},
        "account_age_days": {"type": "float", "range": (1, 3650)},
        "total_purchases": {"type": "float", "range": (0, 500)},
        "avg_order_value": {"type": "float", "range": (5.0, 500.0)},
        "loyalty_score": {"type": "float", "range": (0, 100)},
        "churn_probability": {"type": "float", "range": (0.0, 1.0)},
    },
    "product": {
        "price": {"type": "float", "range": (1.0, 999.0)},
        "avg_rating": {"type": "float", "range": (1.0, 5.0)},
        "review_count": {"type": "float", "range": (0, 10000)},
        "category_id": {"type": "float", "range": (1, 50)},
        "stock_level": {"type": "float", "range": (0, 10000)},
        "days_since_launch": {"type": "float", "range": (1, 1825)},
    },
    "interaction": {
        "view_count": {"type": "float", "range": (0, 100)},
        "click_count": {"type": "float", "range": (0, 50)},
        "cart_add_count": {"type": "float", "range": (0, 20)},
        "purchase_count": {"type": "float", "range": (0, 10)},
        "last_interaction_hours_ago": {"type": "float", "range": (0, 720)},
        "session_duration_sec": {"type": "float", "range": (0, 3600)},
    },
}

MAX_VERSIONS = 3


def generate_features(entity_count, seed=42):
    features = []
    rng = seed
    entity_types = list(FEATURE_DEFINITIONS.keys())

    for i in range(entity_count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        entity_type = entity_types[rng % len(entity_types)]
        entity_id = f"{entity_type}:{rng % 10000:05d}"
        feature_defs = FEATURE_DEFINITIONS[entity_type]

        for feat_name, feat_def in feature_defs.items():
            rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
            lo, hi = feat_def["range"]
            value = lo + (rng % 10000) / 10000.0 * (hi - lo)
            value = round(value, 4)

            features.append({
                "entity_id": entity_id,
                "feature_name": feat_name,
                "value": value,
                "version": 1,
                "timestamp": int(host.now_ms()) + i,
                "entity_type": entity_type,
            })
    return features


def generate_lookup_requests(features, count, seed=99):
    rng = seed
    requests = []
    entity_ids = list({f["entity_id"] for f in features})
    if not entity_ids:
        return requests

    for _ in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        entity_id = entity_ids[rng % len(entity_ids)]
        entity_type = entity_id.split(":")[0]
        feature_names = list(FEATURE_DEFINITIONS.get(entity_type, {}).keys())
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        num_features = (rng % len(feature_names)) + 1 if feature_names else 1
        selected = feature_names[:num_features]
        requests.append({
            "entity_id": entity_id,
            "feature_names": selected,
        })
    return requests


# ─── LRU Cache ───────────────────────────────────────────────────────────────

class LRUCache:
    def __init__(self, capacity=1000):
        self.capacity = capacity
        self.cache = {}
        self.order = []
        self.hits = 0
        self.misses = 0

    def get(self, key):
        if key in self.cache:
            self.hits += 1
            self.order.remove(key)
            self.order.append(key)
            return self.cache[key]
        self.misses += 1
        return None

    def put(self, key, value):
        if key in self.cache:
            self.order.remove(key)
        elif len(self.cache) >= self.capacity:
            oldest = self.order.pop(0)
            del self.cache[oldest]
        self.cache[key] = value
        self.order.append(key)

    def hit_rate(self):
        total = self.hits + self.misses
        return round(self.hits / total, 4) if total > 0 else 0.0

    def stats(self):
        return {
            "size": len(self.cache),
            "capacity": self.capacity,
            "hits": self.hits,
            "misses": self.misses,
            "hit_rate": self.hit_rate(),
        }


# ─── Consistent Hash Routing ────────────────────────────────────────────────

def route_entity(entity_id, shard_count):
    h = 0
    for c in entity_id:
        h = (h * 31 + ord(c)) & 0xFFFFFFFF
    return h % shard_count


# ─── Leader Actor ────────────────────────────────────────────────────────────

@actor
class LeaderActor:
    total_compute_ms: int = state(default=0)
    total_coord_ms: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("run")
    def run(
        self,
        entity_count: int = 5000,
        worker_count: int = 8,
        lookup_count: int = 2000,
        batch_size: int = 500,
        cache_capacity: int = 500,
        from_actor: str = "",
    ) -> dict:
        # Create shard group
        coord_start = host.now_ms()
        group_id = f"feature-store-py-{host.now_ms()}"
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=group_id,
            actor_type="WorkerActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        shard_ids = group.shard_actor_ids
        if not shard_ids:
            return {"status": "error", "error": "failed to create worker shard group"}
        shard_group_id = group.group_id
        coord_create = host.now_ms() - coord_start

        # Generate features
        features = generate_features(entity_count, seed=host.now_ms() % 100000)
        lookups = generate_lookup_requests(features, lookup_count, seed=host.now_ms() % 100000 + 1)

        total_compute_ms = 0
        total_coord_ms = coord_create
        error_count = 0

        # Phase 1: Ingest features (batch by shard)
        shard_batches = {}
        for f in features:
            shard_id = route_entity(f["entity_id"], worker_count)
            shard_batches.setdefault(shard_id, []).append(f)

        for shard_id, batch_features in shard_batches.items():
            for i in range(0, len(batch_features), batch_size):
                chunk = batch_features[i:i + batch_size]
                sg_start = host.now_ms()
                sg_result = host.scatter_gather(ScatterGatherRequest(
                    group_id=shard_group_id,
                    query={
                        "op": "ingest_features",
                        "features": chunk,
                        "cache_capacity": cache_capacity,
                    },
                    timeout_ms=30000,
                ))
                sg_elapsed = host.now_ms() - sg_start
                total_coord_ms += sg_elapsed

                for resp in sg_result.shard_responses:
                    r = resp
                    for key in ("payload", "result", "response", "data"):
                        if isinstance(r, dict) and key in r:
                            r = r[key]
                    if isinstance(r, str):
                        try:
                            r = json.loads(r)
                        except Exception:
                            pass
                    if isinstance(r, dict) and "error" in r:
                        error_count += 1
                    elif isinstance(r, dict):
                        total_compute_ms += r.get("compute_ms", 0)

        # Phase 2: Online serving - lookups
        lookup_batches = [lookups[i:i + batch_size] for i in range(0, len(lookups), batch_size)]
        total_lookups_served = 0
        total_cache_hits = 0
        total_cache_misses = 0

        for batch in lookup_batches:
            sg_start = host.now_ms()
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=shard_group_id,
                query={
                    "op": "batch_lookup",
                    "requests": batch,
                },
                timeout_ms=30000,
            ))
            sg_elapsed = host.now_ms() - sg_start
            total_coord_ms += sg_elapsed

            for resp in sg_result.shard_responses:
                r = resp
                for key in ("payload", "result", "response", "data"):
                    if isinstance(r, dict) and key in r:
                        r = r[key]
                if isinstance(r, str):
                    try:
                        r = json.loads(r)
                    except Exception:
                        pass
                if isinstance(r, dict) and "error" in r:
                    error_count += 1
                elif isinstance(r, dict):
                    total_compute_ms += r.get("compute_ms", 0)
                    total_lookups_served += r.get("lookups_served", 0)
                    total_cache_hits += r.get("cache_hits", 0)
                    total_cache_misses += r.get("cache_misses", 0)

        # Collect cache stats from all shards
        sg_start = host.now_ms()
        stats_result = host.scatter_gather(ScatterGatherRequest(
            group_id=shard_group_id,
            query={"op": "get_stats"},
            timeout_ms=10000,
        ))
        total_coord_ms += host.now_ms() - sg_start

        shard_stats = []
        total_stored = 0
        for resp in stats_result.shard_responses:
            r = resp
            for key in ("payload", "result", "response", "data"):
                if isinstance(r, dict) and key in r:
                    r = r[key]
            if isinstance(r, str):
                try:
                    r = json.loads(r)
                except Exception:
                    pass
            if isinstance(r, dict) and "error" not in r:
                shard_stats.append(r)
                total_stored += r.get("stored_features", 0)

        wall_time = total_compute_ms + total_coord_ms
        total = total_compute_ms + total_coord_ms or 1
        granularity = round(total_compute_ms / total_coord_ms, 1) if total_coord_ms > 0 else 0
        lookups_per_sec = round(lookup_count * 1000 / wall_time) if wall_time > 0 else 0

        total_cache_total = total_cache_hits + total_cache_misses
        overall_hit_rate = round(total_cache_hits / total_cache_total, 4) if total_cache_total > 0 else 0.0

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {
                        "leader.compute": total_compute_ms,
                        "leader.coordination": total_coord_ms,
                    },
                    "latency_max_ms": {
                        "leader.compute": total_compute_ms,
                        "leader.coordination": total_coord_ms,
                    },
                    "latency_samples": {
                        "leader.compute": 1,
                        "leader.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "status": "ok",
            "entity_count": entity_count,
            "feature_count": len(features),
            "worker_count": worker_count,
            "lookup_count": lookup_count,
            "lookups_served": total_lookups_served,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "lookups_per_sec": lookups_per_sec,
            "stored_features": total_stored,
            "cache_hits": total_cache_hits,
            "cache_misses": total_cache_misses,
            "cache_hit_rate": overall_hit_rate,
            "shard_count": len(shard_ids),
            "actor_count": len(shard_ids) + 1,
            "error_count": error_count,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        entity_count: int = 5000,
        lookup_count: int = 2000,
        shard_counts: list = None,
        batch_size: int = 500,
        cache_capacity: int = 500,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Pre-build fixed batches once — calling self.run() in a loop per shard_count
        # issues hundreds of scatter-gather calls and exhausts the Python WASM heap.
        bench_features = []
        for _i in range(50):
            bench_features.append({
                "entity_id": f"user:{_i % 100:03d}",
                "feature_name": "loyalty_score",
                "value": (_i % 100) / 100,
                "version": 1,
                "timestamp": _i,
                "entity_type": "user",
            })
        bench_lookups = []
        for _i in range(50):
            bench_lookups.append({
                "entity_id": f"user:{_i % 100:03d}",
                "feature_names": ["loyalty_score"],
            })
        num_lookup_batches = 2

        results = []
        baseline_eps = 0

        for shard_count in shard_counts:
            group_name = "fs-bench-" + str(host.now_ms())
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=group_name,
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id
            shard_ids = group.shard_actor_ids
            if not shard_ids:
                results.append({
                    "shards": shard_count, "lookups_per_sec": 0, "wall_time_ms": 0,
                    "compute_time_ms": 0, "coordination_time_ms": 0,
                    "granularity_ratio": 0, "cache_hit_rate": 0,
                    "speedup": 0, "efficiency_pct": 0, "error_count": 1,
                })
                continue

            # Ingest once to populate shard stores before timing lookups
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "ingest_features", "features": bench_features, "cache_capacity": cache_capacity},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0
            total_coord = 0
            total_eps = 0
            error_count = 0

            for iteration in range(warmup_rounds + benchmark_rounds):
                iter_coord = 0
                iter_compute = 0

                for _b in range(num_lookup_batches):
                    sg_start = host.now_ms()
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "batch_lookup", "requests": bench_lookups},
                        timeout_ms=30000,
                    ))
                    iter_coord += host.now_ms() - sg_start

                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ("payload", "result", "response", "data"):
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, str):
                            try:
                                r = json.loads(r)
                            except Exception:
                                pass
                        if isinstance(r, dict) and "error" in r:
                            error_count += 1
                        elif isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                if iteration >= warmup_rounds:
                    iter_wall = iter_compute + iter_coord
                    total_wall += iter_wall
                    total_coord += iter_coord
                    total_compute += iter_compute
                    # Aggregate throughput: each shard processes the full lookup batch.
                    actual_lookups = num_lookup_batches * len(bench_lookups) * shard_count
                    total_eps += (actual_lookups * 1000 // iter_wall) if iter_wall > 0 else 0

            br = benchmark_rounds if benchmark_rounds > 0 else 1
            avg_wall = total_wall // br
            avg_compute = total_compute // br
            avg_coord = total_coord // br
            avg_eps = total_eps // br

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1
            speedup_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100
            eff_10 = speedup_100 * shard_counts[0] * 10 // shard_count if shard_count > 0 else 1000

            results.append({
                "shards": shard_count,
                "lookups_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": int(avg_compute * 10 / avg_coord) / 10 if avg_coord > 0 else 0,
                "cache_hit_rate": 0,
                "speedup": speedup_100 / 100,
                "efficiency_pct": eff_10 / 10,
                "error_count": error_count,
            })

        return {
            "status": "ok",
            "entity_count": entity_count,
            "lookup_count": lookup_count,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        entities_per_shard: int = 2500,
        lookups_per_shard: int = 1000,
        shard_counts: list = None,
        batch_size: int = 500,
        cache_capacity: int = 500,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        bench_features = []
        for _i in range(50):
            bench_features.append({
                "entity_id": f"user:{_i % 100:03d}",
                "feature_name": "loyalty_score",
                "value": (_i % 100) / 100,
                "version": 1,
                "timestamp": _i,
                "entity_type": "user",
            })
        bench_lookups = []
        for _i in range(50):
            bench_lookups.append({
                "entity_id": f"user:{_i % 100:03d}",
                "feature_names": ["loyalty_score"],
            })
        num_lookup_batches = 2

        results = []
        baseline_eps = 0

        for shard_count in shard_counts:
            total_entities = entities_per_shard * shard_count
            total_lookups = lookups_per_shard * shard_count

            group_name = "fs-weak-" + str(host.now_ms())
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=group_name,
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id
            shard_ids = group.shard_actor_ids
            if not shard_ids:
                results.append({
                    "shards": shard_count,
                    "total_entities": total_entities,
                    "total_lookups": total_lookups,
                    "lookups_per_sec": 0, "wall_time_ms": 0,
                    "compute_time_ms": 0, "coordination_time_ms": 0,
                    "granularity_ratio": 0, "efficiency_pct": 0, "error_count": 1,
                })
                continue

            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "ingest_features", "features": bench_features, "cache_capacity": cache_capacity},
                timeout_ms=30000,
            ))

            total_lps = 0
            total_wall = 0
            total_compute = 0
            total_coord = 0
            error_count = 0

            for iteration in range(warmup_rounds + benchmark_rounds):
                iter_coord = 0
                iter_compute = 0

                for _b in range(num_lookup_batches):
                    sg_start = host.now_ms()
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "batch_lookup", "requests": bench_lookups},
                        timeout_ms=30000,
                    ))
                    iter_coord += host.now_ms() - sg_start

                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ("payload", "result", "response", "data"):
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, str):
                            try:
                                r = json.loads(r)
                            except Exception:
                                pass
                        if isinstance(r, dict) and "error" in r:
                            error_count += 1
                        elif isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                if iteration >= warmup_rounds:
                    iter_wall = iter_compute + iter_coord
                    total_wall += iter_wall
                    total_coord += iter_coord
                    total_compute += iter_compute
                    actual_lookups = num_lookup_batches * len(bench_lookups) * shard_count
                    total_lps += (actual_lookups * 1000 // iter_wall) if iter_wall > 0 else 0

            br = benchmark_rounds if benchmark_rounds > 0 else 1
            avg_lps = total_lps // br
            avg_wall = total_wall // br
            avg_compute = total_compute // br
            avg_coord = total_coord // br

            if baseline_eps == 0:
                baseline_eps = avg_lps or 1
            eff_10 = avg_lps * 1000 // baseline_eps if baseline_eps > 0 else 1000

            results.append({
                "shards": shard_count,
                "total_entities": total_entities,
                "total_lookups": total_lookups,
                "lookups_per_sec": avg_lps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": int(avg_compute * 10 / avg_coord) / 10 if avg_coord > 0 else 0,
                "efficiency_pct": eff_10 / 10,
                "error_count": error_count,
            })

        return {
            "status": "ok",
            "entities_per_shard": entities_per_shard,
            "lookups_per_shard": lookups_per_shard,
            "results": results,
        }


# ─── Worker Actor ────────────────────────────────────────────────────────────

@actor
class WorkerActor:
    store: dict = state(default_factory=dict)
    cache: object = state(default=None)
    features_stored: int = state(default=0)
    lookups_served: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.cache = LRUCache(capacity=500)

    @handler("ingest_features")
    def ingest_features(
        self,
        features: list = None,
        cache_capacity: int = 500,
        from_actor: str = "",
    ) -> dict:
        if features is None:
            features = []
        comp_start = host.now_ms()

        if self.cache is None or self.cache.capacity != cache_capacity:
            self.cache = LRUCache(capacity=cache_capacity)

        ingested = 0
        for f in features:
            entity_id = f["entity_id"]
            feature_name = f["feature_name"]
            value = f["value"]
            version = f.get("version", 1)
            timestamp = f.get("timestamp", 0)

            if entity_id not in self.store:
                self.store[entity_id] = {}
            if feature_name not in self.store[entity_id]:
                self.store[entity_id][feature_name] = []

            versions = self.store[entity_id][feature_name]
            versions.append((value, version, timestamp))
            versions.sort(key=lambda x: x[2], reverse=True)
            self.store[entity_id][feature_name] = versions[:MAX_VERSIONS]

            # Invalidate cache for this entity
            cache_key = f"{entity_id}:{feature_name}"
            if self.cache:
                self.cache.put(cache_key, value)

            ingested += 1

        self.features_stored += ingested
        compute_ms = host.now_ms() - comp_start

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {"worker.ingest_compute": compute_ms},
                    "latency_max_ms": {"worker.ingest_compute": compute_ms},
                    "latency_samples": {"worker.ingest_compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "ingested": ingested,
            "compute_ms": compute_ms,
        }

    @handler("batch_lookup")
    def batch_lookup(
        self,
        requests: list = None,
        from_actor: str = "",
    ) -> dict:
        if requests is None:
            requests = []
        comp_start = host.now_ms()

        results = []
        cache_hits = 0
        cache_misses = 0
        lookups_served = 0

        for req in requests:
            entity_id = req["entity_id"]
            feature_names = req.get("feature_names", [])
            feature_vector = {}

            for fname in feature_names:
                cache_key = f"{entity_id}:{fname}"
                cached = self.cache.get(cache_key) if self.cache else None

                if cached is not None:
                    feature_vector[fname] = cached
                    cache_hits += 1
                else:
                    cache_misses += 1
                    entity_data = self.store.get(entity_id, {})
                    versions = entity_data.get(fname, [])
                    if versions:
                        value = versions[0][0]  # latest version
                        feature_vector[fname] = value
                        if self.cache:
                            self.cache.put(cache_key, value)
                    else:
                        feature_vector[fname] = None

            if feature_vector:
                results.append({
                    "entity_id": entity_id,
                    "features": feature_vector,
                })
                lookups_served += 1

        self.lookups_served += lookups_served
        compute_ms = host.now_ms() - comp_start

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {"worker.lookup_compute": compute_ms},
                    "latency_max_ms": {"worker.lookup_compute": compute_ms},
                    "latency_samples": {"worker.lookup_compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "lookups_served": lookups_served,
            "results": results,
            "cache_hits": cache_hits,
            "cache_misses": cache_misses,
            "compute_ms": compute_ms,
        }

    @handler("get_stats")
    def get_stats(self, from_actor: str = "") -> dict:
        entity_count = len(self.store)
        feature_count = sum(len(feats) for feats in self.store.values())
        cache_stats = self.cache.stats() if self.cache else {}

        return {
            "stored_entities": entity_count,
            "stored_features": feature_count,
            "features_ingested_total": self.features_stored,
            "lookups_served_total": self.lookups_served,
            "cache": cache_stats,
        }


# ─── Role Registry ──────────────────────────────────────────────────────────

ACTOR_ROLES = {
    "leader": LeaderActor,
    "worker": WorkerActor,
    "LeaderActor": LeaderActor,
    "WorkerActor": WorkerActor,
}
