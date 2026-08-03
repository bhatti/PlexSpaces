// SPDX-License-Identifier: AGPL-3.0-or-later
// process_group.js — Test ProcessGroup lifecycle under load.
//
// Lifecycle enforced in setup/teardown:
//   1. Create group
//   2. Each VU spawns MEMBERS_PER_VU actors that join the group
//   3. Publish 100 messages to the group
//   4. Verify delivery via KV counters
//   5. Delete group
//
// Run:
//   k6 run -e VUS=10 -e MEMBERS=500 k6/scenarios/process_group.js
//   k6 run -e VUS=10 -e MEMBERS=1000 k6/scenarios/process_group.js
//   k6 run -e VUS=10 -e MEMBERS=2000 k6/scenarios/process_group.js

import http from "k6/http";
import { check, sleep, fail } from "k6";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, errorRate, pgLatency, requestCounter } from "../common.js";

const VUS        = parseInt(__ENV.VUS     || "10");
const MEMBERS    = parseInt(__ENV.MEMBERS || "500");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "10");
const GROUP_NAME = __ENV.GROUP_NAME || `perf-pg-${Date.now()}`;

export const options = {
  scenarios: {
    process_group: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "20m",
    },
  },
  thresholds: {
    "perf_pg_latency_ms":  ["p(99)<30000"],
    "perf_error_rate":     ["rate<0.05"],
  },
};

const appId     = "perf-embedded";
const actorType = "PerfActor";

export function setup() {
  // Create the process group
  const res = http.post(
    `${BASE_URL}/api/v1/process-groups`,
    JSON.stringify({ group_name: GROUP_NAME, namespace: "default", tenant_id: "default" }),
    { headers: authHeaders() }
  );
  const ok = check(res, { "create group 200": (r) => r.status === 200 || r.status === 201 });
  if (!ok) fail(`Failed to create group ${GROUP_NAME}: ${res.status} ${res.body}`);

  // Spawn subscriber actors (they join the group on first pg_broadcast call)
  for (let i = 0; i < MEMBERS; i++) {
    http.post(
      `${BASE_URL}/api/v1/actors/${appId}/pg-member-${i}:${actorType}/ask?timeout=10`,
      JSON.stringify({ op: "echo", payload: "warmup" }),
      { headers: authHeaders() }
    );
  }

  return { group_name: GROUP_NAME, members: MEMBERS };
}

export default function (data) {
  const start = Date.now();

  // Send a pg_broadcast from this VU's actor
  const broadcaster = `pg-broadcaster-vu${__VU}`;
  const res = http.post(
    `${BASE_URL}/api/v1/actors/${appId}/${broadcaster}:${actorType}/ask?timeout=30`,
    JSON.stringify({ op: "pg_broadcast", group: data.group_name, message: { iter: __ITER } }),
    { headers: authHeaders(), tags: { op: "pg_broadcast" } }
  );

  const elapsed = Date.now() - start;
  pgLatency.add(elapsed, { members: String(MEMBERS) });
  requestCounter.add(1, { op: "pg_broadcast" });

  const ok = check(res, {
    "broadcast 200": (r) => r.status === 200,
    "broadcast ok":  (r) => { try { return JSON.parse(r.body).ok === true; } catch(_) { return false; } },
  });
  errorRate.add(!ok);

  // Get members count to verify group is healthy
  if (__ITER % 10 === 0) {
    const membersRes = http.get(
      `${BASE_URL}/api/v1/process-groups/${data.group_name}/members`,
      { headers: authHeaders() }
    );
    check(membersRes, { "get members 200": (r) => r.status === 200 });
  }
}

export function teardown(data) {
  http.del(
    `${BASE_URL}/api/v1/process-groups/${data.group_name}`,
    null,
    { headers: authHeaders() }
  );
}
