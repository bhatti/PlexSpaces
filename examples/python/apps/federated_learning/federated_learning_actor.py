# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Federated Learning Simulation - Python WASM
#
# Privacy-preserving distributed ML training: client actors train locally on
# private data partitions, send clipped+noised gradients to a central
# aggregator that applies FedAvg with differential privacy (Gaussian mechanism),
# broadcasts updated weights, and tracks convergence.
#
# Pure Python math (no numpy/sklearn). Simple logistic regression on synthetic
# binary classification (10 features).
#
# Real-world analog: Flower, PySyft, FATE, TensorFlow Federated

import math

from plexspaces import ActorID, actor, handler, init_handler, host, state
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement

_builtin_round = round

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


# ─── Pure-Python Math Helpers ────────────────────────────────────────────────

def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def sigmoid(z):
    if z >= 0:
        return 1.0 / (1.0 + math.exp(-z))
    ez = math.exp(z)
    return ez / (1.0 + ez)


def vec_add(a, b):
    return [x + y for x, y in zip(a, b)]


def vec_sub(a, b):
    return [x - y for x, y in zip(a, b)]


def vec_scale(a, s):
    return [x * s for x in a]


def vec_norm(a):
    return math.sqrt(sum(x * x for x in a))


def clip_gradient(grad, max_norm):
    n = vec_norm(grad)
    if n > max_norm:
        return vec_scale(grad, max_norm / n)
    return grad


# ─── Synthetic Data Generation ───────────────────────────────────────────────

def lcg(seed):
    return (seed * 1103515245 + 12345) & 0x7FFFFFFF


def generate_dataset(num_samples, num_features, seed=42):
    rng = seed
    true_weights = []
    for i in range(num_features):
        rng = lcg(rng)
        true_weights.append((rng % 200 - 100) / 100.0)

    X = []
    y = []
    for _ in range(num_samples):
        row = []
        for _ in range(num_features):
            rng = lcg(rng)
            row.append((rng % 2000 - 1000) / 1000.0)
        X.append(row)
        logit = dot(row, true_weights)
        rng = lcg(rng)
        noise = (rng % 200 - 100) / 500.0
        label = 1 if sigmoid(logit + noise) > 0.5 else 0
        y.append(label)

    return X, y, true_weights


def partition_data(X, y, num_partitions):
    n = len(X)
    size = n // num_partitions
    partitions = []
    for i in range(num_partitions):
        start = i * size
        end = start + size if i < num_partitions - 1 else n
        partitions.append((X[start:end], y[start:end]))
    return partitions


# ─── Logistic Regression (pure Python) ──────────────────────────────────────

def predict(X, weights, bias):
    return [sigmoid(dot(x, weights) + bias) for x in X]


def binary_cross_entropy(y_true, y_pred):
    eps = 1e-7
    total = 0.0
    for yt, yp in zip(y_true, y_pred):
        yp = max(eps, min(1 - eps, yp))
        total += -(yt * math.log(yp) + (1 - yt) * math.log(1 - yp))
    return total / len(y_true) if y_true else 0.0


def compute_gradients(X, y, weights, bias, lr=0.1):
    n = len(X)
    if n == 0:
        return [0.0] * len(weights), 0.0

    preds = predict(X, weights, bias)
    errors = [p - yt for p, yt in zip(preds, y)]

    grad_w = [0.0] * len(weights)
    for i in range(n):
        for j in range(len(weights)):
            grad_w[j] += errors[i] * X[i][j]
    grad_w = [g / n for g in grad_w]

    grad_b = sum(errors) / n

    return grad_w, grad_b


def accuracy(y_true, y_pred):
    correct = sum(1 for yt, yp in zip(y_true, y_pred) if (yp >= 0.5) == (yt == 1))
    return correct / len(y_true) if y_true else 0.0


# ─── Differential Privacy ───────────────────────────────────────────────────

def gaussian_noise(seed, size, sigma):
    values = []
    rng = seed
    for _ in range(size):
        # Box-Muller transform using LCG
        rng = lcg(rng)
        u1 = max(1e-10, (rng % 10000) / 10000.0)
        rng = lcg(rng)
        u2 = (rng % 10000) / 10000.0
        z = math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)
        values.append(z * sigma)
    return values, rng


def compute_dp_sigma(epsilon, delta, sensitivity, num_rounds):
    # Gaussian mechanism: sigma = sensitivity * sqrt(2 * ln(1.25/delta)) / epsilon
    # With composition over rounds
    per_round_eps = epsilon / math.sqrt(num_rounds)
    sigma = sensitivity * math.sqrt(2.0 * math.log(1.25 / delta)) / per_round_eps
    return sigma


# ─── Aggregator Actor (Leader) ───────────────────────────────────────────────

@register_role("leader")
@actor
class AggregatorActor:
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
        num_clients: int = 4,
        num_rounds: int = 20,
        num_features: int = 10,
        total_samples: int = 2000,
        learning_rate: float = 0.5,
        max_grad_norm: float = 1.0,
        epsilon: float = 1.0,
        delta: float = 1e-5,
        convergence_threshold: float = 0.001,
        from_actor: str = "",
    ) -> dict:

        # Create shard group for client workers
        coord_start = host.now_ms()
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=f"fl-clients-{host.now_ms()}",
            actor_type="ClientActor",
            shard_count=num_clients,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        group_id = group.group_id
        shard_ids = group.shard_actor_ids
        if not shard_ids:
            return {"status": "error", "error": "failed to create client shard group"}
        coord_create = host.now_ms() - coord_start

        # Generate dataset and partition
        X, y, true_weights = generate_dataset(total_samples, num_features, seed=42)
        partitions = partition_data(X, y, num_clients)

        # Initialize model
        weights = [0.0] * num_features
        bias = 0.0
        dp_sigma = compute_dp_sigma(epsilon, delta, max_grad_norm, num_rounds)

        total_compute_ms = 0
        total_coord_ms = coord_create
        round_history = []
        rng_seed = 12345

        for round_idx in range(num_rounds):
            # Distribute training to clients
            sg_start = host.now_ms()
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={
                    "op": "train_round",
                    "weights": weights,
                    "bias": bias,
                    "learning_rate": learning_rate,
                    "max_grad_norm": max_grad_norm,
                    "round": round_idx,
                    "total_samples": total_samples,
                    "num_features": num_features,
                    "num_clients": num_clients,
                },
                timeout_ms=30000,
            ))
            sg_elapsed = host.now_ms() - sg_start
            total_coord_ms += sg_elapsed

            # Collect gradients from all clients
            all_grad_w = []
            all_grad_b = []
            client_compute = 0
            client_losses = []
            client_accuracies = []
            client_samples = []

            for resp in sg_result.shard_responses:
                result = resp
                for key in ["payload", "result", "response", "data"]:
                    if isinstance(result, dict) and key in result:
                        result = result[key]
                if not isinstance(result, dict) or "error" in result:
                    continue
                all_grad_w.append(result.get("gradient_w", [0.0] * num_features))
                all_grad_b.append(result.get("gradient_b", 0.0))
                client_compute += result.get("compute_ms", 0)
                client_losses.append(result.get("loss", 0.0))
                client_accuracies.append(result.get("accuracy", 0.0))
                client_samples.append(result.get("num_samples", 0))

            if not all_grad_w:
                continue

            total_compute_ms += client_compute

            # FedAvg: weighted average of gradients
            agg_start = host.now_ms()
            total_client_samples = sum(client_samples) or 1
            avg_grad_w = [0.0] * num_features
            avg_grad_b = 0.0

            for i, (gw, gb, ns) in enumerate(zip(all_grad_w, all_grad_b, client_samples)):
                weight_factor = ns / total_client_samples
                for j in range(num_features):
                    avg_grad_w[j] += gw[j] * weight_factor
                avg_grad_b += gb * weight_factor

            # Add DP noise (Gaussian mechanism)
            noise_w, rng_seed = gaussian_noise(rng_seed, num_features, dp_sigma / total_client_samples)
            rng_seed = lcg(rng_seed)
            noise_b_vals, rng_seed = gaussian_noise(rng_seed, 1, dp_sigma / total_client_samples)
            noise_b = noise_b_vals[0]

            noised_grad_w = vec_add(avg_grad_w, noise_w)
            noised_grad_b = avg_grad_b + noise_b

            # Update global model
            weights = vec_sub(weights, vec_scale(noised_grad_w, learning_rate))
            bias = bias - learning_rate * noised_grad_b

            agg_elapsed = host.now_ms() - agg_start
            total_compute_ms += agg_elapsed

            # Evaluate on full dataset
            preds = predict(X, weights, bias)
            global_loss = binary_cross_entropy(y, preds)
            global_acc = accuracy(y, preds)
            avg_client_loss = sum(client_losses) / len(client_losses) if client_losses else 0.0
            avg_client_acc = sum(client_accuracies) / len(client_accuracies) if client_accuracies else 0.0

            round_history.append({
                "round": round_idx,
                "global_loss": round(global_loss, 4),
                "global_accuracy": round(global_acc, 4),
                "avg_client_loss": round(avg_client_loss, 4),
                "avg_client_accuracy": round(avg_client_acc, 4),
                "dp_noise_norm": round(vec_norm(noise_w), 4),
                "grad_norm": round(vec_norm(avg_grad_w), 4),
            })

            # Convergence check
            if round_idx > 2:
                loss_delta = abs(round_history[-2]["global_loss"] - round_history[-1]["global_loss"])
                if loss_delta < convergence_threshold:
                    break

        wall_time = total_compute_ms + total_coord_ms
        total = total_compute_ms + total_coord_ms or 1
        granularity = round(total_compute_ms / total_coord_ms, 1) if total_coord_ms > 0 else 0

        final_preds = predict(X, weights, bias)
        final_loss = binary_cross_entropy(y, final_preds)
        final_acc = accuracy(y, final_preds)

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {
                        "aggregator.compute": total_compute_ms,
                        "aggregator.coordination": total_coord_ms,
                    },
                    "latency_samples": {
                        "aggregator.compute": 1,
                        "aggregator.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "status": "ok",
            "num_clients": num_clients,
            "num_rounds": len(round_history),
            "max_rounds": num_rounds,
            "num_features": num_features,
            "total_samples": total_samples,
            "final_loss": round(final_loss, 4),
            "final_accuracy": round(final_acc, 4),
            "converged": len(round_history) < num_rounds,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "dp_epsilon": epsilon,
            "dp_delta": delta,
            "dp_sigma": round(dp_sigma, 4),
            "round_history": round_history[:5] + round_history[-3:] if len(round_history) > 8 else round_history,
            "actor_count": len(shard_ids) + 1,
            "error_count": 0,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        total_samples: int = 2000,
        client_counts: list = None,
        num_rounds: int = 10,
        num_features: int = 10,
        learning_rate: float = 0.5,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if client_counts is None:
            client_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion: 2 FL rounds per timing pass
        TIMING_ROUNDS = 2

        results = []
        baseline_eps = 0

        for client_count in client_counts:
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"fl-bench-{client_count}-{host.now_ms()}",
                actor_type="ClientActor",
                shard_count=client_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            warmup_weights = [0.0] * num_features
            warmup_bias = 0.0
            # Each client processes its proportional share: total_samples // client_count.
            # Sending total_samples=S with num_clients=1 means each client generates S samples
            # and uses all of them, making per-client work O(S/N) for strong-scaling correctness.
            samples_per_client = max(1, total_samples // client_count)

            # 1 warmup scatter-gather (discard timing)
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={
                    "op": "train_round",
                    "weights": warmup_weights, "bias": warmup_bias,
                    "learning_rate": learning_rate, "max_grad_norm": 1.0,
                    "round": 0, "total_samples": samples_per_client,
                    "num_features": num_features, "num_clients": 1,
                },
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0

            for _bi in range(benchmark_rounds):
                bench_weights = [0.0] * num_features
                bench_bias = 0.0
                iter_start = host.now_ms()
                iter_compute = 0

                for r_idx in range(TIMING_ROUNDS):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={
                            "op": "train_round",
                            "weights": bench_weights, "bias": bench_bias,
                            "learning_rate": learning_rate, "max_grad_norm": 1.0,
                            "round": r_idx, "total_samples": samples_per_client,
                            "num_features": num_features, "num_clients": 1,
                        },
                        timeout_ms=30000,
                    ))

                    all_gw = []; all_gb = []; all_ns = []
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict) and "gradient_w" in r:
                            all_gw.append(r["gradient_w"])
                            all_gb.append(r.get("gradient_b", 0.0))
                            all_ns.append(r.get("num_samples", 1))
                            iter_compute += r.get("compute_ms", 0)

                    if all_gw:
                        total_ns = sum(all_ns) or 1
                        avg_gw = [0.0] * num_features
                        avg_gb = 0.0
                        for gw, gb, ns in zip(all_gw, all_gb, all_ns):
                            wf = ns / total_ns
                            for j in range(num_features):
                                avg_gw[j] += gw[j] * wf
                            avg_gb += gb * wf
                        bench_weights = [w - learning_rate * g for w, g in zip(bench_weights, avg_gw)]
                        bench_bias = bench_bias - learning_rate * avg_gb

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            # Throughput: gradient computations per second (total_samples × TIMING_ROUNDS)
            actual_work = TIMING_ROUNDS * total_samples
            avg_eps = actual_work * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1

            speedup_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100
            speedup = speedup_100 / 100
            eff_100 = speedup_100 * client_counts[0] // client_count if client_count > 0 else 100
            efficiency = eff_100 / 1

            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "clients": client_count,
                "rounds_per_bench": TIMING_ROUNDS,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran,
                "samples_per_sec": avg_eps,
                "speedup": speedup,
                "efficiency_pct": efficiency,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "total_samples": total_samples,
            "num_features": num_features,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        samples_per_client: int = 500,
        client_counts: list = None,
        num_rounds: int = 10,
        num_features: int = 10,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if client_counts is None:
            client_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion: 2 FL rounds per timing pass
        TIMING_ROUNDS = 2

        results = []
        baseline_eps = 0

        for client_count in client_counts:
            total_samples = samples_per_client * client_count

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"fl-weak-{client_count}-{host.now_ms()}",
                actor_type="ClientActor",
                shard_count=client_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            warmup_weights = [0.0] * num_features
            warmup_bias = 0.0
            # Each client processes samples_per_client samples (fixed per worker for weak scaling)
            weak_samples_per_client = samples_per_client

            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={
                    "op": "train_round",
                    "weights": warmup_weights, "bias": warmup_bias,
                    "learning_rate": 0.5, "max_grad_norm": 1.0,
                    "round": 0, "total_samples": weak_samples_per_client,
                    "num_features": num_features, "num_clients": 1,
                },
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0

            for _bi in range(benchmark_rounds):
                bench_weights = [0.0] * num_features
                bench_bias = 0.0
                iter_start = host.now_ms()
                iter_compute = 0

                for r_idx in range(TIMING_ROUNDS):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={
                            "op": "train_round",
                            "weights": bench_weights, "bias": bench_bias,
                            "learning_rate": 0.5, "max_grad_norm": 1.0,
                            "round": r_idx, "total_samples": weak_samples_per_client,
                            "num_features": num_features, "num_clients": 1,
                        },
                        timeout_ms=30000,
                    ))

                    all_gw = []; all_gb = []; all_ns = []
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict) and "gradient_w" in r:
                            all_gw.append(r["gradient_w"])
                            all_gb.append(r.get("gradient_b", 0.0))
                            all_ns.append(r.get("num_samples", 1))
                            iter_compute += r.get("compute_ms", 0)

                    if all_gw:
                        total_ns = sum(all_ns) or 1
                        avg_gw = [0.0] * num_features
                        avg_gb = 0.0
                        for gw, gb, ns in zip(all_gw, all_gb, all_ns):
                            wf = ns / total_ns
                            for j in range(num_features):
                                avg_gw[j] += gw[j] * wf
                            avg_gb += gb * wf
                        bench_weights = [w - 0.5 * g for w, g in zip(bench_weights, avg_gw)]
                        bench_bias = bench_bias - 0.5 * avg_gb

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            actual_work = TIMING_ROUNDS * total_samples
            avg_eps = actual_work * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_eps == 0:
                baseline_eps = avg_eps or 1

            eff_100 = avg_eps * 100 // baseline_eps if baseline_eps > 0 else 100

            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "clients": client_count,
                "total_samples": total_samples,
                "samples_per_sec": avg_eps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran,
                "efficiency_pct": eff_100,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "samples_per_client": samples_per_client,
            "num_features": num_features,
            "results": results,
        }


# ─── Client Actor (Worker) ──────────────────────────────────────────────────

@register_role("worker")
@actor
class ClientActor:
    rounds_trained: int = state(default=0)
    total_compute_ms: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("train_round")
    def train_round(
        self,
        weights: list = None,
        bias: float = 0.0,
        learning_rate: float = 0.5,
        max_grad_norm: float = 1.0,
        round: int = 0,
        total_samples: int = 2000,
        num_features: int = 10,
        num_clients: int = 4,
        from_actor: str = "",
    ) -> dict:
        if weights is None:
            weights = []
        round_idx = round
        comp_start = host.now_ms()

        # Determine this client's partition from shard index
        # Use actor_id hash to get a deterministic partition index
        actor_id_str = self.actor_id
        shard_idx = 0
        for ch in actor_id_str:
            shard_idx = (shard_idx * 31 + ord(ch)) & 0x7FFFFFFF
        shard_idx = shard_idx % num_clients

        # Generate this client's data partition (deterministic)
        X, y, _ = generate_dataset(total_samples, num_features, seed=42)
        partitions = partition_data(X, y, num_clients)
        if shard_idx < len(partitions):
            X_local, y_local = partitions[shard_idx]
        else:
            X_local, y_local = partitions[0]

        # Compute gradients on local data
        grad_w, grad_b = compute_gradients(X_local, y_local, weights, bias, learning_rate)

        # Clip gradients for DP
        grad_w = clip_gradient(grad_w, max_grad_norm)
        grad_b = max(-max_grad_norm, min(max_grad_norm, grad_b))

        # Evaluate local model
        preds = predict(X_local, weights, bias)
        loss = binary_cross_entropy(y_local, preds)
        acc = accuracy(y_local, preds)

        compute_ms = host.now_ms() - comp_start
        self.rounds_trained += 1
        self.total_compute_ms += compute_ms

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {
                        "client.compute": compute_ms,
                    },
                    "latency_samples": {
                        "client.compute": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "gradient_w": [_builtin_round(g, 6) for g in grad_w],
            "gradient_b": _builtin_round(grad_b, 6),
            "loss": _builtin_round(loss, 4),
            "accuracy": _builtin_round(acc, 4),
            "num_samples": len(X_local),
            "compute_ms": compute_ms,
            "shard_idx": shard_idx,
            "round": round_idx,
        }
