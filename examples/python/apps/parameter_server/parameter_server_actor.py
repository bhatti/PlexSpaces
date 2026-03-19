"""Python Parameter Server - Distributed ML Training."""

import math
from typing import Any, Dict, List, Mapping

from plexspaces import actor, handler, host, init_handler, state


def worker_seed(actor_id: str) -> int:
    """Derive a stable synthetic seed from the runtime actor ID."""
    return sum(ord(ch) for ch in actor_id) % 10000


def actor_node_id(actor_id: str) -> str:
    """Extract the runtime node id from a canonical or child-style actor id."""
    if "@" in actor_id:
        return actor_id.rsplit("@", 1)[1]
    return "local"


def actor_application_id(actor_id: str) -> str:
    """Extract the application namespace from a canonical or child-style actor id."""
    if "//" in actor_id and "::" in actor_id:
        suffix = actor_id.split("//", 1)[1]
        qualified = suffix.split("@", 1)[0]
        return qualified.rsplit("::", 1)[1]
    if ":" in actor_id and "@" in actor_id:
        return actor_id.split(":", 1)[1].split("@", 1)[0]
    return ""


def status_metrics_value(status: Mapping[str, Any], field: str) -> Any:
    return status.get("application", {}).get("metrics", {}).get(field)


def status_metrics_map(status: Mapping[str, Any], field: str) -> Dict[str, int]:
    value = status_metrics_value(status, field)
    if not isinstance(value, Mapping):
        return {}
    out: Dict[str, int] = {}
    for key, raw in value.items():
        if isinstance(raw, (int, float)):
            out[str(key)] = int(raw)
    return out


def saturating_map_delta(end: Dict[str, int], start: Dict[str, int]) -> Dict[str, int]:
    keys = set(end) | set(start)
    return {key: max(0, end.get(key, 0) - start.get(key, 0)) for key in keys}


def normalize_worker_payload(payload: Any) -> Dict[str, Any]:
    current = payload
    while isinstance(current, Mapping):
        if any(
            key in current
            for key in (
                "status",
                "error",
                "latency_ms",
                "samples_processed",
                "gradient_checksum",
                "gradient_scale",
            )
        ):
            return dict(current)
        for key in ("payload", "result", "response", "data"):
            nested = current.get(key)
            if isinstance(nested, Mapping):
                current = nested
                break
        else:
            return dict(current)
    return {}


def average_latency(metric: Mapping[str, int]) -> float:
    responses = int(metric.get("responses", 0))
    total_latency_ms = int(metric.get("total_latency_ms", 0))
    if responses <= 0:
        return 0.0
    return total_latency_ms / responses


def compute_actor_counts(leader_node_id: str, shard_actor_ids: List[str]) -> Dict[str, Dict[str, int]]:
    nodes: Dict[str, Dict[str, int]] = {
        leader_node_id: {
            "actors": 1,
            "leader_actors": 1,
            "worker_actors": 0,
        }
    }
    for shard_actor_id in shard_actor_ids:
        node_id = actor_node_id(shard_actor_id)
        node = nodes.setdefault(
            node_id,
            {"actors": 0, "leader_actors": 0, "worker_actors": 0},
        )
        node["actors"] += 1
        node["worker_actors"] += 1
    return nodes


def apply_status_delta(
    node_metrics: Dict[str, Dict[str, int]],
    role_metrics: Dict[str, Dict[str, int]],
    start_status: Mapping[str, Any],
    end_status: Mapping[str, Any],
) -> None:
    counter_delta = saturating_map_delta(
        status_metrics_map(end_status, "counter_metrics"),
        status_metrics_map(start_status, "counter_metrics"),
    )
    latency_totals_delta = saturating_map_delta(
        status_metrics_map(end_status, "latency_totals_ms"),
        status_metrics_map(start_status, "latency_totals_ms"),
    )
    latency_max_end = status_metrics_map(end_status, "latency_max_ms")
    latency_max_start = status_metrics_map(start_status, "latency_max_ms")
    latency_samples_delta = saturating_map_delta(
        status_metrics_map(end_status, "latency_samples"),
        status_metrics_map(start_status, "latency_samples"),
    )
    message_delta = max(
        0,
        int(status_metrics_value(end_status, "message_count") or 0)
        - int(status_metrics_value(start_status, "message_count") or 0),
    )
    error_delta = max(
        0,
        int(status_metrics_value(end_status, "error_count") or 0)
        - int(status_metrics_value(start_status, "error_count") or 0),
    )

    node_id = str(end_status.get("node_id") or "unknown")
    node = node_metrics.setdefault(
        node_id,
        {
            "actors": 0,
            "leader_actors": 0,
            "worker_actors": 0,
            "messages": 0,
            "leader_messages": 0,
            "worker_messages": 0,
            "gradient_operations": 0,
            "samples_processed": 0,
            "compute_time_ms": 0,
            "coordination_time_ms": 0,
            "total_latency_ms": 0,
            "max_latency_ms": 0,
            "responses": 0,
            "errors": 0,
        },
    )
    node["messages"] += message_delta
    node["leader_messages"] += counter_delta.get("leader_messages", 0)
    node["worker_messages"] += counter_delta.get("worker_messages", 0)
    node["gradient_operations"] += counter_delta.get("gradient_operation_count", 0)
    node["samples_processed"] += counter_delta.get("samples_processed", 0)
    node["compute_time_ms"] += (
        latency_totals_delta.get("worker.compute", 0)
        + latency_totals_delta.get("leader.compute", 0)
    )
    node["coordination_time_ms"] += (
        latency_totals_delta.get("worker.coordination", 0)
        + latency_totals_delta.get("leader.coordination", 0)
    )
    node["total_latency_ms"] += (
        latency_totals_delta.get("worker", 0) + latency_totals_delta.get("leader", 0)
    )
    node["max_latency_ms"] = max(
        node["max_latency_ms"],
        latency_max_end.get("worker", latency_max_start.get("worker", 0)),
        latency_max_end.get("leader", latency_max_start.get("leader", 0)),
    )
    node["responses"] += latency_samples_delta.get("worker", 0)
    node["errors"] += error_delta

    role_deltas = {
        "leader": {
            "messages": counter_delta.get("leader_messages", 0),
            "gradient_operations": 0,
            "samples_processed": 0,
            "compute_time_ms": latency_totals_delta.get("leader.compute", 0),
            "coordination_time_ms": latency_totals_delta.get("leader.coordination", 0),
            "total_latency_ms": latency_totals_delta.get("leader", 0),
            "max_latency_ms": latency_max_end.get("leader", latency_max_start.get("leader", 0)),
            "responses": latency_samples_delta.get("leader", 0),
            "errors": 0,
        },
        "worker": {
            "messages": counter_delta.get("worker_messages", 0),
            "gradient_operations": counter_delta.get("gradient_operation_count", 0),
            "samples_processed": counter_delta.get("samples_processed", 0),
            "compute_time_ms": latency_totals_delta.get("worker.compute", 0),
            "coordination_time_ms": latency_totals_delta.get("worker.coordination", 0),
            "total_latency_ms": latency_totals_delta.get("worker", 0),
            "max_latency_ms": latency_max_end.get("worker", latency_max_start.get("worker", 0)),
            "responses": latency_samples_delta.get("worker", 0),
            "errors": error_delta,
        },
    }
    for role, delta in role_deltas.items():
        role_entry = role_metrics.setdefault(
            role,
            {
                "actors": 0,
                "messages": 0,
                "gradient_operations": 0,
                "samples_processed": 0,
                "compute_time_ms": 0,
                "coordination_time_ms": 0,
                "total_latency_ms": 0,
                "max_latency_ms": 0,
                "responses": 0,
                "errors": 0,
            },
        )
        for key, value in delta.items():
            if key == "max_latency_ms":
                role_entry[key] = max(role_entry[key], value)
            else:
                role_entry[key] += value


@actor
class Leader:
    input_dim: int = state(default=100)
    hidden_dim: int = state(default=64)
    learning_rate: float = state(default=0.01)
    iteration: int = state(default=0)
    num_workers: int = state(default=4)
    batch_size: int = state(default=256)
    w1: List[List[float]] = state(default_factory=list)
    w2: List[float] = state(default_factory=list)
    total_coord_ms: float = state(default=0.0)
    total_compute_ms: float = state(default=0.0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict):
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        args = config.get("args", {})
        self.learning_rate = float(args.get("learning_rate", 0.01))
        self.input_dim = int(args.get("input_dim", 100))
        self.hidden_dim = int(args.get("hidden_dim", 64))
        self.num_workers = int(args.get("num_workers", 4))
        self.batch_size = int(args.get("batch_size", 256))
        self.iteration = 0
        self.total_coord_ms = 0.0
        self.total_compute_ms = 0.0

        self.w1 = []
        for i in range(self.hidden_dim):
            row = []
            for j in range(self.input_dim):
                row.append(
                    ((i * 7 + j * 13 + 42) % 1000 - 500)
                    / (500.0 * math.sqrt(self.input_dim))
                )
            self.w1.append(row)
        self.w2 = [
            ((i * 11 + 7) % 1000 - 500) / (500.0 * math.sqrt(self.hidden_dim))
            for i in range(self.hidden_dim)
        ]

    @handler("train")
    def train(self, iterations: int = 10, from_actor: str = "") -> dict:
        if not isinstance(iterations, int):
            iterations = int(iterations)
        group_id = f"python-parameter-server-{host.now_ms()}"
        group = host.create_shard_group(
            {
                "group_id": group_id,
                "actor_type": "worker",
                "shard_count": self.num_workers,
                "partition_strategy": "hash",
                "rebalance_policy": "manual",
                "placement": {"strategy": "from_registry"},
                "initial_state": {},
            }
        )
        shard_actor_ids = group.get("shard_actor_ids", [])
        if not shard_actor_ids:
            return {"status": "error", "error": "failed to create worker shard group"}

        leader_node_id = actor_node_id(self.actor_id or host.self_id())
        participant_node_ids = sorted(
            {leader_node_id, *(actor_node_id(actor_id) for actor_id in shard_actor_ids)}
        )
        start_statuses: Dict[str, Dict[str, Any]] = {}
        node_addresses: Dict[str, str] = {}
        for node_id in participant_node_ids:
            try:
                status = host.application_get_status(self.application_id, node_id)
            except Exception as exc:
                return {
                    "status": "error",
                    "error": f"failed to capture application status for {node_id}: {exc}",
                }
            start_statuses[node_id] = status
            node_address = status.get("node_address")
            if isinstance(node_address, str) and node_address:
                node_addresses[node_id] = node_address

        results = []
        total_samples = 0
        total_errors = 0
        total_worker_latency_ms = 0
        total_worker_responses = 0
        max_worker_latency_ms = 0
        remote_nodes_with_work = set()

        for _ in range(iterations):
            coord_start = host.now_ms()
            response = host.scatter_gather(
                {
                    "group_id": group_id,
                    "query": {
                        "op": "compute_gradient",
                        "weights": {"w1": self.w1, "w2": self.w2},
                        "input_dim": self.input_dim,
                        "hidden_dim": self.hidden_dim,
                    },
                    "aggregation": "concat",
                    "min_responses": self.num_workers,
                    "timeout_ms": 30000,
                }
            )
            gradients = []
            iteration_errors = 0
            iteration_responses = 0
            iteration_latency_ms = 0
            iteration_max_latency_ms = 0
            iteration_samples = 0
            for shard in response.get("shard_responses", []):
                payload = normalize_worker_payload(shard.get("payload", {}))
                if payload.get("status") == "ok":
                    gradients.append(payload.get("gradients", {}))
                    samples = int(payload.get("samples_processed", payload.get("samples", 0)))
                    latency_ms = int(payload.get("latency_ms", 0))
                    total_samples += samples
                    iteration_samples += samples
                    iteration_responses += 1
                    total_worker_responses += 1
                    iteration_latency_ms += latency_ms
                    total_worker_latency_ms += latency_ms
                    iteration_max_latency_ms = max(iteration_max_latency_ms, latency_ms)
                    max_worker_latency_ms = max(max_worker_latency_ms, latency_ms)
                    actor_id = str(payload.get("actor_id", ""))
                    node_id = str(payload.get("node_id") or actor_node_id(actor_id))
                    if node_id and node_id != leader_node_id:
                        remote_nodes_with_work.add(node_id)
                else:
                    iteration_errors += 1
                    total_errors += 1

            coord_ms = host.now_ms() - coord_start

            if not gradients:
                return {"status": "error", "error": "no gradients returned"}

            compute_start = host.now_ms()
            n_workers = len(gradients)
            agg_w1 = [[0.0] * self.input_dim for _ in range(self.hidden_dim)]
            agg_w2 = [0.0] * self.hidden_dim
            for grad in gradients:
                d_w1 = grad.get("d_w1", [])
                d_w2 = grad.get("d_w2", [])
                for i in range(min(self.hidden_dim, len(d_w1))):
                    row = d_w1[i]
                    for j in range(min(self.input_dim, len(row))):
                        agg_w1[i][j] += row[j]
                    if i < len(d_w2):
                        agg_w2[i] += d_w2[i]

            for i in range(self.hidden_dim):
                for j in range(self.input_dim):
                    agg_w1[i][j] /= n_workers
                    self.w1[i][j] -= self.learning_rate * agg_w1[i][j]
                agg_w2[i] /= n_workers
                self.w2[i] -= self.learning_rate * agg_w2[i]

            compute_ms = host.now_ms() - compute_start
            self.total_coord_ms += coord_ms
            self.total_compute_ms += compute_ms
            self.iteration += 1

            results.append(
                {
                    "iteration": self.iteration,
                    "workers": n_workers,
                    "coord_ms": coord_ms,
                    "compute_ms": compute_ms,
                    "samples_processed": iteration_samples,
                    "responses": iteration_responses,
                    "errors": iteration_errors,
                    "avg_latency_ms": (
                        iteration_latency_ms / iteration_responses
                        if iteration_responses
                        else 0.0
                    ),
                    "max_latency_ms": iteration_max_latency_ms,
                }
            )

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "leader_messages": iterations + 1,
                        "leader_runs": 1,
                        "weight_update_count": iterations,
                        "training_rounds": iterations,
                    },
                    "latency_totals_ms": {
                        "leader": int(self.total_compute_ms + self.total_coord_ms),
                        "leader.compute": int(self.total_compute_ms),
                        "leader.coordination": int(self.total_coord_ms),
                    },
                    "latency_max_ms": {
                        "leader": int(self.total_compute_ms + self.total_coord_ms),
                        "leader.compute": int(self.total_compute_ms),
                        "leader.coordination": int(self.total_coord_ms),
                    },
                    "latency_samples": {
                        "leader": 1,
                        "leader.compute": 1,
                        "leader.coordination": 1,
                    },
                },
            )
        except Exception as exc:
            return {"status": "error", "error": f"leader metrics update failed: {exc}"}

        node_metrics: Dict[str, Dict[str, int]] = {}
        role_metrics: Dict[str, Dict[str, int]] = {}
        for node_id in participant_node_ids:
            try:
                status = host.application_get_status(self.application_id, node_id)
            except Exception as exc:
                return {
                    "status": "error",
                    "error": f"failed to collect final application status for {node_id}: {exc}",
                }
            node_address = status.get("node_address")
            if isinstance(node_address, str) and node_address:
                node_addresses[node_id] = node_address
            apply_status_delta(node_metrics, role_metrics, start_statuses[node_id], status)

        topology_counts = compute_actor_counts(leader_node_id, shard_actor_ids)
        for node_id, counts in topology_counts.items():
            node = node_metrics.setdefault(
                node_id,
                {
                    "actors": 0,
                    "leader_actors": 0,
                    "worker_actors": 0,
                    "messages": 0,
                    "leader_messages": 0,
                    "worker_messages": 0,
                    "gradient_operations": 0,
                    "samples_processed": 0,
                    "compute_time_ms": 0,
                    "coordination_time_ms": 0,
                    "total_latency_ms": 0,
                    "max_latency_ms": 0,
                    "responses": 0,
                    "errors": 0,
                },
            )
            node["actors"] += counts["actors"]
            node["leader_actors"] += counts["leader_actors"]
            node["worker_actors"] += counts["worker_actors"]

        for role in ("leader", "worker"):
            role_metrics.setdefault(
                role,
                {
                    "actors": 0,
                    "messages": 0,
                    "gradient_operations": 0,
                    "samples_processed": 0,
                    "compute_time_ms": 0,
                    "coordination_time_ms": 0,
                    "total_latency_ms": 0,
                    "max_latency_ms": 0,
                    "responses": 0,
                    "errors": 0,
                },
            )
        role_metrics["leader"]["actors"] = 1
        role_metrics["worker"]["actors"] = len(shard_actor_ids)

        total_messages = sum(node["messages"] for node in node_metrics.values())
        total_gradient_operations = sum(
            node["gradient_operations"] for node in node_metrics.values()
        )
        total_compute_ms = sum(node["compute_time_ms"] for node in node_metrics.values())
        total_coordination_ms = sum(
            node["coordination_time_ms"] for node in node_metrics.values()
        )
        worker_node_count = sum(
            1
            for node_id, node in node_metrics.items()
            if node_id != leader_node_id and node["responses"] > 0
        )
        actor_counts = [node["actors"] for node in node_metrics.values()]
        actor_distribution_skew = (max(actor_counts) - min(actor_counts)) if actor_counts else 0

        return {
            "status": "ok",
            "iterations_completed": self.iteration,
            "worker_count": self.num_workers,
            "samples_processed": total_samples,
            "results": results,
            "param_count": self.input_dim * self.hidden_dim + self.hidden_dim,
            "iterations": iterations,
            "batch_size": self.batch_size,
            "training_rounds": iterations,
            "leader_node_id": leader_node_id,
            "node_addresses": node_addresses,
            "shard_actor_ids": shard_actor_ids,
            "node_count": len(node_metrics),
            "worker_node_count": worker_node_count,
            "actor_count": len(shard_actor_ids) + 1,
            "message_count": total_messages,
            "gradient_operation_count": total_gradient_operations,
            "weight_update_count": iterations,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coordination_ms,
            "total_time_ms": total_compute_ms + total_coordination_ms,
            "granularity_ratio": (
                total_compute_ms / total_coordination_ms if total_coordination_ms else 0.0
            ),
            "avg_worker_latency_ms": (
                total_worker_latency_ms / total_worker_responses
                if total_worker_responses
                else 0.0
            ),
            "max_worker_latency_ms": max_worker_latency_ms,
            "error_count": total_errors,
            "remote_nodes_with_work": sorted(remote_nodes_with_work),
            "actor_distribution_skew": actor_distribution_skew,
            "nodes": {
                node_id: {
                    **metrics,
                    "avg_latency_ms": average_latency(metrics),
                }
                for node_id, metrics in node_metrics.items()
            },
            "roles": {
                role: {
                    **metrics,
                    "avg_latency_ms": average_latency(metrics),
                }
                for role, metrics in role_metrics.items()
            },
        }


@actor
class Worker:
    worker_id: str = state(default="")
    application_id: str = state(default="")
    shard_size: int = state(default=2000)
    batch_size: int = state(default=256)

    @init_handler
    def on_init(self, config: dict):
        self.worker_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.worker_id)
        args = config.get("args", {})
        self.batch_size = int(args.get("batch_size", self.batch_size))

    @handler("compute_gradient")
    def compute_gradient(
        self,
        weights: Dict[str, Any] = None,
        input_dim: int = 100,
        hidden_dim: int = 64,
        from_actor: str = "",
    ) -> dict:
        start_ms = host.now_ms()
        if not weights:
            return {"status": "error", "error": "missing weights"}

        w1 = weights.get("w1", [])
        w2 = weights.get("w2", [])
        if not w1 or not w2:
            return {"status": "error", "error": "invalid weights"}

        d_w1 = [[0.0] * input_dim for _ in range(hidden_dim)]
        d_w2 = [0.0] * hidden_dim

        seed = worker_seed(self.worker_id)
        for sample_idx in range(self.batch_size):
            for i in range(hidden_dim):
                scale = ((seed + sample_idx + i * 17) % 1000) / 1000.0 - 0.5
                d_w2[i] += scale
                for j in range(input_dim):
                    d_w1[i][j] += scale * (((seed + j * 13) % 1000) / 1000.0 - 0.5)

        for i in range(hidden_dim):
            for j in range(input_dim):
                d_w1[i][j] /= self.batch_size
            d_w2[i] /= self.batch_size

        latency_ms = host.now_ms() - start_ms
        gradient_checksum = 0
        gradient_scale = 0
        for row in d_w1:
            for value in row:
                scaled = int(round(value * 1_000_000))
                gradient_checksum = (gradient_checksum + scaled) & 0xFFFFFFFFFFFFFFFF
                gradient_scale += abs(scaled)
        for value in d_w2:
            scaled = int(round(value * 1_000_000))
            gradient_checksum = (gradient_checksum + scaled) & 0xFFFFFFFFFFFFFFFF
            gradient_scale += abs(scaled)

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "worker_messages": 1,
                        "gradient_operation_count": 1,
                        "samples_processed": self.batch_size,
                    },
                    "latency_totals_ms": {
                        "worker": latency_ms,
                        "worker.compute": latency_ms,
                        "worker.coordination": 0,
                    },
                    "latency_max_ms": {
                        "worker": latency_ms,
                        "worker.compute": latency_ms,
                        "worker.coordination": 0,
                    },
                    "latency_samples": {
                        "worker": 1,
                        "worker.compute": 1,
                        "worker.coordination": 1,
                    },
                },
            )
        except Exception as exc:
            return {"status": "error", "error": f"worker compute metrics update: {exc}"}

        return {
            "status": "ok",
            "actor_id": self.worker_id,
            "node_id": actor_node_id(self.worker_id),
            "worker_id": self.worker_id,
            "samples": self.batch_size,
            "samples_processed": self.batch_size,
            "latency_ms": latency_ms,
            "gradient_checksum": gradient_checksum,
            "gradient_scale": gradient_scale,
            "gradients": {"d_w1": d_w1, "d_w2": d_w2},
        }


ACTOR_ROLES = {
    "leader": Leader,
    "worker": Worker,
}
