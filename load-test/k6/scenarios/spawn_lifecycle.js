// SPDX-License-Identifier: AGPL-3.0-or-later
// spawn_lifecycle.js — Measure actor spawn → call → stop latency.
//
// Tests the full lifecycle cost per actor. Run against rust-embedded (baseline).
// Expected result: spawn < 5ms, call < 10ms, stop < 5ms.
//
// Run:
//   k6 run -e VUS=100 -e ITERATIONS=100 k6/scenarios/spawn_lifecycle.js

import http from "k6/http";
import { check } from "k6";
import exec from "k6/execution";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, errorRate, requestCounter } from "../common.js";

const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const DURATION   = __ENV.DURATION   || "";
const ASK_TIMEOUT = 10;  // lifecycle uses a 10s ask timeout (virtual actor spawn)

function parseDurationSecs(d) {
  if (!d) return Infinity;
  let s = 0;
  const m = d.match(/(\d+)m/); if (m) s += parseInt(m[1]) * 60;
  const sec = d.match(/(\d+)s/); if (sec) s += parseInt(sec[1]);
  return s || Infinity;
}

function buildScenario() {
  if (DURATION) {
    return {
      spawn_lifecycle: {
        executor: "constant-vus",
        vus: VUS,
        duration: DURATION,
        gracefulStop: "0s",
      },
    };
  }
  return {
    spawn_lifecycle: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "15m",
    },
  };
}

export const options = {
  scenarios: buildScenario(),
  thresholds: {
    "perf_spawn_ms":    ["p(99)<10000"],
    "perf_error_rate":  ["rate<0.05"],
  },
};

const spawnLatency = new Trend("perf_spawn_ms", true);
// For embedded actors, use port 8092; for WASM actors use main server (8091).
const LANG    = __ENV.LANG || "rust-embedded";
const appId   = (LANG === "rust-embedded") ? "perf-embedded" : (APP_IDS[LANG] || `perf-${LANG}`);
const serverUrl = (LANG === "rust-embedded") ? EMBEDDED_URL : BASE_URL;
// Embedded uses gen_server slug; WASM actors use PerfActor as declared in app-config.toml
const actorType = (LANG === "rust-embedded") ? "gen_server" : "PerfActor";

export default function () {
  let httpTimeout = `${ASK_TIMEOUT + 1}s`;
  if (DURATION) {
    const durSecs = parseDurationSecs(DURATION);
    const remaining = durSecs * (1 - exec.scenario.progress);
    // Skip the last (ASK_TIMEOUT + 5)s to prevent in-flight requests racing the server ask timeout.
    if (remaining <= ASK_TIMEOUT + 5) {
      return;
    }
    const clampedMs = Math.max(500, Math.min((ASK_TIMEOUT + 1) * 1000, (remaining - 0.5) * 1000));
    httpTimeout = `${clampedMs}ms`;
  }
  // Each VU reuses a single actor name — stopped at the end of every iteration.
  // This keeps live actor count bounded at VUS (not VUS × ITER) regardless of run length.
  const actorName = `lifecycle-vu${__VU}`;

  // First ask — auto-activates (virtual actor on-demand spawn)
  const spawnStart = Date.now();
  const spawnRes = http.post(
    `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=10`,
    JSON.stringify({ op: "echo", payload: "init" }),
    { headers: authHeaders(), tags: { op: "spawn" }, timeout: httpTimeout }
  );
  const spawnElapsed = Date.now() - spawnStart;
  spawnLatency.add(spawnElapsed, { phase: "first_ask" });
  requestCounter.add(1, { op: "spawn" });

  const spawnOk = check(spawnRes, {
    "first ask 200": (r) => r.status === 200,
    "success": (r) => { try { const b = JSON.parse(r.body); return b.success === true || b.ok === true; } catch(_) { return false; } },
  });
  errorRate.add(!spawnOk);

  // Second ask — actor already active (warm)
  const callRes = http.post(
    `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=10`,
    JSON.stringify({ op: "get_stats" }),
    { headers: authHeaders(), tags: { op: "call" }, timeout: httpTimeout }
  );
  const callOk = check(callRes, { "get_stats 200": (r) => r.status === 200 });
  errorRate.add(!callOk);
  requestCounter.add(1, { op: "call" });

  // Stop the actor — DELETE so the next iteration measures a fresh activation.
  // Without this, actors accumulate (VUS × ITER live) and saturate the mailbox queue.
  http.del(
    `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}`,
    null,
    { headers: authHeaders(), tags: { op: "stop" }, timeout: httpTimeout }
  );
  requestCounter.add(1, { op: "stop" });
}
