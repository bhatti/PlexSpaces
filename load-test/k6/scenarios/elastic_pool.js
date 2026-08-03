// SPDX-License-Identifier: AGPL-3.0-or-later
// elastic_pool.js — Test ElasticPool checkout/checkin throughput.
//
// Lifecycle:
//   1. Create pool (min=10, max=2000, scale_up_threshold=0.8)
//   2. VUs concurrently: checkout → call compute → checkin
//   3. Watch pool auto-scale under load
//   4. Delete pool
//
// Run:
//   k6 run -e VUS=100 -e ITERATIONS=100 k6/scenarios/elastic_pool.js

import http from "k6/http";
import { check, sleep, fail } from "k6";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, errorRate, requestCounter } from "../common.js";

const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const POOL_NAME  = __ENV.POOL_NAME || `perf-pool-${Date.now()}`;
const POOL_MIN   = parseInt(__ENV.POOL_MIN   || "10");
const POOL_MAX   = parseInt(__ENV.POOL_MAX   || "2000");

const checkoutLatency = new Trend("perf_pool_checkout_ms", true);
const computeLatency  = new Trend("perf_pool_compute_ms",  true);
const checkinLatency  = new Trend("perf_pool_checkin_ms",  true);

export const options = {
  scenarios: {
    elastic_pool: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "20m",
    },
  },
  thresholds: {
    "perf_pool_checkout_ms": ["p(99)<10000"],
    "perf_pool_compute_ms":  ["p(99)<5000"],
    "perf_error_rate":       ["rate<0.05"],
  },
};

export function setup() {
  const res = http.post(
    `${BASE_URL}/api/v1/pools`,
    JSON.stringify({
      pool_name: POOL_NAME,
      actor_type: "PerfActor",
      namespace: "perf-embedded",
      tenant_id: "default",
      config: {
        min_size: POOL_MIN,
        max_size: POOL_MAX,
        initial_size: POOL_MIN,
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.3,
        idle_timeout_seconds: 300,
        checkout_timeout_ms: 5000,
      },
    }),
    { headers: authHeaders() }
  );
  const ok = check(res, { "create pool 200": (r) => r.status === 200 || r.status === 201 });
  if (!ok) fail(`Failed to create pool: ${res.status} ${res.body}`);
  sleep(2); // give pool time to initialize minimum actors
  return { pool_name: POOL_NAME };
}

export default function (data) {
  // Checkout
  const coStart = Date.now();
  const coRes = http.post(
    `${BASE_URL}/api/v1/pools/${data.pool_name}:checkout`,
    JSON.stringify({ timeout_ms: 5000 }),
    { headers: authHeaders(), tags: { op: "checkout" } }
  );
  checkoutLatency.add(Date.now() - coStart);
  requestCounter.add(1, { op: "checkout" });

  const coOk = check(coRes, {
    "checkout 200": (r) => r.status === 200,
    "has actor_id": (r) => { try { return !!JSON.parse(r.body).actor_id; } catch(_) { return false; } },
  });
  errorRate.add(!coOk);
  if (!coOk) return;

  let checkoutId, actorId;
  try {
    const body = JSON.parse(coRes.body);
    checkoutId = body.checkout_id;
    actorId    = body.actor_id;
  } catch (_) {
    errorRate.add(1);
    return;
  }

  // Compute — call the checked-out actor directly
  const compStart = Date.now();
  const compRes = http.post(
    `${BASE_URL}/api/v1/actors/perf-embedded/${actorId}:PerfActor/ask?timeout=10`,
    JSON.stringify({ op: "compute", p: 7 }),
    { headers: authHeaders(), tags: { op: "compute" } }
  );
  computeLatency.add(Date.now() - compStart);
  requestCounter.add(1, { op: "compute" });
  check(compRes, { "compute 200": (r) => r.status === 200 });

  // Checkin
  const ciStart = Date.now();
  const ciRes = http.post(
    `${BASE_URL}/api/v1/pools/${data.pool_name}:checkin`,
    JSON.stringify({ actor_id: actorId, checkout_id: checkoutId, healthy: true }),
    { headers: authHeaders(), tags: { op: "checkin" } }
  );
  checkinLatency.add(Date.now() - ciStart);
  requestCounter.add(1, { op: "checkin" });
  check(ciRes, { "checkin 200": (r) => r.status === 200 });
}

export function teardown(data) {
  // Drain first, then delete
  http.post(`${BASE_URL}/api/v1/pools/${data.pool_name}:drain`, JSON.stringify({ timeout_ms: 10000 }), { headers: authHeaders() });
  http.del(`${BASE_URL}/api/v1/pools/${data.pool_name}`, null, { headers: authHeaders() });
}
