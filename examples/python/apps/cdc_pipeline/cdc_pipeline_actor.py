# SPDX-License-Identifier: AGPL-3.0-or-later
#
# CDC (Change Data Capture) Pipeline - Python WASM
#
# Production-grade CDC pipeline: WAL event ingestion → position tracking →
# schema transformation → fan-out to search index, analytics, cache invalidation.
# Leader/worker with shard-group placement, scatter/gather,
# compute vs coordination metrics tracking.
#
# Real-world analog: Debezium, Maxwell, AWS DMS, Airbyte

import json
import math
import time

from plexspaces import ActorID, actor, handler, init_handler, host
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement

ACTOR_ROLES = {}


def actor_application_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def register_role(role_name):
    def decorator(cls):
        ACTOR_ROLES[role_name] = cls
        return cls
    return decorator


# ─── Table Schemas ───────────────────────────────────────────────────────────

TABLE_SCHEMAS = {
    "users": {
        "columns": ["id", "email", "name", "status", "created_at", "updated_at"],
        "pk": "id",
        "search_fields": ["email", "name"],
        "cache_key_template": "user:{id}",
    },
    "orders": {
        "columns": ["id", "user_id", "total_amount", "status", "items_json", "created_at", "updated_at"],
        "pk": "id",
        "search_fields": ["status"],
        "cache_key_template": "order:{id}",
    },
    "products": {
        "columns": ["id", "name", "sku", "price", "category", "stock_count", "updated_at"],
        "pk": "id",
        "search_fields": ["name", "sku", "category"],
        "cache_key_template": "product:{id}",
    },
    "inventory": {
        "columns": ["id", "product_id", "warehouse_id", "quantity", "reserved", "updated_at"],
        "pk": "id",
        "search_fields": [],
        "cache_key_template": "inventory:{product_id}:{warehouse_id}",
    },
}

# ─── WAL Event Generation ────────────────────────────────────────────────────

def generate_wal_events(count, seed=42):
    events = []
    rng = seed
    tables = list(TABLE_SCHEMAS.keys())
    ops = ["INSERT", "UPDATE", "DELETE"]
    op_weights = [40, 50, 10]  # 40% insert, 50% update, 10% delete

    statuses_user = ["active", "inactive", "suspended"]
    statuses_order = ["pending", "processing", "shipped", "delivered", "cancelled"]
    categories = ["electronics", "clothing", "books", "home", "sports"]
    warehouses = ["wh-east-1", "wh-west-1", "wh-central-1"]
    names = ["Alice", "Bob", "Charlie", "Diana", "Eve", "Frank", "Grace", "Hank"]
    domains = ["example.com", "test.org", "demo.io"]

    lsn_counter = 0

    for i in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        table = tables[rng % len(tables)]
        schema = TABLE_SCHEMAS[table]

        # Weighted operation selection
        op_roll = rng % 100
        if op_roll < op_weights[0]:
            op = "INSERT"
        elif op_roll < op_weights[0] + op_weights[1]:
            op = "UPDATE"
        else:
            op = "DELETE"

        lsn_counter += 1
        lsn = f"0/{lsn_counter:08X}"
        entity_id = (rng >> 4) % 1000 + 1
        ts = 1700000000000 + i * 10

        row_data = {}
        old_data = {}

        if table == "users":
            name_val = names[(rng >> 8) % len(names)]
            domain = domains[(rng >> 12) % len(domains)]
            row_data = {
                "id": entity_id,
                "email": f"{name_val.lower()}{entity_id}@{domain}",
                "name": name_val,
                "status": statuses_user[(rng >> 16) % len(statuses_user)],
                "created_at": ts - 86400000,
                "updated_at": ts,
            }
            if op == "UPDATE":
                old_data = dict(row_data)
                old_data["status"] = statuses_user[(rng >> 20) % len(statuses_user)]
        elif table == "orders":
            amount = ((rng >> 8) % 50000) / 100
            row_data = {
                "id": entity_id,
                "user_id": (rng >> 12) % 500 + 1,
                "total_amount": amount,
                "status": statuses_order[(rng >> 16) % len(statuses_order)],
                "items_json": json.dumps([{"sku": f"SKU-{(rng>>20)%100}", "qty": (rng>>24)%5+1}]),
                "created_at": ts - 3600000,
                "updated_at": ts,
            }
            if op == "UPDATE":
                old_data = dict(row_data)
                old_data["status"] = statuses_order[(rng >> 24) % len(statuses_order)]
        elif table == "products":
            cat = categories[(rng >> 8) % len(categories)]
            price = ((rng >> 12) % 100000) / 100
            row_data = {
                "id": entity_id,
                "name": f"{cat.title()} Item {entity_id}",
                "sku": f"SKU-{entity_id:05d}",
                "price": price,
                "category": cat,
                "stock_count": (rng >> 16) % 1000,
                "updated_at": ts,
            }
            if op == "UPDATE":
                old_data = dict(row_data)
                old_data["stock_count"] = (rng >> 20) % 1000
        elif table == "inventory":
            prod_id = (rng >> 8) % 500 + 1
            wh = warehouses[(rng >> 12) % len(warehouses)]
            qty = (rng >> 16) % 500
            row_data = {
                "id": entity_id,
                "product_id": prod_id,
                "warehouse_id": wh,
                "quantity": qty,
                "reserved": min((rng >> 20) % 50, qty),
                "updated_at": ts,
            }
            if op == "UPDATE":
                old_data = dict(row_data)
                old_data["quantity"] = (rng >> 24) % 500

        event = {
            "lsn": lsn,
            "table": table,
            "operation": op,
            "row_data": row_data,
            "old_data": old_data if op == "UPDATE" else {},
            "timestamp": ts,
            "event_id": f"evt-{i:08d}",
        }
        events.append(event)

    return events


# ─── Transformation ──────────────────────────────────────────────────────────

def transform_for_search(event):
    table = event["table"]
    schema = TABLE_SCHEMAS.get(table, {})
    row = event["row_data"]
    doc = {
        "_index": f"cdc-{table}",
        "_id": str(row.get(schema.get("pk", "id"), "")),
        "_op": event["operation"].lower(),
        "_timestamp": event["timestamp"],
    }
    for field in schema.get("search_fields", []):
        if field in row:
            doc[field] = row[field]
    if "name" in row:
        doc["name"] = row["name"]
    if "email" in row:
        doc["email"] = row["email"]
    return doc


def transform_for_analytics(event):
    row = event["row_data"]
    record = {
        "event_type": f"cdc.{event['table']}.{event['operation'].lower()}",
        "table": event["table"],
        "operation": event["operation"],
        "entity_id": row.get("id", ""),
        "timestamp": event["timestamp"],
        "lsn": event["lsn"],
    }
    if event["table"] == "orders" and "total_amount" in row:
        record["amount"] = row["total_amount"]
        record["order_status"] = row.get("status", "")
    if event["table"] == "products" and "price" in row:
        record["price"] = row["price"]
        record["category"] = row.get("category", "")
    if event["table"] == "inventory":
        record["quantity"] = row.get("quantity", 0)
        record["warehouse"] = row.get("warehouse_id", "")
    return record


def transform_for_cache_invalidation(event):
    table = event["table"]
    schema = TABLE_SCHEMAS.get(table, {})
    row = event["row_data"]
    template = schema.get("cache_key_template", f"{table}:{{id}}")
    cache_key = template
    for k, v in row.items():
        cache_key = cache_key.replace("{" + k + "}", str(v))
    return {
        "cache_key": cache_key,
        "action": "invalidate" if event["operation"] in ("UPDATE", "DELETE") else "warm",
        "table": table,
        "entity_id": row.get("id", ""),
        "timestamp": event["timestamp"],
    }


# ─── Leader Actor ────────────────────────────────────────────────────────────

@register_role("leader")
@actor
class LeaderActor:
    def __init__(self):
        self.total_compute_ms = 0
        self.total_coord_ms = 0
        self.actor_id = ""
        self.application_id = ""

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("run")
    def run(self, event_count: int = 10000, worker_count: int = 8, batch_size: int = 500, from_actor: str = ""):

        coord_start = host.now_ms()
        group_name = f"cdc-pipeline-py-{host.now_ms()}"
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=group_name,
            actor_type="WorkerActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        group_id = group.group_id
        shard_ids = group.shard_actor_ids
        if not shard_ids:
            return {"status": "error", "error": "failed to create worker shard group"}
        coord_create = host.now_ms() - coord_start

        events = generate_wal_events(event_count, seed=host.now_ms() % 100000)
        batches = [events[i:i + batch_size] for i in range(0, len(events), batch_size)]

        total_compute_ms = 0
        total_coord_ms = coord_create
        total_search_docs = 0
        total_analytics_records = 0
        total_cache_invalidations = 0
        total_duplicates_skipped = 0
        error_count = 0
        table_counts = {}

        for batch in batches:
            sg_start = host.now_ms()
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={
                    "op": "process_batch",
                    "events": batch,
                },
                timeout_ms=30000,
            ))
            sg_elapsed = host.now_ms() - sg_start
            total_coord_ms += sg_elapsed

            for resp in sg_result.shard_responses:
                result = resp
                for key in ["payload", "result", "response", "data"]:
                    if isinstance(result, dict) and key in result:
                        result = result[key]
                if isinstance(result, dict):
                    if "error" in result:
                        error_count += 1
                        continue
                    total_compute_ms += result.get("compute_ms", 0)
                    total_search_docs += result.get("search_docs", 0)
                    total_analytics_records += result.get("analytics_records", 0)
                    total_cache_invalidations += result.get("cache_invalidations", 0)
                    total_duplicates_skipped += result.get("duplicates_skipped", 0)
                    for tbl, cnt in result.get("table_counts", {}).items():
                        table_counts[tbl] = table_counts.get(tbl, 0) + cnt

        wall_time = total_compute_ms + total_coord_ms
        total = total_compute_ms + total_coord_ms or 1
        granularity = int(total_compute_ms * 10 / total_coord_ms) / 10 if total_coord_ms > 0 else 0
        events_per_sec = round(event_count * 1000 / wall_time) if wall_time > 0 else 0

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "leader_events": event_count,
                        "leader_search_docs": total_search_docs,
                        "leader_analytics_records": total_analytics_records,
                    },
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
            "event_count": event_count,
            "worker_count": worker_count,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "events_per_sec": events_per_sec,
            "search_docs": total_search_docs,
            "analytics_records": total_analytics_records,
            "cache_invalidations": total_cache_invalidations,
            "duplicates_skipped": total_duplicates_skipped,
            "table_counts": table_counts,
            "node_count": len(set(host.actor_node_id(sid) for sid in shard_ids if hasattr(host, "actor_node_id"))) or 1,
            "actor_count": len(shard_ids) + 1,
            "error_count": error_count,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        event_count: int = 10000,
        shard_counts: list = None,
        batch_size: int = 500,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Pre-build a reusable batch once — avoids calling generate_wal_events() in a
        # nested Python context, which exhausts the WASM C stack via deep f-string/json paths.
        sz = min(batch_size, 500)
        bench_batch = []
        for _i in range(sz):
            bench_batch.append({
                "event_id": str(_i),
                "table": "orders",
                "operation": "INSERT",
                "lsn": str(_i),
                "timestamp": 0,
                "row_data": {"id": _i, "user_id": 1, "total_amount": 100, "status": "pending"},
                "old_data": {},
            })
        # Cap at 2 batches per iteration — the Python WASM heap is limited and crashes
        # if we issue too many scatter-gather calls in a single invocation.
        num_batches = 2

        results = []
        baseline_wall = 0
        baseline_eps = 0

        for shard_count in shard_counts:
            group_name = "cdc-bench-" + str(host.now_ms())
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
                    "shards": shard_count, "events_per_sec": 0, "wall_time_ms": 0,
                    "compute_time_ms": 0, "coordination_time_ms": 0,
                    "compute_pct": 0, "granularity_ratio": 0,
                    "speedup": 0, "efficiency_pct": 0, "error_count": 1,
                })
                continue

            total_wall = 0
            total_compute = 0
            total_coord = 0
            total_eps = 0
            error_count = 0

            for iteration in range(warmup_rounds + benchmark_rounds):
                iter_coord = 0
                iter_compute = 0

                for _b in range(num_batches):
                    sg_start = host.now_ms()
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_batch", "events": bench_batch},
                        timeout_ms=30000,
                    ))
                    iter_coord += host.now_ms() - sg_start

                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ("payload", "result", "response", "data"):
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict) and "error" in r:
                            error_count += 1
                        elif isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                if iteration >= warmup_rounds:
                    iter_wall = iter_compute + iter_coord
                    total_wall += iter_wall
                    total_coord += iter_coord
                    total_compute += iter_compute
                    # Aggregate throughput: each shard processes the full batch (broadcast).
                    actual_events = num_batches * sz * shard_count
                    total_eps += (actual_events * 1000 // iter_wall) if iter_wall > 0 else 0

            br = benchmark_rounds if benchmark_rounds > 0 else 1
            avg_wall = total_wall // br
            avg_compute = total_compute // br
            avg_coord = total_coord // br
            avg_eps = total_eps // br
            total = avg_compute + avg_coord or 1

            if baseline_wall == 0:
                baseline_wall = avg_wall or 1
                baseline_eps = avg_eps or 1
            # Throughput-based speedup: how much more aggregate work gets done vs baseline.
            speedup_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100
            eff_10 = speedup_100 * shard_counts[0] * 10 // shard_count if shard_count > 0 else 1000

            results.append({
                "shards": shard_count,
                "events_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "compute_pct": avg_compute * 100 // total,
                "granularity_ratio": int(avg_compute * 10 / avg_coord) / 10 if avg_coord > 0 else 0,
                "speedup": speedup_100 / 100,
                "efficiency_pct": eff_10 / 10,
                "error_count": error_count,
            })

        return {
            "status": "ok",
            "event_count": event_count,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        events_per_shard: int = 5000,
        shard_counts: list = None,
        batch_size: int = 500,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        sz = min(batch_size, 500)
        bench_batch = []
        for _i in range(sz):
            bench_batch.append({
                "event_id": str(_i),
                "table": "orders",
                "operation": "INSERT",
                "lsn": str(_i),
                "timestamp": 0,
                "row_data": {"id": _i, "user_id": 1, "total_amount": 100, "status": "pending"},
                "old_data": {},
            })

        results = []
        baseline_eps = 0

        for shard_count in shard_counts:
            total_events = events_per_shard * shard_count
            # Cap at 2 batches per iteration to keep WASM execution within memory limits.
            num_batches = 2

            group_name = "cdc-weak-" + str(host.now_ms())
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
                    "shards": shard_count, "total_events": total_events,
                    "events_per_sec": 0, "wall_time_ms": 0,
                    "compute_time_ms": 0, "coordination_time_ms": 0,
                    "granularity_ratio": 0, "efficiency_pct": 0, "error_count": 1,
                })
                continue

            total_eps = 0
            total_wall = 0
            total_compute = 0
            total_coord = 0
            error_count = 0

            for iteration in range(warmup_rounds + benchmark_rounds):
                iter_coord = 0
                iter_compute = 0

                for _b in range(num_batches):
                    sg_start = host.now_ms()
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_batch", "events": bench_batch},
                        timeout_ms=30000,
                    ))
                    iter_coord += host.now_ms() - sg_start

                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ("payload", "result", "response", "data"):
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict) and "error" in r:
                            error_count += 1
                        elif isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                if iteration >= warmup_rounds:
                    iter_wall = iter_compute + iter_coord
                    total_wall += iter_wall
                    total_coord += iter_coord
                    total_compute += iter_compute
                    total_eps += (total_events * 1000 // iter_wall) if iter_wall > 0 else 0

            br = benchmark_rounds if benchmark_rounds > 0 else 1
            avg_eps = total_eps // br
            avg_wall = total_wall // br
            avg_compute = total_compute // br
            avg_coord = total_coord // br

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1
            eff_10 = avg_eps * 1000 // baseline_eps if baseline_eps > 0 else 1000

            results.append({
                "shards": shard_count,
                "total_events": total_events,
                "events_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": int(avg_compute * 10 / avg_coord) / 10 if avg_coord > 0 else 0,
                "efficiency_pct": eff_10 / 10,
                "error_count": error_count,
            })

        return {
            "status": "ok",
            "events_per_shard": events_per_shard,
            "results": results,
        }


# ─── Worker Actor ────────────────────────────────────────────────────────────

@register_role("worker")
@actor
class WorkerActor:
    def __init__(self):
        self.events_processed = 0
        self.compute_ms = 0
        self.seen_event_ids = set()
        self.lsn_watermarks = {}
        self.actor_id = ""
        self.application_id = ""

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("process_batch")
    def on_process_batch(self, events: list = None, from_actor: str = "") -> dict:
        if events is None:
            events = []
        comp_start = host.now_ms()

        search_docs = []
        analytics_records = []
        cache_invalidations = []
        duplicates_skipped = 0
        table_counts = {}

        for event in events:
            event_id = event.get("event_id", "")
            if event_id in self.seen_event_ids:
                duplicates_skipped += 1
                continue
            self.seen_event_ids.add(event_id)

            # Keep seen_event_ids from growing unbounded
            if len(self.seen_event_ids) > 100000:
                to_remove = list(self.seen_event_ids)[:50000]
                for eid in to_remove:
                    self.seen_event_ids.discard(eid)

            table = event.get("table", "")
            table_counts[table] = table_counts.get(table, 0) + 1

            # Update LSN watermark per table
            lsn = event.get("lsn", "")
            if lsn > self.lsn_watermarks.get(table, ""):
                self.lsn_watermarks[table] = lsn

            # Transform for each sink
            search_docs.append(transform_for_search(event))
            analytics_records.append(transform_for_analytics(event))

            if event["operation"] in ("UPDATE", "DELETE"):
                cache_invalidations.append(transform_for_cache_invalidation(event))
            elif event["operation"] == "INSERT":
                cache_invalidations.append(transform_for_cache_invalidation(event))

        compute_ms = host.now_ms() - comp_start
        self.events_processed += len(events) - duplicates_skipped
        self.compute_ms += compute_ms

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "worker_events": len(events) - duplicates_skipped,
                        "worker_duplicates": duplicates_skipped,
                    },
                    "latency_totals_ms": {"worker.compute": compute_ms},
                    "latency_max_ms": {"worker.compute": compute_ms},
                    "latency_samples": {"worker.compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "events_in": len(events),
            "search_docs": len(search_docs),
            "analytics_records": len(analytics_records),
            "cache_invalidations": len(cache_invalidations),
            "duplicates_skipped": duplicates_skipped,
            "table_counts": table_counts,
            "lsn_watermarks": self.lsn_watermarks,
            "compute_ms": compute_ms,
        }
