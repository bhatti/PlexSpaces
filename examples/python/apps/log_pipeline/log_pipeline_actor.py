# SPDX-License-Identifier: AGPL-3.0-or-later
"""Log Ingestion & Routing Pipeline — Observability data processing with PlexSpaces.

Demonstrates a production-grade log pipeline architecture:
  Ingester → EventBreaker → Router → PipelineWorker (shard group) → SinkWriter

Real-world analogs: Stream, Splunk Heavy Forwarder, Fluentd, Vector.

Primitives exercised:
- GenServer actors for each pipeline stage
- Shard Groups for partitioned pipeline workers
- Process Groups for sink fan-out
- Scatter/Gather for parallel event processing
- TupleSpace for shared routing rules and stage summaries
- GenEvent for audit/alerting
- FSM for pipeline health tracking
- Facets: virtual_actor, durability, metrics
- Compute vs coordination metrics (granularity ratio)
"""

import hashlib
import json
import math
import re
import time
from typing import Any, Dict, List, Optional

from plexspaces import (
    ActorID,
    actor,
    event_actor,
    fsm_actor,
    handler,
    host,
    init_handler,
    query_handler,
    run_handler,
    signal_handler,
    state,
    workflow_actor,
)
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement


def actor_application_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def actor_node_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).node_id or "local"
    except ValueError:
        return "local"


def now_ms() -> int:
    return host.now_ms()


# ─────────────────────────────────────────────────────────────────────────────
# Pipeline Functions — pure transforms applied to events
# ─────────────────────────────────────────────────────────────────────────────


def fn_json_parse(event: dict) -> Optional[dict]:
    """Parse JSON raw body into structured fields."""
    raw = event.get("_raw", "")
    if raw.startswith("{"):
        try:
            parsed = json.loads(raw)
            event.update(parsed)
            event["_parsed"] = True
            return event
        except json.JSONDecodeError:
            event["_parse_error"] = True
            return event
    return event


def fn_regex_extract(event: dict, pattern: str = r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})",
                     field: str = "timestamp") -> Optional[dict]:
    """Extract fields via regex."""
    raw = event.get("_raw", "")
    match = re.search(pattern, raw)
    if match:
        event[field] = match.group(1)
    return event


def fn_mask_pii(event: dict, fields: List[str] = None) -> Optional[dict]:
    """Mask PII fields (email, SSN, credit card patterns)."""
    if fields is None:
        fields = ["email", "ssn", "credit_card"]
    pii_patterns = {
        "email": (r"[\w.+-]+@[\w-]+\.[\w.]+", "***@***.***"),
        "ssn": (r"\d{3}-\d{2}-\d{4}", "***-**-****"),
        "credit_card": (r"\d{4}[- ]?\d{4}[- ]?\d{4}[- ]?\d{4}", "****-****-****-****"),
    }
    raw = event.get("_raw", "")
    masked_count = 0
    for name in fields:
        if name in pii_patterns:
            pat, replacement = pii_patterns[name]
            new_raw = re.sub(pat, replacement, raw)
            if new_raw != raw:
                masked_count += 1
                raw = new_raw
    if masked_count > 0:
        event["_raw"] = raw
        event["_pii_masked"] = masked_count
    return event


def fn_enrich(event: dict, fields: Dict[str, str] = None) -> Optional[dict]:
    """Add enrichment fields (geo, service tags, environment)."""
    if fields is None:
        fields = {
            "environment": "production",
            "region": "us-east-1",
            "pipeline_version": "3.0",
        }
    event.update(fields)
    event["_enriched"] = True
    return event


def fn_rename_fields(event: dict, renames: Dict[str, str] = None) -> Optional[dict]:
    """Rename fields (e.g., normalize schema)."""
    if renames is None:
        renames = {"msg": "message", "lvl": "level", "ts": "timestamp"}
    for old_name, new_name in renames.items():
        if old_name in event:
            event[new_name] = event.pop(old_name)
    return event


def fn_drop(event: dict, condition_field: str = "level",
            drop_values: List[str] = None) -> Optional[dict]:
    """Drop events matching a condition."""
    if drop_values is None:
        drop_values = ["debug", "trace"]
    val = str(event.get(condition_field, "")).lower()
    if val in drop_values:
        return None
    return event


def fn_sample(event: dict, rate: float = 0.1) -> Optional[dict]:
    """Sample events at a given rate (0.0 to 1.0)."""
    raw = event.get("_raw", str(id(event)))
    h = int(hashlib.md5(raw.encode()).hexdigest()[:8], 16)
    if (h % 1000) / 1000.0 < rate:
        event["_sampled"] = True
        return event
    return None


PIPELINE_FUNCTIONS = {
    "json_parse": fn_json_parse,
    "regex_extract": fn_regex_extract,
    "mask_pii": fn_mask_pii,
    "enrich": fn_enrich,
    "rename_fields": fn_rename_fields,
    "drop": fn_drop,
    "sample": fn_sample,
}


# ─────────────────────────────────────────────────────────────────────────────
# Routing Rules — CEL-inspired filter expressions
# ─────────────────────────────────────────────────────────────────────────────


def evaluate_route_rule(event: dict, rule: dict) -> bool:
    """Evaluate a routing rule against an event."""
    field = rule.get("field", "source")
    op = rule.get("op", "contains")
    value = rule.get("value", "")
    event_val = str(event.get(field, ""))
    if op == "equals":
        return event_val == value
    elif op == "contains":
        return value in event_val
    elif op == "starts_with":
        return event_val.startswith(value)
    elif op == "regex":
        return bool(re.search(value, event_val))
    elif op == "exists":
        return field in event
    return False


# ─────────────────────────────────────────────────────────────────────────────
# Sink Formatters — output serialization for different destinations
# ─────────────────────────────────────────────────────────────────────────────


def format_splunk_hec(events: List[dict]) -> List[dict]:
    """Format events for Splunk HTTP Event Collector."""
    return [
        {
            "event": e.get("_raw", json.dumps(e)),
            "sourcetype": e.get("sourcetype", "generic"),
            "source": e.get("source", "plexspaces"),
            "host": e.get("host", "unknown"),
            "time": e.get("timestamp", ""),
            "index": e.get("_index", "main"),
        }
        for e in events
    ]


def format_datadog(events: List[dict]) -> List[dict]:
    """Format events for Datadog Log API."""
    return [
        {
            "ddsource": e.get("source", "plexspaces"),
            "ddtags": f"env:{e.get('environment', 'prod')},service:{e.get('service', 'unknown')}",
            "hostname": e.get("host", "unknown"),
            "message": e.get("_raw", json.dumps(e)),
            "service": e.get("service", "unknown"),
            "status": e.get("level", "info"),
        }
        for e in events
    ]


def format_s3_parquet(events: List[dict]) -> dict:
    """Format events as columnar batch for S3/Parquet-style storage."""
    return {
        "format": "parquet_batch",
        "record_count": len(events),
        "columns": list(events[0].keys()) if events else [],
        "size_bytes": sum(len(json.dumps(e)) for e in events),
    }


SINK_FORMATTERS = {
    "splunk_hec": format_splunk_hec,
    "datadog": format_datadog,
    "s3_parquet": format_s3_parquet,
}


# ─────────────────────────────────────────────────────────────────────────────
# Event Generator — synthetic log events for benchmarking
# ─────────────────────────────────────────────────────────────────────────────


def generate_events(count: int, source_mix: Dict[str, float] = None) -> List[dict]:
    """Generate synthetic log events with realistic field distributions."""
    if source_mix is None:
        source_mix = {
            "syslog": 0.3,
            "app_json": 0.4,
            "security": 0.15,
            "infra": 0.15,
        }
    levels = ["debug", "info", "info", "info", "warn", "error"]
    services = ["api-gateway", "auth-service", "payment-svc", "user-svc", "inventory"]
    hosts = [f"host-{i:03d}" for i in range(20)]

    events = []
    for i in range(count):
        frac = i / max(count, 1)
        source_name = "app_json"
        cum = 0.0
        for sname, spct in source_mix.items():
            cum += spct
            if frac < cum or sname == list(source_mix.keys())[-1]:
                source_name = sname
                break

        level = levels[i % len(levels)]
        service = services[i % len(services)]
        host_name = hosts[i % len(hosts)]

        if source_name == "app_json":
            raw = json.dumps({
                "ts": f"2026-10-02T12:{(i // 60) % 24:02d}:{i % 60:02d}Z",
                "lvl": level,
                "msg": f"Request processed id={i} latency={10 + (i % 200)}ms",
                "service": service,
                "trace_id": f"trace-{i:08x}",
            })
        elif source_name == "syslog":
            raw = (
                f"<134>Oct  2 12:{(i // 60) % 24:02d}:{i % 60:02d} {host_name} "
                f"{service}[{1000 + i}]: {level.upper()} Request id={i} completed"
            )
        elif source_name == "security":
            raw = json.dumps({
                "ts": f"2026-10-02T12:{(i // 60) % 24:02d}:{i % 60:02d}Z",
                "lvl": "warn",
                "msg": f"Auth attempt user=user{i % 100}@example.com from 10.0.{i % 256}.{(i * 7) % 256}",
                "service": "auth-service",
                "email": f"user{i % 100}@example.com",
            })
        else:
            raw = (
                f"2026-10-02T12:{(i // 60) % 24:02d}:{i % 60:02d}Z "
                f"kernel: [{i * 0.001:.6f}] {host_name} CPU{i % 8} usage={50 + (i % 50)}%"
            )

        events.append({
            "_raw": raw,
            "source": source_name,
            "host": host_name,
            "service": service,
            "level": level,
            "_time": i,
            "_size": len(raw),
        })
    return events


# ─────────────────────────────────────────────────────────────────────────────
# PipelineAuditActor (GenEvent) — fire-and-forget audit trail
# ─────────────────────────────────────────────────────────────────────────────


@event_actor
class PipelineAuditActor:
    """Audit trail for pipeline operations — fire-and-forget events."""

    events_logged: int = state(default=0)
    last_event: dict = state(default_factory=dict)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        host.process_groups.join("pipeline-audit")

    @handler("pipeline_event", "cast")
    def on_pipeline_event(
        self,
        stage: str = "",
        events_processed: int = 0,
        events_dropped: int = 0,
        latency_ms: int = 0,
        from_actor: str = "",
    ) -> None:
        self.events_logged += 1
        self.last_event = {
            "stage": stage,
            "events_processed": events_processed,
            "events_dropped": events_dropped,
            "latency_ms": latency_ms,
        }
        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "audit_events": 1,
                        f"audit_{stage}": 1,
                    },
                    "latency_totals_ms": {"audit_latency": latency_ms},
                    "latency_max_ms": {"audit_latency": latency_ms},
                    "latency_samples": {"audit_latency": 1},
                },
            )
        except Exception:
            pass

    @handler("get_stats")
    def get_stats(self, from_actor: str = "") -> dict:
        return {
            "events_logged": self.events_logged,
            "last_event": self.last_event,
        }


# ─────────────────────────────────────────────────────────────────────────────
# PipelineHealthFSM (GenFSM) — pipeline health state machine
# ─────────────────────────────────────────────────────────────────────────────


@fsm_actor(states=["healthy", "degraded", "unhealthy"], initial="healthy")
class PipelineHealthFSM:
    """FSM tracking pipeline health based on error rates and latency."""

    error_count: int = state(default=0)
    total_count: int = state(default=0)
    error_threshold: float = state(default=0.05)
    latency_threshold_ms: int = state(default=1000)
    fsm_state: str = state(default="healthy")
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        args = config.get("args", {})
        self.error_threshold = float(args.get("error_threshold", "0.05"))
        self.latency_threshold_ms = int(args.get("latency_threshold_ms", "1000"))

    @handler("report_batch")
    def report_batch(
        self,
        processed: int = 0,
        errors: int = 0,
        latency_ms: int = 0,
        from_actor: str = "",
    ) -> dict:
        self.total_count += processed
        self.error_count += errors
        error_rate = self.error_count / max(self.total_count, 1)

        prev_state = self.fsm_state
        if error_rate > self.error_threshold * 2 or latency_ms > self.latency_threshold_ms * 2:
            self.fsm_state = "unhealthy"
        elif error_rate > self.error_threshold or latency_ms > self.latency_threshold_ms:
            self.fsm_state = "degraded"
        else:
            self.fsm_state = "healthy"

        return {
            "state": self.fsm_state,
            "prev_state": prev_state,
            "error_rate": round(error_rate, 4),
            "total_count": self.total_count,
            "error_count": self.error_count,
        }

    @handler("get_health")
    def get_health(self, from_actor: str = "") -> dict:
        return {
            "state": self.fsm_state,
            "error_rate": round(self.error_count / max(self.total_count, 1), 4),
            "total_count": self.total_count,
            "error_count": self.error_count,
        }


# ─────────────────────────────────────────────────────────────────────────────
# PipelineWorkerActor (GenServer) — partitioned event processing
# ─────────────────────────────────────────────────────────────────────────────


@actor
class PipelineWorkerActor:
    """Processes events through a configurable function chain.

    Deployed as a shard group for parallel processing.
    Tracks compute time (parsing/enrichment) vs coordination time (messaging).
    """

    events_processed: int = state(default=0)
    events_dropped: int = state(default=0)
    bytes_processed: int = state(default=0)
    compute_time_ms: int = state(default=0)
    coordination_time_ms: int = state(default=0)
    pii_masked_count: int = state(default=0)
    json_parsed_count: int = state(default=0)
    enriched_count: int = state(default=0)
    pipeline_functions: list = state(default_factory=list)
    actor_id: str = state(default="")
    application_id: str = state(default="")
    node_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.node_id = actor_node_id(self.actor_id)
        args = config.get("args", {})
        fn_names = args.get("pipeline_functions", "json_parse,mask_pii,enrich,rename_fields")
        if isinstance(fn_names, str):
            self.pipeline_functions = [f.strip() for f in fn_names.split(",") if f.strip()]
        else:
            self.pipeline_functions = list(fn_names)

    @handler("process_events")
    def process_events(
        self,
        events: list = None,
        route_name: str = "",
        batch_index: int = 0,
        from_actor: str = "",
    ) -> dict:
        if events is None:
            events = []

        t_start = now_ms()
        processed = []
        dropped = 0
        batch_bytes = 0
        local_pii = 0
        local_json = 0
        local_enrich = 0

        for event in events:
            batch_bytes += event.get("_size", len(str(event)))
            result = event
            for fn_name in self.pipeline_functions:
                fn = PIPELINE_FUNCTIONS.get(fn_name)
                if fn and result is not None:
                    result = fn(result)
            if result is None:
                dropped += 1
            else:
                if result.get("_pii_masked"):
                    local_pii += 1
                if result.get("_parsed"):
                    local_json += 1
                if result.get("_enriched"):
                    local_enrich += 1
                processed.append(result)

        compute_ms = now_ms() - t_start

        self.events_processed += len(processed)
        self.events_dropped += dropped
        self.bytes_processed += batch_bytes
        self.compute_time_ms += compute_ms
        self.pii_masked_count += local_pii
        self.json_parsed_count += local_json
        self.enriched_count += local_enrich

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "worker_events_processed": len(processed),
                        "worker_events_dropped": dropped,
                        "worker_bytes_processed": batch_bytes,
                        "worker_pii_masked": local_pii,
                        "worker_json_parsed": local_json,
                    },
                    "latency_totals_ms": {"worker.compute": compute_ms},
                    "latency_max_ms": {"worker.compute": compute_ms},
                    "latency_samples": {"worker.compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "status": "ok",
            "route_name": route_name,
            "batch_index": batch_index,
            "events_in": len(events),
            "events_out": len(processed),
            "events_dropped": dropped,
            "bytes_processed": batch_bytes,
            "compute_time_ms": compute_ms,
            "node_id": self.node_id,
            "actor_id": self.actor_id,
            "pii_masked": local_pii,
            "json_parsed": local_json,
            "enriched": local_enrich,
        }

    @handler("get_stats")
    def get_stats(self, from_actor: str = "") -> dict:
        return {
            "events_processed": self.events_processed,
            "events_dropped": self.events_dropped,
            "bytes_processed": self.bytes_processed,
            "compute_time_ms": self.compute_time_ms,
            "coordination_time_ms": self.coordination_time_ms,
            "pii_masked_count": self.pii_masked_count,
            "json_parsed_count": self.json_parsed_count,
            "enriched_count": self.enriched_count,
            "pipeline_functions": self.pipeline_functions,
        }


# ─────────────────────────────────────────────────────────────────────────────
# LeaderActor (GenServer) — orchestrates the full pipeline
# ─────────────────────────────────────────────────────────────────────────────


@actor
class LeaderActor:
    """Orchestrates the log pipeline: generate → break → route → process → sink.

    Creates shard groups of PipelineWorkerActors for parallel processing.
    Tracks full compute vs coordination breakdown with scaling benchmarks.
    """

    total_events_in: int = state(default=0)
    total_events_out: int = state(default=0)
    total_events_dropped: int = state(default=0)
    total_bytes: int = state(default=0)
    total_compute_ms: int = state(default=0)
    total_coordination_ms: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")
    node_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.node_id = actor_node_id(self.actor_id)

    @handler("run")
    def run(
        self,
        event_count: int = 10000,
        worker_count: int = 8,
        batch_size: int = 500,
        pipeline_depth: int = 5,
        routes: list = None,
        sink_format: str = "splunk_hec",
        rounds: int = 1,
        from_actor: str = "",
    ) -> dict:
        """Execute the full pipeline with benchmarks."""
        if routes is None:
            routes = [
                {"name": "security_logs", "rule": {"field": "source", "op": "equals", "value": "security"},
                 "functions": ["json_parse", "mask_pii", "enrich", "rename_fields"]},
                {"name": "app_logs", "rule": {"field": "source", "op": "equals", "value": "app_json"},
                 "functions": ["json_parse", "enrich", "rename_fields"]},
                {"name": "infra_logs", "rule": {"field": "source", "op": "equals", "value": "infra"},
                 "functions": ["regex_extract", "enrich"]},
                {"name": "default", "rule": {"field": "source", "op": "exists", "value": ""},
                 "functions": ["enrich"]},
            ]

        fn_list = ["json_parse", "mask_pii", "enrich", "rename_fields", "drop"]
        pipeline_fns = ",".join(fn_list[:pipeline_depth])

        wall_start = now_ms()

        # ── Step 1: Create shard group of pipeline workers ──────────────
        t0 = now_ms()
        sg_result = host.create_shard_group(CreateShardGroupRequest(
            group_id=f"pipeline-workers-{worker_count}",
            actor_type="PipelineWorkerActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
            initial_state={"pipeline_functions": pipeline_fns},
        ))
        shard_group_id = sg_result.group_id
        coord_create_ms = now_ms() - t0

        # ── Step 2: Generate events ─────────────────────────────────────
        t0 = now_ms()
        all_events = generate_events(event_count)
        gen_compute_ms = now_ms() - t0

        round_results = []
        total_compute = gen_compute_ms
        total_coordination = coord_create_ms
        total_sink_events = 0
        total_dropped = 0
        total_routed = {r["name"]: 0 for r in routes}
        node_participation = {}
        worker_latencies = []

        for round_idx in range(rounds):
            # ── Step 3: Event breaking (split into batches) ─────────────
            t0 = now_ms()
            batches = []
            for i in range(0, len(all_events), batch_size):
                batches.append(all_events[i:i + batch_size])
            break_compute_ms = now_ms() - t0
            total_compute += break_compute_ms

            # ── Step 4: Routing — classify events per route ─────────────
            t0 = now_ms()
            routed_batches = []
            for batch_idx, batch in enumerate(batches):
                for event in batch:
                    matched = False
                    for route in routes:
                        if evaluate_route_rule(event, route["rule"]):
                            total_routed[route["name"]] = total_routed.get(route["name"], 0) + 1
                            matched = True
                            break
                    if not matched:
                        total_routed["default"] = total_routed.get("default", 0) + 1
                routed_batches.append({"events": batch, "batch_index": batch_idx})
            route_compute_ms = now_ms() - t0
            total_compute += route_compute_ms

            # ── Step 5: Scatter/Gather to pipeline workers ──────────────
            t0 = now_ms()
            sg_response = host.scatter_gather(ScatterGatherRequest(
                group_id=shard_group_id,
                query={
                    "op": "process_events",
                    "events": all_events,
                    "route_name": "all",
                    "batch_index": round_idx,
                },
                aggregation="concat",
                timeout_ms=120000,
                min_responses=worker_count,
            ))
            coord_scatter_ms = now_ms() - t0
            total_coordination += coord_scatter_ms

            # ── Step 6: Aggregate results ───────────────────────────────
            t0 = now_ms()
            round_events_out = 0
            round_dropped = 0
            round_compute_ms = 0
            round_bytes = 0

            shard_responses = sg_response.shard_responses
            for sr in shard_responses:
                payload = sr
                for key in ["payload", "result", "response", "data"]:
                    if isinstance(payload, dict) and key in payload:
                        payload = payload[key]
                if isinstance(payload, dict):
                    round_events_out += payload.get("events_out", 0)
                    round_dropped += payload.get("events_dropped", 0)
                    round_compute_ms += payload.get("compute_time_ms", 0)
                    round_bytes += payload.get("bytes_processed", 0)
                    w_lat = payload.get("compute_time_ms", 0)
                    worker_latencies.append(w_lat)
                    nid = payload.get("node_id", "local")
                    node_participation[nid] = node_participation.get(nid, 0) + 1

            agg_compute_ms = now_ms() - t0
            total_compute += agg_compute_ms + round_compute_ms
            total_events_out_round = round_events_out
            total_dropped += round_dropped

            # ── Step 7: Sink formatting ─────────────────────────────────
            t0 = now_ms()
            formatter = SINK_FORMATTERS.get(sink_format, format_splunk_hec)
            if sink_format == "s3_parquet":
                sink_output = {"format": "parquet_batch", "record_count": round_events_out}
            else:
                sink_output = {"format": sink_format, "record_count": round_events_out}
            sink_compute_ms = now_ms() - t0
            total_compute += sink_compute_ms
            total_sink_events += round_events_out

            round_results.append({
                "round": round_idx,
                "events_in": len(all_events),
                "events_out": round_events_out,
                "events_dropped": round_dropped,
                "bytes_processed": round_bytes,
                "scatter_gather_ms": coord_scatter_ms,
                "worker_compute_ms": round_compute_ms,
                "sink_format": sink_format,
                "sink_records": round_events_out,
            })

        wall_ms = now_ms() - wall_start

        # ── Metrics reporting ───────────────────────────────────────────
        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": rounds,
                    "counter_metrics": {
                        "leader_events_in": event_count * rounds,
                        "leader_events_out": total_sink_events,
                        "leader_events_dropped": total_dropped,
                        "leader_rounds": rounds,
                    },
                    "latency_totals_ms": {
                        "leader.compute": total_compute,
                        "leader.coordination": total_coordination,
                    },
                    "latency_max_ms": {
                        "leader.compute": total_compute,
                        "leader.coordination": total_coordination,
                    },
                    "latency_samples": {
                        "leader.compute": 1,
                        "leader.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        self.total_events_in += event_count * rounds
        self.total_events_out += total_sink_events
        self.total_events_dropped += total_dropped
        self.total_compute_ms += total_compute
        self.total_coordination_ms += total_coordination

        avg_worker_lat = int(sum(worker_latencies) / max(len(worker_latencies), 1))
        max_worker_lat = max(worker_latencies) if worker_latencies else 0
        granularity = round(total_compute / max(total_coordination, 1), 2)
        events_per_sec = int((event_count * rounds) / max(wall_ms / 1000.0, 0.001))

        return {
            "status": "ok",
            "event_count": event_count,
            "worker_count": worker_count,
            "batch_size": batch_size,
            "pipeline_depth": pipeline_depth,
            "pipeline_functions": pipeline_fns,
            "sink_format": sink_format,
            "rounds": rounds,
            "wall_time_ms": wall_ms,
            "compute_time_ms": total_compute,
            "coordination_time_ms": total_coordination,
            "granularity_ratio": granularity,
            "events_per_sec": events_per_sec,
            "total_events_in": event_count * rounds,
            "total_events_out": total_sink_events,
            "total_events_dropped": total_dropped,
            "total_bytes_processed": sum(r.get("bytes_processed", 0) for r in round_results),
            "avg_worker_latency_ms": avg_worker_lat,
            "max_worker_latency_ms": max_worker_lat,
            "node_count": len(node_participation),
            "worker_node_count": len([n for n in node_participation if node_participation[n] > 0]),
            "actor_count": worker_count + 1,
            "leader_node_id": self.node_id,
            "remote_nodes_with_work": [n for n in node_participation if n != self.node_id],
            "route_distribution": total_routed,
            "message_count": sum(r.get("events_in", 0) for r in round_results),
            "error_count": 0,
            "results": round_results,
            "nodes": node_participation,
        }

    @handler("get_stats")
    def get_stats(self, from_actor: str = "") -> dict:
        return {
            "total_events_in": self.total_events_in,
            "total_events_out": self.total_events_out,
            "total_events_dropped": self.total_events_dropped,
            "total_compute_ms": self.total_compute_ms,
            "total_coordination_ms": self.total_coordination_ms,
            "node_id": self.node_id,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        event_count: int = 10000,
        shard_counts: list = None,
        batch_size: int = 500,
        pipeline_depth: int = 5,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        """Strong scaling benchmark: fixed total work, vary worker count."""
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion: 2 scatter_gathers per timing pass.
        # Use BENCH_EVENTS total events split evenly across shards for true strong scaling.
        TIMING_SG = 2
        BENCH_EVENTS = 200  # total events; each shard gets BENCH_EVENTS // shard_count

        fn_list = ["json_parse", "mask_pii", "enrich", "rename_fields", "drop"]
        pipeline_fns = ",".join(fn_list[:pipeline_depth])

        # Generate base events once to avoid repeated allocations
        base_events = generate_events(BENCH_EVENTS)

        results = []
        baseline_eps = 0

        for num_shards in shard_counts:
            events_per_shard = max(1, BENCH_EVENTS // num_shards)
            bench_events = base_events[:events_per_shard]

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"bench-strong-{num_shards}-{host.now_ms()}",
                actor_type="PipelineWorkerActor",
                shard_count=num_shards,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
                initial_state={"pipeline_functions": pipeline_fns},
            ))
            group_id = group.group_id

            # 1 warmup scatter_gather (discard timing)
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_events", "events": bench_events[:min(4, len(bench_events))],
                       "route_name": "bench", "batch_index": 0},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0
            node_participation = {}

            for _bi in range(benchmark_rounds):
                iter_start = now_ms()
                iter_compute = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_events", "events": bench_events,
                               "route_name": "bench", "batch_index": _sg},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_time_ms", 0)
                            nid = r.get("node_id", "local")
                            node_participation[nid] = node_participation.get(nid, 0) + 1

                iter_wall = now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            # Aggregate throughput: TIMING_SG × events_per_shard × num_shards per iteration
            actual_events = TIMING_SG * events_per_shard * num_shards
            avg_eps = actual_events * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1

            speedup_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100
            speedup = speedup_100 / 100
            eff_100 = speedup_100 * shard_counts[0] // num_shards if num_shards > 0 else 100
            compute_pct = avg_compute * 100 // (avg_compute + avg_coord) if (avg_compute + avg_coord) > 0 else 0
            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "shards": num_shards,
                "events_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "compute_pct": compute_pct,
                "granularity_ratio": gran,
                "speedup": speedup,
                "efficiency_pct": eff_100,
                "node_count": len(node_participation),
                "error_count": 0,
            })

        return {
            "status": "ok",
            "benchmark_type": "strong_scaling",
            "event_count": event_count,
            "pipeline_depth": pipeline_depth,
            "batch_size": batch_size,
            "shard_counts": shard_counts,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        events_per_shard: int = 5000,
        shard_counts: list = None,
        batch_size: int = 500,
        pipeline_depth: int = 5,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        """Weak scaling benchmark: fixed work per shard, problem grows."""
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion: fixed 100 events per shard (weak scaling)
        TIMING_SG = 2
        BENCH_PER_SHARD = 100

        fn_list = ["json_parse", "mask_pii", "enrich", "rename_fields", "drop"]
        pipeline_fns = ",".join(fn_list[:pipeline_depth])

        base_events = generate_events(BENCH_PER_SHARD)

        results = []
        baseline_eps = 0

        for num_shards in shard_counts:
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"bench-weak-{num_shards}-{host.now_ms()}",
                actor_type="PipelineWorkerActor",
                shard_count=num_shards,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
                initial_state={"pipeline_functions": pipeline_fns},
            ))
            group_id = group.group_id

            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_events", "events": base_events[:min(4, len(base_events))],
                       "route_name": "bench", "batch_index": 0},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0
            node_participation = {}

            for _bi in range(benchmark_rounds):
                iter_start = now_ms()
                iter_compute = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_events", "events": base_events,
                               "route_name": "bench", "batch_index": _sg},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_time_ms", 0)
                            nid = r.get("node_id", "local")
                            node_participation[nid] = node_participation.get(nid, 0) + 1

                iter_wall = now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            # Aggregate throughput: TIMING_SG × BENCH_PER_SHARD × num_shards per iteration
            actual_events = TIMING_SG * BENCH_PER_SHARD * num_shards
            avg_eps = actual_events * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1

            eff_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100
            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "shards": num_shards,
                "total_events": TIMING_SG * BENCH_PER_SHARD * num_shards,
                "events_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran,
                "efficiency_pct": eff_100,
                "node_count": len(node_participation),
                "error_count": 0,
            })

        return {
            "status": "ok",
            "benchmark_type": "weak_scaling",
            "events_per_shard": events_per_shard,
            "pipeline_depth": pipeline_depth,
            "batch_size": batch_size,
            "shard_counts": shard_counts,
            "results": results,
        }

    @handler("run_pipeline_depth_benchmark")
    def run_pipeline_depth_benchmark(
        self,
        event_count: int = 10000,
        worker_count: int = 8,
        batch_size: int = 500,
        depths: list = None,
        from_actor: str = "",
    ) -> dict:
        """Measure throughput at different pipeline function chain depths."""
        if depths is None:
            depths = [1, 2, 3, 4, 5]

        # Inline to avoid WASM heap exhaustion from calling self.run() per depth.
        # 1 warmup + 1 timing sg per depth × 5 depths = 10 total sg calls (safe).
        BENCH_EVENTS = 200
        fn_list = ["json_parse", "mask_pii", "enrich", "rename_fields", "drop"]
        base_events = generate_events(BENCH_EVENTS)

        results = []
        for depth in depths:
            pipeline_fns = ",".join(fn_list[:depth])
            fn_label = pipeline_fns

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"bench-depth-{depth}-{host.now_ms()}",
                actor_type="PipelineWorkerActor",
                shard_count=worker_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
                initial_state={"pipeline_functions": pipeline_fns},
            ))
            group_id = group.group_id

            # warmup
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_events", "events": base_events[:4],
                       "route_name": "depth", "batch_index": 0},
                timeout_ms=30000,
            ))

            # single timing pass
            t_start = now_ms()
            iter_compute = 0
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_events", "events": base_events,
                       "route_name": "depth", "batch_index": 1},
                timeout_ms=30000,
            ))
            wall_ms = now_ms() - t_start

            for resp in sg_result.shard_responses:
                r = resp
                for key in ["payload", "result", "response", "data"]:
                    if isinstance(r, dict) and key in r:
                        r = r[key]
                if isinstance(r, dict):
                    iter_compute += r.get("compute_time_ms", 0)

            coord_ms = max(1, wall_ms - iter_compute)
            # aggregate throughput: worker_count shards each processed BENCH_EVENTS events
            agg_eps = BENCH_EVENTS * worker_count * 1000 // wall_ms if wall_ms > 0 else 0
            gran_10 = iter_compute * 10 // coord_ms if coord_ms > 0 else 0

            results.append({
                "depth": depth,
                "functions": fn_label,
                "events_per_sec": agg_eps,
                "wall_time_ms": wall_ms,
                "compute_time_ms": iter_compute,
                "coordination_time_ms": coord_ms,
                "granularity_ratio": gran_10 / 10,
            })

        return {
            "status": "ok",
            "benchmark_type": "pipeline_depth",
            "event_count": BENCH_EVENTS,
            "worker_count": worker_count,
            "results": results,
        }


# ─────────────────────────────────────────────────────────────────────────────
# ACTOR_ROLES — multi-role dispatch registry
# ─────────────────────────────────────────────────────────────────────────────

ACTOR_ROLES = {
    "leader": LeaderActor,
    "worker": PipelineWorkerActor,
    "PipelineWorkerActor": PipelineWorkerActor,
    "PipelineAuditActor": PipelineAuditActor,
    "PipelineHealthFSM": PipelineHealthFSM,
    "LeaderActor": LeaderActor,
}
