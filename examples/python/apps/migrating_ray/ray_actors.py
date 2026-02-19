"""
Ray Parameter Server - Distributed ML Training (Python WASM)

Demonstrates the Ray parameter server pattern for distributed ML training:
- ParameterServer actor manages centralized model weights
- DataWorker actors compute gradients on data shards in parallel
- Training is orchestrated via inter-actor messaging (host.ask)

Real-world use case: Distributed ML training (TensorFlow, PyTorch, Ray).

## SDK Features Used

- @actor: Marks classes as PlexSpaces actors
- state(): Defines persistent state
- @handler(): Routes operations
- host.ask(): Inter-actor request-reply (coordinates workers)
- host.now_ms(): Timing for benchmarks
- ACTOR_ROLES: Multi-actor ApplicationSpec support
"""

import math
from typing import List, Dict, Any
from plexspaces import actor, state, handler, init_handler, host


@actor
class ParameterServer:
    """Centralized parameter server for distributed ML training."""

    # Model architecture: 2-layer neural network
    # Layer 1: input_dim x hidden_dim (e.g., 100x64 = 6,400 weights)
    # Layer 2: hidden_dim bias (e.g., 64 weights)
    # Total: 6,464 parameters
    input_dim: int = state(default=100)
    hidden_dim: int = state(default=64)
    w1: List[List[float]] = state(default_factory=list)
    w2: List[float] = state(default_factory=list)
    learning_rate: float = state(default=0.01)
    iteration: int = state(default=0)
    num_workers: int = state(default=4)
    worker_ids: List[str] = state(default_factory=list)

    # Benchmark tracking
    total_coord_ms: float = state(default=0.0)
    total_compute_ms: float = state(default=0.0)

    @init_handler
    def on_init(self, config: dict):
        """Initialize parameter server from framework config."""
        # Config comes from child_spec: {"actor_id": "parameter-server:ns@node", "args": {...}}
        # actor_id uses full name:namespace@node_id format.
        actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        lr = args.get("learning_rate", None)
        self.learning_rate = float(lr) if lr else 0.01
        inp = args.get("input_dim", None)
        self.input_dim = int(inp) if inp else 100
        hid = args.get("hidden_dim", None)
        self.hidden_dim = int(hid) if hid else 64
        nw = args.get("num_workers", None)
        self.num_workers = int(nw) if nw else 4
        self.iteration = 0
        self.total_coord_ms = 0.0
        self.total_compute_ms = 0.0

        # Xavier-like weight initialization (deterministic for reproducibility)
        self.w1 = []
        for i in range(self.hidden_dim):
            row = []
            for j in range(self.input_dim):
                val = ((i * 7 + j * 13 + 42) % 1000 - 500) / (500.0 * math.sqrt(self.input_dim))
                row.append(val)
            self.w1.append(row)
        self.w2 = [((i * 11 + 7) % 1000 - 500) / (500.0 * math.sqrt(self.hidden_dim))
                    for i in range(self.hidden_dim)]

        # Build worker IDs using full name:namespace@node_id format.
        # Extract :namespace@node_id suffix from own actor_id to construct
        # sibling actor IDs consistently.
        id_suffix = ""
        if ":" in actor_id:
            id_suffix = actor_id[actor_id.index(":"):]  # e.g. ":ray-ps@test-node"
        self.worker_ids = [f"data-worker-{i}{id_suffix}" for i in range(self.num_workers)]
        total_params = self.input_dim * self.hidden_dim + self.hidden_dim
        host.info(f"ParameterServer: {self.input_dim}x{self.hidden_dim} ({total_params} params), "
                   f"lr={self.learning_rate}, workers={self.num_workers}")

    @handler("get_weights")
    def get_weights(self) -> dict:
        """Get current model weights summary (for external inspection)."""
        w1_flat = [w for row in self.w1 for w in row]
        w1_mean = sum(w1_flat) / len(w1_flat) if w1_flat else 0.0
        w2_mean = sum(self.w2) / len(self.w2) if self.w2 else 0.0
        return {
            "status": "ok",
            "iteration": self.iteration,
            "total_params": self.input_dim * self.hidden_dim + self.hidden_dim,
            "w1_mean": w1_mean,
            "w2_mean": w2_mean,
        }

    @handler("train")
    def train(self, iterations: int = 10) -> dict:
        """
        Run synchronous distributed training for N iterations.

        Each iteration:
        1. Fan-out: send weights to all workers via host.ask (coordination)
        2. Workers: compute gradients on their data shards (computation)
        3. Fan-in: aggregate gradients and update weights (computation)
        """
        if not isinstance(iterations, int):
            iterations = int(iterations)
        total_params = self.input_dim * self.hidden_dim + self.hidden_dim
        results = []

        for it in range(iterations):
            # -- Coordination: fan-out weights to workers --
            coord_start = host.now_ms()
            weights_payload = {
                "weights": {"w1": self.w1, "w2": self.w2},
                "input_dim": self.input_dim,
                "hidden_dim": self.hidden_dim,
            }

            # Collect gradients from all workers via host.ask (request-reply)
            all_gradients = []
            for worker_id in self.worker_ids:
                try:
                    resp = host.ask(worker_id, "compute_gradients", weights_payload, timeout_ms=30000)
                    if isinstance(resp, dict) and resp.get("status") == "ok":
                        all_gradients.append(resp.get("gradients", {}))
                except Exception as e:
                    host.warn(f"Worker {worker_id} failed: {e}")

            coord_end = host.now_ms()
            coord_ms = coord_end - coord_start

            if not all_gradients:
                results.append({"iteration": self.iteration, "error": "no gradients"})
                continue

            # -- Computation: aggregate gradients and update weights --
            compute_start = host.now_ms()
            n_workers = len(all_gradients)

            # Average gradients across workers
            agg_d_w1 = [[0.0] * self.input_dim for _ in range(self.hidden_dim)]
            agg_d_w2 = [0.0] * self.hidden_dim

            for grad in all_gradients:
                d_w1 = grad.get("d_w1", [])
                d_w2 = grad.get("d_w2", [])
                for i in range(min(self.hidden_dim, len(d_w1))):
                    row = d_w1[i] if i < len(d_w1) else []
                    for j in range(min(self.input_dim, len(row))):
                        agg_d_w1[i][j] += row[j]
                    if i < len(d_w2):
                        agg_d_w2[i] += d_w2[i]

            for i in range(self.hidden_dim):
                for j in range(self.input_dim):
                    agg_d_w1[i][j] /= n_workers
                agg_d_w2[i] /= n_workers

            # SGD update: w = w - lr * grad
            for i in range(self.hidden_dim):
                for j in range(self.input_dim):
                    self.w1[i][j] -= self.learning_rate * agg_d_w1[i][j]
                self.w2[i] -= self.learning_rate * agg_d_w2[i]

            compute_end = host.now_ms()
            compute_ms = compute_end - compute_start

            self.iteration += 1
            self.total_coord_ms += coord_ms
            self.total_compute_ms += compute_ms

            # Compute loss proxy (RMS of weights - tracks convergence)
            loss = sum(w * w for row in self.w1 for w in row) + sum(w * w for w in self.w2)
            loss = math.sqrt(loss / total_params)

            results.append({
                "iteration": self.iteration,
                "loss": loss,
                "coord_ms": coord_ms,
                "compute_ms": compute_ms,
                "workers": n_workers,
            })

        return {
            "status": "ok",
            "iterations_completed": self.iteration,
            "results": results,
            "total_coord_ms": self.total_coord_ms,
            "total_compute_ms": self.total_compute_ms,
        }

    @handler("stats")
    def get_stats(self) -> dict:
        """Get comprehensive training statistics and benchmarks."""
        total_params = self.input_dim * self.hidden_dim + self.hidden_dim
        w1_flat = [w for row in self.w1 for w in row]
        w1_mean = sum(w1_flat) / len(w1_flat) if w1_flat else 0.0
        w2_mean = sum(self.w2) / len(self.w2) if self.w2 else 0.0

        total_time = self.total_coord_ms + self.total_compute_ms
        coord_pct = (self.total_coord_ms / total_time * 100) if total_time > 0 else 0
        compute_pct = (self.total_compute_ms / total_time * 100) if total_time > 0 else 0
        granularity = (self.total_compute_ms / self.total_coord_ms) if self.total_coord_ms > 0 else 0
        params_per_sec = (total_params * self.iteration * 1000 / total_time) if total_time > 0 else 0
        flops = total_params * self.iteration * 4
        gflops = flops / (total_time / 1000) / 1e9 if total_time > 0 else 0

        return {
            "status": "ok",
            "model": {"arch": f"{self.input_dim}x{self.hidden_dim}", "params": total_params,
                      "w1_mean": w1_mean, "w2_mean": w2_mean},
            "training": {"iterations": self.iteration, "lr": self.learning_rate,
                         "workers": self.num_workers},
            "benchmarks": {"total_ms": total_time, "coord_ms": self.total_coord_ms,
                           "compute_ms": self.total_compute_ms, "coord_pct": coord_pct,
                           "compute_pct": compute_pct, "granularity": granularity,
                           "params_per_sec": params_per_sec, "gflops": gflops},
        }


@actor
class DataWorker:
    """Data worker for distributed gradient computation."""

    worker_id: str = state(default="")
    shard_size: int = state(default=2000)
    batch_size: int = state(default=256)
    data_shard: List[List[float]] = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict):
        """Initialize data worker with synthetic data shard."""
        actor_id = config.get("actor_id", "")
        self.worker_id = actor_id

        # Generate synthetic data shard (worker-specific seed for diversity)
        seed = hash(actor_id) % 10000
        self.data_shard = []
        for i in range(self.shard_size):
            sample = []
            for j in range(100):  # 100 input features
                val = ((seed + i * 7 + j * 13) % 1000) / 1000.0 - 0.5
                sample.append(val)
            target = float((seed + i) % 10)
            sample.append(target)
            self.data_shard.append(sample)
        host.info(f"DataWorker {self.worker_id}: {self.shard_size} samples, batch={self.batch_size}")

    @handler("compute_gradients")
    def compute_gradients(self, weights: Dict[str, Any] = None,
                          input_dim: int = 100, hidden_dim: int = 64) -> dict:
        """
        Compute gradients via forward+backward pass on data shard.

        Forward: h = ReLU(X @ W1.T), y_pred = h @ W2
        Backward: MSE loss gradients via chain rule
        """
        if not weights:
            return {"status": "error", "error": "No weights provided"}
        w1 = weights.get("w1", [])
        w2 = weights.get("w2", [])
        if not w1 or not w2:
            return {"status": "error", "error": "Invalid weights"}
        if not isinstance(input_dim, int):
            input_dim = int(input_dim)
        if not isinstance(hidden_dim, int):
            hidden_dim = int(hidden_dim)

        d_w1 = [[0.0] * input_dim for _ in range(hidden_dim)]
        d_w2 = [0.0] * hidden_dim
        batch = self.data_shard[:self.batch_size]
        n = len(batch)

        for sample in batch:
            x = sample[:input_dim]
            target = sample[input_dim] if len(sample) > input_dim else 0.0

            # Forward: h = ReLU(W1 @ x)
            h = [0.0] * hidden_dim
            for i in range(hidden_dim):
                val = 0.0
                w1r = w1[i] if i < len(w1) else []
                for j in range(min(input_dim, len(x), len(w1r))):
                    val += w1r[j] * x[j]
                h[i] = max(0.0, val)

            # Forward: y_pred = W2 @ h
            y_pred = sum(w2[i] * h[i] for i in range(min(hidden_dim, len(w2))))
            error = y_pred - target

            # Backward: dL/dW2 = error * h
            for i in range(hidden_dim):
                d_w2[i] += error * h[i]
            # Backward: dL/dW1 = error * W2 * ReLU'(z) * x
            for i in range(hidden_dim):
                if h[i] > 0:
                    gs = error * (w2[i] if i < len(w2) else 0.0)
                    for j in range(min(input_dim, len(x))):
                        d_w1[i][j] += gs * x[j]

        # Average over batch
        if n > 0:
            for i in range(hidden_dim):
                for j in range(input_dim):
                    d_w1[i][j] /= n
                d_w2[i] /= n

        return {
            "status": "ok",
            "gradients": {"d_w1": d_w1, "d_w2": d_w2},
            "worker_id": self.worker_id,
            "samples": n,
        }

    @handler("stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "worker_id": self.worker_id,
                "shard_size": self.shard_size, "batch_size": self.batch_size}


# Multi-actor role mapping: actor_id prefix -> actor class
# Framework passes full IDs like {"actor_id": "parameter-server:ray-ps@node"}
# or {"actor_id": "data-worker-0:ray-ps@node"}; prefix matching selects the class.
ACTOR_ROLES = {
    "parameter-server": ParameterServer,
    "data-worker": DataWorker,
}
