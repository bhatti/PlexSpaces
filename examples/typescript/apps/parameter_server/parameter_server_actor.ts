// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Distributed parameter server example — TypeScript WASM actors.
//
// LeaderActor: initializes weights, creates a shard group of workers, and runs
// scatter-gather rounds.  Each round sends a small request to all workers;
// workers compute synthetic gradients on their local data shards and return
// d_w2 (hidden_dim floats) plus a gradient norm.  The leader averages and
// applies SGD to its local w1 and w2 without serializing the full weight matrix.
//
// WorkerActor: receives an iteration request, computes synthetic mini-batch
// gradients using its local seed (derived from actor_id), and returns only the
// compressed gradient (d_w2 + d_w1_norm) — keeping payloads small.

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

// ─── Helpers ──────────────────────────────────────────────────────────────────

function actorNodeId(actorId: string): string {
  if (actorId.includes("@")) return actorId.split("@").pop()!;
  return "local";
}

function actorApplicationId(actorId: string): string {
  if (actorId.includes("//") && actorId.includes("::")) {
    const suffix = actorId.split("//")[1]!;
    const qualified = suffix.split("@")[0]!;
    return qualified.split("::").pop()!;
  }
  if (actorId.includes(":") && actorId.includes("@")) {
    return actorId.split(":")[1]!.split("@")[0]!;
  }
  return "";
}

function workerSeed(actorId: string): number {
  let sum = 0;
  for (let i = 0; i < actorId.length; i++) sum += actorId.charCodeAt(i);
  return sum % 10000;
}

function intVal(v: unknown, def: number): number {
  if (typeof v === "number") return Math.trunc(v);
  if (typeof v === "string") { const n = parseInt(v, 10); return isNaN(n) ? def : n; }
  return def;
}

function floatVal(v: unknown, def: number): number {
  if (typeof v === "number") return v;
  if (typeof v === "string") { const n = parseFloat(v); return isNaN(n) ? def : n; }
  return def;
}

// ─── Types ────────────────────────────────────────────────────────────────────

interface LeaderState extends Record<string, unknown> {
  actor_id: string;
  application_id: string;
  input_dim: number;
  hidden_dim: number;
  num_workers: number;
  batch_size: number;
  learning_rate: number;
  iteration: number;
  w1: number[][];
  w2: number[];
  total_coord_ms: number;
  total_compute_ms: number;
}

interface WorkerState extends Record<string, unknown> {
  worker_id: string;
  application_id: string;
  input_dim: number;
  hidden_dim: number;
  batch_size: number;
}

// ─── LeaderActor ─────────────────────────────────────────────────────────────

class LeaderActor extends PlexSpacesActor<LeaderState> {
  getDefaultState(): LeaderState {
    return {
      actor_id: "", application_id: "",
      input_dim: 100, hidden_dim: 64,
      num_workers: 4, batch_size: 256,
      learning_rate: 0.01, iteration: 0,
      w1: [], w2: [],
      total_coord_ms: 0, total_compute_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    const args = (config.args ?? {}) as Record<string, unknown>;
    this.state.learning_rate = floatVal(args.learning_rate, 0.01);
    this.state.input_dim = intVal(args.input_dim, 100);
    this.state.hidden_dim = intVal(args.hidden_dim, 64);
    this.state.num_workers = intVal(args.num_workers, 4);
    this.state.batch_size = intVal(args.batch_size, 256);
    this.state.iteration = 0;
    this.state.total_coord_ms = 0;
    this.state.total_compute_ms = 0;
    this._initWeights();
  }

  private _initWeights(): void {
    const { input_dim: I, hidden_dim: H } = this.state;
    const w1: number[][] = [];
    for (let i = 0; i < H; i++) {
      const row: number[] = [];
      for (let j = 0; j < I; j++) {
        row.push(((i * 7 + j * 13 + 42) % 1000 - 500) / (500.0 * Math.sqrt(I)));
      }
      w1.push(row);
    }
    const w2: number[] = [];
    for (let i = 0; i < H; i++) {
      w2.push(((i * 11 + 7) % 1000 - 500) / (500.0 * Math.sqrt(H)));
    }
    this.state.w1 = w1;
    this.state.w2 = w2;
  }

  // Run N iterations of distributed SGD.
  // Workers maintain their own local weight copies and return only d_w2
  // (hidden_dim floats) plus a gradient norm — no large arrays cross the wire.
  onTrain(payload: Record<string, unknown>): unknown {
    const iterations = intVal(payload.iterations, 10);
    const appId = this.state.application_id;
    const { input_dim: I, hidden_dim: H, num_workers, learning_rate, batch_size } = this.state;

    const groupId = `ts-parameter-server-${host.nowMs()}`;
    const group = host.createShardGroup({
      groupId,
      actorType: "worker",
      shardCount: num_workers,
      partitionStrategy: "hash",
      rebalancePolicy: "manual",
      placement: { strategy: "from_registry" },
      initialState: {},
    });
    const shardActorIds = group.shardActorIds;
    if (shardActorIds.length === 0) {
      return { status: "error", error: "failed to create worker shard group" };
    }

    const leaderNodeId = actorNodeId(this.state.actor_id || host.selfId());
    let totalSamples = 0;
    let totalErrors = 0;
    let totalWorkerLatencyMs = 0;
    let totalWorkerResponses = 0;
    let maxWorkerLatencyMs = 0;
    const remoteNodesWithWork = new Set<string>();

    for (let iter = 0; iter < iterations; iter++) {
      const coordStart = host.nowMs();
      // Small payload — just the iteration number and batch size.
      // Workers use their own seeds to generate synthetic gradient data.
      const response = host.scatterGather({
        groupId,
        query: {
          op: "compute_gradient",
          iteration: iter,
          batch_size,
          input_dim: I,
          hidden_dim: H,
        },
        timeoutMs: 30000,
      });
      const coordMs = host.nowMs() - coordStart;
      this.state.total_coord_ms += coordMs;

      const computeStart = host.nowMs();
      const aggW2: number[] = new Array(H).fill(0);
      let aggW1Norm = 0;
      let nW = 0;

      for (const resp of response.shardResponses) {
        const wp = resp.payload as Record<string, unknown> | null;
        if (!wp || wp["status"] !== "ok") { totalErrors++; continue; }

        const d_w2 = wp["d_w2"] as number[] | undefined;
        if (d_w2 && d_w2.length === H) {
          for (let i = 0; i < H; i++) aggW2[i]! += d_w2[i]!;
          aggW1Norm += floatVal(wp["d_w1_norm"], 0);
          nW++;
        } else {
          totalErrors++;
          continue;
        }

        const latMs = intVal(wp["latency_ms"], 0);
        const samples = intVal(wp["samples_processed"], 0);
        totalSamples += samples;
        totalWorkerResponses++;
        totalWorkerLatencyMs += latMs;
        if (latMs > maxWorkerLatencyMs) maxWorkerLatencyMs = latMs;

        const workerNodeId = String(wp["node_id"] ?? actorNodeId(String(wp["actor_id"] ?? "")));
        if (workerNodeId && workerNodeId !== leaderNodeId) {
          remoteNodesWithWork.add(workerNodeId);
        }
      }

      if (nW === 0) {
        return { status: "error", error: `no valid gradients in iteration ${iter}` };
      }

      // SGD update: w2 from averaged worker gradients; w1 via synthetic norm-based update.
      const scale = learning_rate / nW;
      for (let i = 0; i < H; i++) {
        this.state.w2[i]! -= scale * aggW2[i]!;
        // Approximate w1 update proportional to the mean gradient norm.
        const normScale = (aggW1Norm / nW) * scale;
        for (let j = 0; j < I; j++) {
          this.state.w1[i]![j]! -= normScale * (((i * 7 + j * 13) % 100) / 100.0 - 0.5);
        }
      }

      const computeMs = host.nowMs() - computeStart;
      this.state.total_compute_ms += computeMs;
      this.state.iteration++;
    }

    try {
      host.applicationMetricsAdd(appId, {
        message_count: 1,
        counter_metrics: {
          leader_messages: iterations + 1,
          leader_runs: 1,
          weight_update_count: iterations,
          training_rounds: iterations,
        },
        latency_totals_ms: {
          leader: Math.round(this.state.total_compute_ms + this.state.total_coord_ms),
          "leader.compute": Math.round(this.state.total_compute_ms),
          "leader.coordination": Math.round(this.state.total_coord_ms),
        },
        latency_max_ms: {
          leader: Math.round(this.state.total_compute_ms + this.state.total_coord_ms),
          "leader.compute": Math.round(this.state.total_compute_ms),
          "leader.coordination": Math.round(this.state.total_coord_ms),
        },
        latency_samples: { leader: 1, "leader.compute": 1, "leader.coordination": 1 },
      });
    } catch (_) { /* non-fatal */ }

    const paramCount = I * H + H;
    let weightChecksum = 0;
    for (let i = 0; i < H; i++) {
      for (let j = 0; j < I; j++) {
        weightChecksum = (weightChecksum + Math.round(this.state.w1[i]![j]! * 1_000_000)) & 0x7FFFFFFF;
      }
      weightChecksum = (weightChecksum + Math.round(this.state.w2[i]! * 1_000_000)) & 0x7FFFFFFF;
    }

    const totalComputeMs = Math.round(this.state.total_compute_ms);
    const totalCoordMs = Math.round(this.state.total_coord_ms);

    return {
      status: "ok",
      iterations_completed: this.state.iteration,
      worker_count: shardActorIds.length,
      samples_processed: totalSamples,
      param_count: paramCount,
      iterations,
      batch_size,
      training_rounds: iterations,
      leader_node_id: leaderNodeId,
      shard_actor_ids: shardActorIds,
      node_count: 1 + new Set(shardActorIds.map(actorNodeId)).size,
      worker_node_count: remoteNodesWithWork.size,
      actor_count: shardActorIds.length + 1,
      gradient_operation_count: shardActorIds.length * iterations,
      weight_update_count: iterations,
      compute_time_ms: totalComputeMs,
      coordination_time_ms: totalCoordMs,
      total_time_ms: totalComputeMs + totalCoordMs,
      granularity_ratio: totalCoordMs > 0 ? totalComputeMs / totalCoordMs : 0,
      avg_worker_latency_ms: totalWorkerResponses > 0 ? totalWorkerLatencyMs / totalWorkerResponses : 0,
      max_worker_latency_ms: maxWorkerLatencyMs,
      error_count: totalErrors,
      remote_nodes_with_work: [...remoteNodesWithWork].sort(),
      weight_checksum: weightChecksum,
      message_count: (shardActorIds.length * iterations) + iterations + 1,
    };
  }

  onRun_scaling_benchmark(payload: Record<string, unknown>): unknown {
    const workerCounts = (payload["worker_counts"] as number[] | undefined) ?? [2, 4, 8, 16];
    const iterations = intVal(payload["iterations"], 5);
    const warmupRounds = intVal(payload["warmup_rounds"], 1);
    const benchmarkRounds = Math.max(1, intVal(payload["benchmark_rounds"], 2));
    const appId = this.state.application_id;
    const { input_dim: I, hidden_dim: H, learning_rate, batch_size } = this.state;

    const results: unknown[] = [];
    let baselineThroughput: number | null = null;

    for (const numWorkers of workerCounts) {
      // Warmup
      for (let w = 0; w < warmupRounds; w++) {
        const groupId = `ts-ps-bench-warm-${numWorkers}-${host.nowMs()}`;
        const group = host.createShardGroup({
          groupId, actorType: "worker", shardCount: numWorkers,
          partitionStrategy: "hash", rebalancePolicy: "manual",
          placement: { strategy: "from_registry" }, initialState: {},
        });
        if ((group.shardActorIds as string[]).length === 0) continue;
        for (let iter = 0; iter < iterations; iter++) {
          host.scatterGather({
            groupId,
            query: { op: "compute_gradient", iteration: iter, batch_size, input_dim: I, hidden_dim: H },
            timeoutMs: 30000,
          });
        }
      }

      let totalComputeMs = 0, totalCoordMs = 0, totalSamples = 0, totalErrors = 0;
      let totalWorkerLatencyMs = 0, totalWorkerResponses = 0;

      for (let r = 0; r < benchmarkRounds; r++) {
        const groupId = `ts-ps-bench-${numWorkers}-r${r}-${host.nowMs()}`;
        const group = host.createShardGroup({
          groupId, actorType: "worker", shardCount: numWorkers,
          partitionStrategy: "hash", rebalancePolicy: "manual",
          placement: { strategy: "from_registry" }, initialState: {},
        });
        const shardActorIds = group.shardActorIds as string[];
        if (shardActorIds.length === 0) { totalErrors++; continue; }

        for (let iter = 0; iter < iterations; iter++) {
          const coordStart = host.nowMs();
          const response = host.scatterGather({
            groupId,
            query: { op: "compute_gradient", iteration: iter, batch_size, input_dim: I, hidden_dim: H },
            timeoutMs: 30000,
          });
          const coordMs = host.nowMs() - coordStart;

          const computeStart = host.nowMs();
          const aggW2: number[] = new Array(H).fill(0);
          let nW = 0;
          for (const resp of response.shardResponses) {
            const wp = resp.payload as Record<string, unknown> | null;
            if (!wp || wp["status"] !== "ok") { totalErrors++; continue; }
            const d_w2 = wp["d_w2"] as number[] | undefined;
            if (d_w2 && d_w2.length === H) {
              for (let i = 0; i < H; i++) aggW2[i]! += d_w2[i]!;
              nW++;
            } else { totalErrors++; continue; }
            totalSamples += intVal(wp["samples_processed"], 0);
            totalWorkerLatencyMs += intVal(wp["latency_ms"], 0);
            totalWorkerResponses++;
          }
          if (nW > 0) {
            const s = learning_rate / nW;
            for (let i = 0; i < H; i++) this.state.w2[i]! -= s * aggW2[i]!;
          }
          const computeMs = host.nowMs() - computeStart;
          totalComputeMs += computeMs;
          totalCoordMs += coordMs;
        }
      }

      const totalMs = totalComputeMs + totalCoordMs;
      const avgComputeMs = Math.round(totalComputeMs / benchmarkRounds);
      const avgCoordMs = Math.round(totalCoordMs / benchmarkRounds);
      const avgTotalMs = avgComputeMs + avgCoordMs;
      const granularityRatio = avgCoordMs > 0 ? avgComputeMs / avgCoordMs : 0;
      const samplesPerSec = avgTotalMs > 0
        ? Math.round((totalSamples / benchmarkRounds) / (avgTotalMs / 1000)) : 0;

      // Throughput-based speedup: samples/sec(N) / samples/sec(baseline).
      // A parameter server adds workers to process MORE data, not the same data faster.
      // Throughput scales positively even when coordination overhead grows with worker count.
      if (baselineThroughput === null) baselineThroughput = samplesPerSec > 0 ? samplesPerSec : 1;
      const speedup = baselineThroughput > 0 ? samplesPerSec / baselineThroughput : 1.0;
      const baselineWorkers = workerCounts[0] ?? numWorkers;
      const idealSpeedup = numWorkers / baselineWorkers;
      const efficiencyPct = idealSpeedup > 0 ? Math.round((speedup / idealSpeedup) * 100) : 0;
      const parallelFraction = speedup > 1 && numWorkers > baselineWorkers
        ? (1 - 1 / speedup) / (1 - baselineWorkers / numWorkers) : 0;

      try {
        host.applicationMetricsAdd(appId, {
          counter_metrics: { benchmark_rounds: benchmarkRounds, samples_processed: totalSamples },
          latency_totals_ms: { "bench.compute": totalComputeMs, "bench.coordination": totalCoordMs },
          latency_samples: { "bench.compute": benchmarkRounds, "bench.coordination": benchmarkRounds },
        });
      } catch (_) { /* non-fatal */ }

      results.push({
        workers: numWorkers,
        iterations,
        benchmark_rounds: benchmarkRounds,
        compute_time_ms: avgComputeMs,
        coordination_time_ms: avgCoordMs,
        total_time_ms: avgTotalMs,
        granularity_ratio: Math.round(granularityRatio * 100) / 100,
        compute_pct: totalMs > 0 ? Math.round(totalComputeMs * 100 / totalMs) : 0,
        coord_pct: totalMs > 0 ? Math.round(totalCoordMs * 100 / totalMs) : 0,
        samples_per_sec: samplesPerSec,
        avg_worker_latency_ms: totalWorkerResponses > 0
          ? Math.round(totalWorkerLatencyMs / totalWorkerResponses) : 0,
        speedup: Math.round(speedup * 100) / 100,
        efficiency_pct: efficiencyPct,
        parallel_fraction: Math.round(parallelFraction * 100) / 100,
        error_count: totalErrors,
      });
    }

    return {
      status: "ok", results,
      param_count: I * H + H, input_dim: I, hidden_dim: H, batch_size,
    };
  }
}

// ─── WorkerActor ──────────────────────────────────────────────────────────────

class WorkerActor extends PlexSpacesActor<WorkerState> {
  getDefaultState(): WorkerState {
    return { worker_id: "", application_id: "", input_dim: 100, hidden_dim: 64, batch_size: 256 };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.worker_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.worker_id);
    const args = (config.args ?? {}) as Record<string, unknown>;
    this.state.batch_size = intVal(args.batch_size, 256);
    this.state.input_dim = intVal(args.input_dim, 100);
    this.state.hidden_dim = intVal(args.hidden_dim, 64);
  }

  // Compute synthetic gradient using local seed.
  // Returns d_w2 (hidden_dim floats) and d_w1_norm (single float).
  // Payloads stay small — full weight matrices never cross the wire.
  onCompute_gradient(payload: Record<string, unknown>): unknown {
    const startMs = host.nowMs();
    const batchSize = intVal(payload["batch_size"], this.state.batch_size);
    const inputDim = intVal(payload["input_dim"], this.state.input_dim);
    const hiddenDim = intVal(payload["hidden_dim"], this.state.hidden_dim);
    const { worker_id, application_id } = this.state;
    const seed = workerSeed(worker_id);

    // Compute synthetic gradient: each worker uses its unique seed + iteration
    // to simulate a different local data shard.
    const dW1Norms: number[] = new Array(hiddenDim).fill(0);
    const dW2: number[] = new Array(hiddenDim).fill(0);

    for (let sampleIdx = 0; sampleIdx < batchSize; sampleIdx++) {
      for (let i = 0; i < hiddenDim; i++) {
        const scale = ((seed + sampleIdx + i * 17) % 1000) / 1000.0 - 0.5;
        dW2[i]! += scale;
        let rowNormSq = 0;
        for (let j = 0; j < inputDim; j++) {
          const g = scale * (((seed + j * 13) % 1000) / 1000.0 - 0.5);
          rowNormSq += g * g;
        }
        dW1Norms[i]! += Math.sqrt(rowNormSq);
      }
    }

    // Normalize by batch size
    let d_w1_norm = 0;
    for (let i = 0; i < hiddenDim; i++) {
      dW2[i]! /= batchSize;
      dW1Norms[i]! /= batchSize;
      d_w1_norm += dW1Norms[i]!;
    }
    d_w1_norm /= hiddenDim;

    const latencyMs = host.nowMs() - startMs;

    try {
      host.applicationMetricsAdd(application_id, {
        message_count: 1,
        counter_metrics: {
          worker_messages: 1,
          gradient_operation_count: 1,
          samples_processed: batchSize,
        },
        latency_totals_ms: { worker: latencyMs, "worker.compute": latencyMs },
        latency_max_ms: { worker: latencyMs, "worker.compute": latencyMs },
        latency_samples: { worker: 1, "worker.compute": 1 },
      });
    } catch (_) { /* non-fatal */ }

    return {
      status: "ok",
      actor_id: worker_id,
      node_id: actorNodeId(worker_id),
      samples_processed: batchSize,
      latency_ms: latencyMs,
      // Compressed gradient: d_w2 (hidden_dim floats) + norm of d_w1 (1 float)
      d_w2: dW2,
      d_w1_norm,
    };
  }
}

// ─── Router ───────────────────────────────────────────────────────────────────

const router = new ActorRouter({
  "leader": () => new LeaderActor(),
  "worker": () => new WorkerActor(),
});

export const actor = {
  init:     (configJson: string) => router.init(configJson),
  handle:   (from: string, msgType: string, payloadJson: string) =>
              router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
