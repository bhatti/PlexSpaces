// SPDX-License-Identifier: AGPL-3.0-or-later
// shard_group.js — Test ShardGroup HPC workload at scale.
//
// Lifecycle (enforced via setup/teardown):
//   1. Create ShardGroup with N shards
//   2. Poll until ACTIVE
//   3. VUs run: scatter-gather → all-reduce → barrier (10 iterations each)
//   4. Delete ShardGroup
//
// The shard_task operation runs Mersenne prime / gradient descent to give
// real compute time — this is what you would see in a real HPC workload.
//
// Run:
//   k6 run -e SHARDS=500  -e VUS=10 -e ITERATIONS=10 k6/scenarios/shard_group.js
//   k6 run -e SHARDS=1000 -e VUS=10 -e ITERATIONS=10 k6/scenarios/shard_group.js
//   k6 run -e SHARDS=2000 -e VUS=10 -e ITERATIONS=10 k6/scenarios/shard_group.js

import http from "k6/http";
import { check, sleep, fail } from "k6";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, errorRate, shardLatency, requestCounter, makeShardValues } from "../common.js";

const SHARDS     = parseInt(__ENV.SHARDS     || "500");
const VUS        = parseInt(__ENV.VUS        || "10");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "10");
const SHARD_APP  = "perf-rust-wasm";   // shard workers are Rust WASM actors
const GROUP_ID   = __ENV.GROUP_ID || `perf-sg-${Date.now()}`;

const scatterLatency = new Trend("perf_scatter_ms",   true);
const reduceLatency  = new Trend("perf_reduce_ms",    true);
const barrierLatency = new Trend("perf_barrier_ms",   true);

export const options = {
  scenarios: {
    shard_group: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "30m",
    },
  },
  thresholds: {
    "perf_scatter_ms":  ["p(99)<60000"],
    "perf_reduce_ms":   ["p(99)<30000"],
    "perf_error_rate":  ["rate<0.05"],
  },
};

export function setup() {
  // Create the shard group
  const createBody = JSON.stringify({
    group_id: GROUP_ID,
    actor_type: "PerfActor",
    namespace: SHARD_APP,
    tenant_id: "default",
    config: {
      shard_count: SHARDS,
      partition_strategy: "HASH",
      placement: "SAME_NODE",
      aggregation_strategy: "MERGE",
    },
  });

  const res = http.post(
    `${BASE_URL}/api/v1/actors/shard-groups`,
    createBody,
    { headers: authHeaders() }
  );
  const ok = check(res, { "create shard group 200": (r) => r.status === 200 || r.status === 201 });
  if (!ok) fail(`Failed to create shard group: ${res.status} ${res.body}`);

  // Poll until ACTIVE (max 60s)
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const statusRes = http.get(
      `${BASE_URL}/api/v1/actors/shard-groups/${GROUP_ID}`,
      { headers: authHeaders() }
    );
    if (statusRes.status === 200) {
      try {
        const body = JSON.parse(statusRes.body);
        if (body.state === "ACTIVE" || body.shard_group?.state === 2) break;
      } catch (_) {}
    }
    sleep(1);
  }

  return { group_id: GROUP_ID, shards: SHARDS };
}

export default function (data) {
  const values = makeShardValues(100, __VU);

  // Scatter-gather: send compute to all shards, collect results
  const sgStart = Date.now();
  const sgRes = http.post(
    `${BASE_URL}/api/v1/actors/shard-groups/${data.group_id}:scatterGather`,
    JSON.stringify({
      payload: JSON.stringify({ op: "shard_task", values, lr: 0.01 }),
      timeout_ms: 30000,
    }),
    { headers: authHeaders(), tags: { op: "scatter_gather", shards: String(SHARDS) } }
  );
  const sgElapsed = Date.now() - sgStart;
  scatterLatency.add(sgElapsed, { shards: String(SHARDS) });
  requestCounter.add(1, { op: "scatter_gather" });

  const sgOk = check(sgRes, {
    "scatter-gather 200": (r) => r.status === 200,
    "scatter-gather ok":  (r) => { try { const b = JSON.parse(r.body); return b.ok !== false; } catch(_) { return false; } },
  });
  errorRate.add(!sgOk);

  // All-reduce: combine partial gradients across all shards
  const arStart = Date.now();
  const arRes = http.post(
    `${BASE_URL}/api/v1/actors/shard-groups/${data.group_id}:allReduce`,
    JSON.stringify({
      payload: JSON.stringify({ gradient: sgOk ? 0.5 : 0.0 }),
      reduce_op: "SUM",
      timeout_ms: 30000,
    }),
    { headers: authHeaders(), tags: { op: "all_reduce", shards: String(SHARDS) } }
  );
  const arElapsed = Date.now() - arStart;
  reduceLatency.add(arElapsed, { shards: String(SHARDS) });
  requestCounter.add(1, { op: "all_reduce" });

  check(arRes, { "all-reduce 200": (r) => r.status === 200 });

  // Barrier: synchronize (only needed every few iterations)
  if (__ITER % 5 === 0) {
    const bStart = Date.now();
    const bRes = http.post(
      `${BASE_URL}/api/v1/actors/shard-groups/${data.group_id}:barrier`,
      JSON.stringify({ timeout_ms: 30000 }),
      { headers: authHeaders(), tags: { op: "barrier" } }
    );
    barrierLatency.add(Date.now() - bStart, { shards: String(SHARDS) });
    check(bRes, { "barrier 200": (r) => r.status === 200 });
  }

  shardLatency.add(sgElapsed + arElapsed, { shards: String(SHARDS) });
}

export function teardown(data) {
  http.del(
    `${BASE_URL}/api/v1/actors/shard-groups/${data.group_id}`,
    null,
    { headers: authHeaders() }
  );
}
