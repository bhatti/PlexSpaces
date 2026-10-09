// SPDX-License-Identifier: AGPL-3.0-or-later
//
// LLM Serving with Request Batching & Model Multiplexing - TypeScript WASM
//
// Production-grade LLM serving: cost-aware routing across model tiers
// (small/medium/large), dynamic request batching, health monitoring,
// streaming responses, supervision for model health.
//
// Architecture: RouterActor (leader) dispatches to ModelActor (worker) shards
// with cost-aware tier selection and dynamic batching.

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

// ─── Types ───────────────────────────────────────────────────────────────────

interface BaseState extends Record<string, unknown> {
  actor_id: string;
  application_id: string;
  role: string;
}

interface RouterState extends BaseState {
  role: "leader";
  total_compute_ms: number;
  total_coord_ms: number;
}

interface ModelState extends BaseState {
  role: "worker";
  model_tier: string;
  requests_served: number;
  tokens_generated: number;
  failures: number;
  compute_ms: number;
}

interface InferenceRequest {
  request_id: string;
  prompt: string;
  task_type: string; // classification, summarization, generation, embedding, chat
  max_tokens: number;
  priority: string;  // low, normal, high
  user_id: string;
}

interface InferenceResult {
  request_id: string;
  model_tier: string;
  tokens_generated: number;
  latency_ms: number;
  cost_units: number;
  status: string;
  output_preview: string;
}

interface ModelHealth {
  tier: string;
  healthy: boolean;
  requests_served: number;
  tokens_generated: number;
  failures: number;
  avg_latency_ms: number;
  error_rate_pct: number;
}

// ─── Model Tier Configuration ────────────────────────────────────────────────

interface TierConfig {
  name: string;
  latency_base_ms: number;
  latency_per_token_ms: number;
  cost_per_token: number;
  max_batch_size: number;
  failure_rate: number;
  max_tokens: number;
}

const MODEL_TIERS: Record<string, TierConfig> = {
  small: {
    name: "small-7b",
    latency_base_ms: 5,
    latency_per_token_ms: 0.5,
    cost_per_token: 0.01,
    max_batch_size: 32,
    failure_rate: 0.01,
    max_tokens: 256,
  },
  medium: {
    name: "medium-13b",
    latency_base_ms: 15,
    latency_per_token_ms: 1.5,
    cost_per_token: 0.05,
    max_batch_size: 16,
    failure_rate: 0.02,
    max_tokens: 1024,
  },
  large: {
    name: "large-70b",
    latency_base_ms: 50,
    latency_per_token_ms: 3.0,
    cost_per_token: 0.15,
    max_batch_size: 8,
    failure_rate: 0.03,
    max_tokens: 4096,
  },
};

// ─── Task → Tier Routing ─────────────────────────────────────────────────────

const TASK_TIER_MAP: Record<string, string> = {
  classification: "small",
  embedding: "small",
  summarization: "medium",
  chat: "medium",
  generation: "large",
  reasoning: "large",
};

function selectTier(taskType: string, priority: string): string {
  const baseTier = TASK_TIER_MAP[taskType] ?? "medium";
  if (priority === "high" && baseTier === "small") return "medium";
  if (priority === "high" && baseTier === "medium") return "large";
  return baseTier;
}

// ─── Request Generator ──────────────────────────────────────────────────────

const TASK_TYPES = ["classification", "summarization", "generation", "embedding", "chat", "reasoning"];
const PROMPTS: Record<string, string[]> = {
  classification: [
    "Classify sentiment: 'The product exceeded my expectations'",
    "Classify intent: 'I want to cancel my subscription'",
    "Classify topic: 'The Federal Reserve raised interest rates'",
  ],
  summarization: [
    "Summarize: The quarterly earnings report showed revenue growth of 15%...",
    "Summarize: In a landmark decision, the Supreme Court ruled...",
    "Summarize: Researchers at MIT developed a new algorithm...",
  ],
  generation: [
    "Write a technical blog post about distributed systems",
    "Generate a product description for a smart home device",
    "Write an API documentation section for the /users endpoint",
  ],
  embedding: [
    "Encode: distributed actor systems with fault tolerance",
    "Encode: machine learning model serving at scale",
    "Encode: real-time data pipeline architecture",
  ],
  chat: [
    "User: How do I implement a load balancer? Assistant:",
    "User: Explain the CAP theorem simply. Assistant:",
    "User: What's the best way to handle backpressure? Assistant:",
  ],
  reasoning: [
    "Step by step, solve: If a train travels at 60mph for 2.5 hours...",
    "Analyze the trade-offs between consistency and availability",
    "Compare microservices vs monolith for a 50-person team",
  ],
};

function generateRequests(count: number, seed: number): InferenceRequest[] {
  const requests: InferenceRequest[] = [];
  let rng = seed;
  const priorities = ["low", "normal", "normal", "normal", "high"];

  for (let i = 0; i < count; i++) {
    rng = (rng * 1103515245 + 12345) & 0x7fffffff;
    const taskType = TASK_TYPES[rng % TASK_TYPES.length]!;
    const promptList = PROMPTS[taskType] ?? PROMPTS["chat"]!;
    const prompt = promptList[(rng >> 8) % promptList.length]!;
    const priority = priorities[(rng >> 16) % priorities.length]!;

    rng = (rng * 1103515245 + 12345) & 0x7fffffff;
    const maxTokens = taskType === "classification" || taskType === "embedding"
      ? 32 + (rng % 64)
      : taskType === "summarization" || taskType === "chat"
        ? 128 + (rng % 384)
        : 256 + (rng % 768);

    requests.push({
      request_id: `req-${i}-${rng % 100000}`,
      prompt,
      task_type: taskType,
      max_tokens: maxTokens,
      priority,
      user_id: `user-${(rng >> 4) % 100}`,
    });
  }
  return requests;
}

// ─── Simulated Inference ────────────────────────────────────────────────────

function simulateInference(
  request: InferenceRequest,
  tier: TierConfig,
  rng: number,
): { result: InferenceResult; rng: number } {
  rng = (rng * 1103515245 + 12345) & 0x7fffffff;
  const failed = (rng % 100) < tier.failure_rate * 100;

  if (failed) {
    return {
      result: {
        request_id: request.request_id,
        model_tier: tier.name,
        tokens_generated: 0,
        latency_ms: tier.latency_base_ms,
        cost_units: 0,
        status: "error",
        output_preview: "MODEL_ERROR: inference failed",
      },
      rng,
    };
  }

  const tokens = Math.min(request.max_tokens, tier.max_tokens);
  rng = (rng * 1103515245 + 12345) & 0x7fffffff;
  const jitter = 0.8 + ((rng % 40) / 100);
  const latency = Math.round(
    (tier.latency_base_ms + tokens * tier.latency_per_token_ms) * jitter,
  );
  const cost = Math.round(tokens * tier.cost_per_token * 100) / 100;

  rng = (rng * 1103515245 + 12345) & 0x7fffffff;
  const words = ["The", "result", "shows", "that", "analysis", "indicates",
    "processing", "complete", "model", "output", "generated", "tokens",
    "inference", "computed", "response", "distributed", "pipeline"];
  let preview = "";
  for (let w = 0; w < Math.min(tokens / 4, 10); w++) {
    rng = (rng * 1103515245 + 12345) & 0x7fffffff;
    preview += (w > 0 ? " " : "") + words[rng % words.length];
  }

  return {
    result: {
      request_id: request.request_id,
      model_tier: tier.name,
      tokens_generated: tokens,
      latency_ms: latency,
      cost_units: cost,
      status: "ok",
      output_preview: preview,
    },
    rng,
  };
}

// ─── Router Actor (Leader) ──────────────────────────────────────────────────

class RouterActor extends PlexSpacesActor<RouterState> {
  getDefaultState(): RouterState {
    return {
      actor_id: "", application_id: "", role: "leader",
      total_compute_ms: 0, total_coord_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = "leader";
    this.state.total_compute_ms = 0;
    this.state.total_coord_ms = 0;
  }

  onRun(payload: Record<string, unknown>): Record<string, unknown> {
    const requestCount = intValue(payload.request_count, 1000);
    const workerCount = intValue(payload.worker_count, 6);
    const batchSize = intValue(payload.batch_size, 16);

    // Create shard group for model workers
    const coordStart = host.nowMs();
    const groupId = `llm-serving-ts-${host.nowMs()}`;
    const group = host.createShardGroup({
      groupId,
      actorType: "llm-worker",
      shardCount: workerCount,
      partitionStrategy: "hash",
      rebalancePolicy: "manual",
      placement: { strategy: "from_registry" },
      initialState: {},
    });
    const shardActorIds = group.shardActorIds;
    if (shardActorIds.length === 0) {
      return { status: "error", error: "failed to create model worker shard group" };
    }
    const coordCreate = host.nowMs() - coordStart;

    const leaderNodeId = actorNodeId(this.state.actor_id);
    const requests = generateRequests(requestCount, host.nowMs() % 100000);

    // Route requests to tiers, batch them
    const tierBatches: Record<string, InferenceRequest[][]> = {};
    const tierCounts: Record<string, number> = {};

    for (const req of requests) {
      const tier = selectTier(req.task_type, req.priority);
      tierCounts[tier] = (tierCounts[tier] ?? 0) + 1;
      if (!tierBatches[tier]) tierBatches[tier] = [];
      const batches = tierBatches[tier]!;
      if (batches.length === 0 || batches[batches.length - 1]!.length >= batchSize) {
        batches.push([]);
      }
      batches[batches.length - 1]!.push(req);
    }

    let totalComputeMs = 0;
    let totalCoordMs = coordCreate;
    let totalTokens = 0;
    let totalCost = 0;
    let totalLatency = 0;
    let maxLatency = 0;
    let successCount = 0;
    let failureCount = 0;
    let batchCount = 0;
    const tierResults: Record<string, { count: number; tokens: number; cost: number; latency: number; errors: number }> = {};

    // Process each tier's batches via scatter-gather
    for (const [tier, batches] of Object.entries(tierBatches)) {
      for (const batch of batches!) {
        batchCount++;
        const sgStart = host.nowMs();
        const sgResult = host.scatterGather({
          groupId,
          query: {
            op: "process_batch",
            requests: batch,
            model_tier: tier,
          },
          timeoutMs: 30000,
        });
        const sgElapsed = host.nowMs() - sgStart;
        totalCoordMs += sgElapsed;

        for (const resp of sgResult.shardResponses) {
          const result = recordValue(resp.payload ?? resp);
          if (result.error && !result.batch_results) continue;

          const batchResults = anyArray(result.batch_results);
          const computeMs = intValue(result.compute_ms, 0);
          totalComputeMs += computeMs;

          if (!tierResults[tier]) {
            tierResults[tier] = { count: 0, tokens: 0, cost: 0, latency: 0, errors: 0 };
          }

          for (const br of batchResults) {
            const item = recordValue(br);
            if (stringValue(item.status) === "ok") {
              successCount++;
              tierResults[tier]!.count++;
              const tokens = intValue(item.tokens_generated, 0);
              const cost = floatValue(item.cost_units, 0);
              const lat = intValue(item.latency_ms, 0);
              totalTokens += tokens;
              totalCost += cost;
              totalLatency += lat;
              if (lat > maxLatency) maxLatency = lat;
              tierResults[tier]!.tokens += tokens;
              tierResults[tier]!.cost += cost;
              tierResults[tier]!.latency += lat;
            } else {
              failureCount++;
              tierResults[tier]!.errors++;
            }
          }
        }
      }
    }

    const wallTime = totalComputeMs + totalCoordMs;
    const total = totalComputeMs + totalCoordMs || 1;
    const granularity = totalCoordMs > 0 ? Math.round((totalComputeMs / totalCoordMs) * 10) / 10 : 0;
    const requestsPerSec = wallTime > 0 ? Math.round((requestCount / wallTime) * 1000) : 0;
    const tokensPerSec = wallTime > 0 ? Math.round((totalTokens / wallTime) * 1000) : 0;
    const avgLatency = successCount > 0 ? Math.round(totalLatency / successCount) : 0;
    const errorRate = requestCount > 0 ? Math.round((failureCount / requestCount) * 10000) / 100 : 0;

    const nodeActorCounts = computeActorCounts(leaderNodeId, shardActorIds);

    try { host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "leader.compute": totalComputeMs, "leader.coordination": totalCoordMs, "leader.requests": requestCount, "leader.tokens": totalTokens } }); } catch (_e) {}

    // Build per-tier summary
    const tierSummary: Record<string, unknown> = {};
    for (const [tier, stats] of Object.entries(tierResults)) {
      tierSummary[tier] = {
        requests: stats.count,
        tokens: stats.tokens,
        cost: Math.round(stats.cost * 100) / 100,
        avg_latency_ms: stats.count > 0 ? Math.round(stats.latency / stats.count) : 0,
        errors: stats.errors,
      };
    }

    return {
      status: "ok",
      request_count: requestCount,
      worker_count: workerCount,
      batch_size: batchSize,
      wall_time_ms: wallTime,
      compute_time_ms: totalComputeMs,
      coordination_time_ms: totalCoordMs,
      granularity_ratio: granularity,
      requests_per_sec: requestsPerSec,
      tokens_per_sec: tokensPerSec,
      total_tokens: totalTokens,
      total_cost: Math.round(totalCost * 100) / 100,
      success_count: successCount,
      failure_count: failureCount,
      error_rate_pct: errorRate,
      avg_latency_ms: avgLatency,
      max_latency_ms: maxLatency,
      batches_processed: batchCount,
      tier_routing: tierCounts,
      tier_results: tierSummary,
      node_count: Object.keys(nodeActorCounts).length,
      actor_count: shardActorIds.length + 1,
      leader_node_id: leaderNodeId,
      nodes: nodeActorCounts,
      error_count: failureCount,
    };
  }

  onRun_scaling_benchmark(payload: Record<string, unknown>): Record<string, unknown> {
    const requestCount = intValue(payload.request_count, 1000);
    const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 6, 8]);
    const batchSize = intValue(payload.batch_size, 16);
    const warmupRounds = intValue(payload.warmup_rounds, 1);
    const benchmarkRounds = intValue(payload.benchmark_rounds, 2);

    const results: Record<string, unknown>[] = [];
    let baselineWall = 0;

    for (const shardCount of shardCounts) {
      for (let w = 0; w < warmupRounds; w++) {
        this.onRun({ request_count: Math.min(requestCount, 200), worker_count: shardCount, batch_size: batchSize });
      }

      let totalWall = 0, totalCompute = 0, totalCoord = 0, totalReqPerSec = 0;
      let totalTokPerSec = 0, nodeCount = 0, errorCount = 0;

      const runRequestCount = Math.min(requestCount, batchSize * 2);
      for (let r = 0; r < benchmarkRounds; r++) {
        const result = this.onRun({ request_count: runRequestCount, worker_count: shardCount, batch_size: batchSize });
        totalWall += intValue(result.wall_time_ms, 0);
        totalCompute += intValue(result.compute_time_ms, 0);
        totalCoord += intValue(result.coordination_time_ms, 0);
        totalReqPerSec += intValue(result.requests_per_sec, 0);
        totalTokPerSec += intValue(result.tokens_per_sec, 0);
        nodeCount = intValue(result.node_count, 0);
        errorCount += intValue(result.error_count, 0);
      }

      const avgWall = Math.round(totalWall / benchmarkRounds);
      const avgCompute = Math.round(totalCompute / benchmarkRounds);
      const avgCoord = Math.round(totalCoord / benchmarkRounds);
      const avgReqPerSec = Math.round(totalReqPerSec / benchmarkRounds);
      const avgTokPerSec = Math.round(totalTokPerSec / benchmarkRounds);
      const total = avgCompute + avgCoord || 1;

      if (baselineWall === 0) baselineWall = avgWall;
      const speedup = baselineWall > 0 && avgWall > 0 ? baselineWall / avgWall : 1;
      const efficiency = (speedup / (shardCount / shardCounts[0]!)) * 100;

      results.push({
        shards: shardCount,
        requests_per_sec: avgReqPerSec,
        tokens_per_sec: avgTokPerSec,
        wall_time_ms: avgWall,
        compute_time_ms: avgCompute,
        coordination_time_ms: avgCoord,
        compute_pct: (avgCompute * 100) / total,
        granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
        speedup: Math.round(speedup * 100) / 100,
        efficiency_pct: Math.round(efficiency * 10) / 10,
        node_count: nodeCount,
        error_count: errorCount,
      });
    }

    return { status: "ok", request_count: requestCount, batch_size: batchSize, results };
  }

  onRun_weak_scaling_benchmark(payload: Record<string, unknown>): Record<string, unknown> {
    const requestsPerShard = intValue(payload.requests_per_shard, 200);
    const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 6, 8]);
    const numPasses = intValue(payload.num_passes, 4);
    const benchmarkRounds = intValue(payload.benchmark_rounds, 1);

    const results: Record<string, unknown>[] = [];
    let baselineThroughput = 0;

    for (const shardCount of shardCounts) {
      const groupId = `llm-bench-ts-${host.nowMs()}`;
      host.createShardGroup({
        groupId,
        actorType: "llm-worker",
        shardCount,
        partitionStrategy: "hash",
        rebalancePolicy: "manual",
        placement: { strategy: "from_registry" },
        initialState: {},
      });

      let totalComputeMs = 0;
      let totalCoordMs = 0;
      let totalRequests = 0;
      let totalTokens = 0;
      let errorCount = 0;

      for (let r = 0; r < benchmarkRounds; r++) {
        const wallStart = host.nowMs();
        // Single broadcast SG — all workers run in parallel, each generates its own requests
        const sgResult = host.scatterGather({
          groupId,
          query: {
            op: "benchmark_requests",
            requests_per_shard: requestsPerShard,
            num_passes: numPasses,
            seed: r + shardCount * 100,
          },
          timeoutMs: 60000,
        });
        const wallMs = host.nowMs() - wallStart;

        let roundCompute = 0;
        let roundRequests = 0;
        let roundTokens = 0;
        for (const resp of sgResult.shardResponses) {
          const result = recordValue(resp.payload ?? resp);
          if (result.error) { errorCount++; continue; }
          roundCompute += intValue(result.compute_ms, 0);
          roundRequests += intValue(result.requests_processed, 0);
          roundTokens += intValue(result.tokens_generated, 0);
        }
        totalCoordMs += wallMs;
        totalComputeMs += roundCompute;
        totalRequests += roundRequests;
        totalTokens += roundTokens;
      }

      const avgWall = Math.round(totalCoordMs / benchmarkRounds);
      const avgCompute = Math.round(totalComputeMs / benchmarkRounds);
      const avgCoord = Math.max(avgWall - Math.round(avgCompute / shardCount), 1);
      const avgRequests = Math.round(totalRequests / benchmarkRounds);
      const avgTokens = Math.round(totalTokens / benchmarkRounds);
      const reqPerSec = avgWall > 0 ? Math.round((avgRequests * 1000) / avgWall) : 0;
      const tokPerSec = avgWall > 0 ? Math.round((avgTokens * 1000) / avgWall) : 0;
      const granularity = avgCoord > 0 ? Math.round((Math.round(avgCompute / shardCount) / avgCoord) * 10) / 10 : 0;

      if (baselineThroughput === 0) baselineThroughput = reqPerSec;
      const efficiency = baselineThroughput > 0 ? (reqPerSec / baselineThroughput) * 100 : 100;

      results.push({
        shards: shardCount,
        total_requests: avgRequests,
        requests_per_sec: reqPerSec,
        tokens_per_sec: tokPerSec,
        wall_time_ms: avgWall,
        compute_time_ms: avgCompute,
        coordination_time_ms: avgCoord,
        granularity_ratio: granularity,
        efficiency_pct: Math.round(efficiency * 10) / 10,
        error_count: errorCount,
      });
    }

    return { status: "ok", requests_per_shard: requestsPerShard, num_passes: numPasses, results };
  }

  onGetHealth(_payload: Record<string, unknown>): Record<string, unknown> {
    return { status: "ok", message: "Use run to get tier-level results" };
  }
}

// ─── Model Actor (Worker) ───────────────────────────────────────────────────

class ModelActor extends PlexSpacesActor<ModelState> {
  private rng = 42;

  getDefaultState(): ModelState {
    return {
      actor_id: "", application_id: "", role: "worker",
      model_tier: "medium",
      requests_served: 0, tokens_generated: 0, failures: 0, compute_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    const args = recordValue(config.args);
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = "worker";
    this.state.model_tier = stringValue(args.model_tier) || "medium";
    this.rng = host.nowMs() % 1000000;
  }

  onProcess_batch(payload: Record<string, unknown>): Record<string, unknown> {
    const compStart = host.nowMs();
    const rawRequests = anyArray(payload.requests);
    const tierName = stringValue(payload.model_tier) || this.state.model_tier;
    const tierConfig = MODEL_TIERS[tierName] ?? MODEL_TIERS["medium"]!;

    const batchResults: Record<string, unknown>[] = [];

    for (const raw of rawRequests) {
      const r = recordValue(raw);
      const request: InferenceRequest = {
        request_id: stringValue(r.request_id),
        prompt: stringValue(r.prompt),
        task_type: stringValue(r.task_type),
        max_tokens: intValue(r.max_tokens, 128),
        priority: stringValue(r.priority),
        user_id: stringValue(r.user_id),
      };

      const { result, rng: newRng } = simulateInference(request, tierConfig, this.rng);
      this.rng = newRng;

      if (result.status === "ok") {
        this.state.requests_served++;
        this.state.tokens_generated += result.tokens_generated;
      } else {
        this.state.failures++;
      }

      batchResults.push(result as unknown as Record<string, unknown>);
    }

    const computeMs = host.nowMs() - compStart;
    this.state.compute_ms += computeMs;

    try { host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.compute": computeMs, "worker.requests": rawRequests.length } }); } catch (_e) {}

    return {
      batch_results: batchResults,
      compute_ms: computeMs,
      requests_processed: rawRequests.length,
      model_tier: tierName,
    };
  }

  // Worker-local benchmark: generates requests locally, runs num_passes of inference.
  // Leader sends one broadcast SG — enables true weak scaling with one round trip.
  onBenchmark_requests(payload: Record<string, unknown>): Record<string, unknown> {
    const requestsPerShard = intValue(payload.requests_per_shard, 200);
    const numPasses = intValue(payload.num_passes, 4);
    const seed = intValue(payload.seed, 42);

    const compStart = host.nowMs();
    const requests = generateRequests(requestsPerShard, seed);
    let totalTokens = 0;
    let totalRequests = 0;
    let failures = 0;

    for (let p = 0; p < numPasses; p++) {
      let rng = seed + p * 7919;
      for (const req of requests) {
        const tier = selectTier(req.task_type, req.priority);
        const tierConfig = MODEL_TIERS[tier] ?? MODEL_TIERS["medium"]!;
        const { result, rng: newRng } = simulateInference(req, tierConfig, rng);
        rng = newRng;
        if (result.status === "ok") {
          totalTokens += result.tokens_generated;
          totalRequests++;
        } else {
          failures++;
        }
      }
    }

    const computeMs = host.nowMs() - compStart;
    try { host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.bench_compute": computeMs } }); } catch (_e) {}

    return {
      requests_processed: totalRequests,
      tokens_generated: totalTokens,
      failures,
      passes: numPasses,
      compute_ms: computeMs,
    };
  }

  onGetHealth(_payload: Record<string, unknown>): Record<string, unknown> {
    const totalReqs = this.state.requests_served + this.state.failures;
    return {
      tier: this.state.model_tier,
      healthy: this.state.failures < totalReqs * 0.1,
      requests_served: this.state.requests_served,
      tokens_generated: this.state.tokens_generated,
      failures: this.state.failures,
      avg_latency_ms: totalReqs > 0 ? Math.round(this.state.compute_ms / totalReqs) : 0,
      error_rate_pct: totalReqs > 0 ? Math.round((this.state.failures / totalReqs) * 10000) / 100 : 0,
    };
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function actorApplicationId(actorId: string): string {
  const doubleSlash = actorId.indexOf("//");
  if (doubleSlash < 0) return "";
  const rest = actorId.slice(doubleSlash + 2);
  const colonColon = rest.indexOf("::");
  if (colonColon < 0) return "";
  const afterColon = rest.slice(colonColon + 2);
  const atSign = afterColon.indexOf("@");
  return atSign >= 0 ? afterColon.slice(0, atSign) : afterColon;
}

function actorNodeId(actorId: string): string {
  const atSign = actorId.lastIndexOf("@");
  return atSign >= 0 ? actorId.slice(atSign + 1) : "";
}

function intValue(value: unknown, fallback: number): number {
  if (typeof value === "number" && Number.isFinite(value)) return Math.trunc(value);
  if (typeof value === "string") {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}

function floatValue(value: unknown, fallback: number): number {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}

function intArrayValue(value: unknown, fallback: number[]): number[] {
  if (!Array.isArray(value)) return fallback;
  return value.map((v) => intValue(v, 0)).filter((v) => v > 0);
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function recordValue(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function anyArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function stringArray(value: unknown): string[] {
  return anyArray(value).map((item) => String(item)).filter((item) => item.length > 0);
}

function computeActorCounts(leaderNodeId: string, shardActorIds: string[]): Record<string, Record<string, number>> {
  const nodes: Record<string, Record<string, number>> = {
    [leaderNodeId]: { actors: 1, leader_actors: 1, worker_actors: 0 },
  };
  for (const actorId of shardActorIds) {
    const nodeId = actorNodeId(actorId);
    if (!nodes[nodeId]) {
      nodes[nodeId] = { actors: 0, leader_actors: 0, worker_actors: 0 };
    }
    nodes[nodeId]!.actors += 1;
    nodes[nodeId]!.worker_actors += 1;
  }
  return nodes;
}

// ─── Router & Export ─────────────────────────────────────────────────────────

const router = new ActorRouter({
  leader: () => new RouterActor(),
  worker: () => new ModelActor(),
  "llm-leader": () => new RouterActor(),
  "llm-worker": () => new ModelActor(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.setState(stateJson),
};
