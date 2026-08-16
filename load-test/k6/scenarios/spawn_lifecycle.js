// SPDX-License-Identifier: AGPL-3.0-or-later
// spawn_lifecycle.js — Measure actor spawn/lifecycle latency.
//
// ACTOR_TYPE=regular (default):
//   Each VU owns one persistent actor (lifecycle-regular-vu<N>). The actor activates
//   on the first ask and stays alive for the whole VU lifetime. Measures warm-call
//   throughput after the initial activation. Applies to all languages.
//
// ACTOR_TYPE=virtual:
//   Each VU cycles through a bounded pool of VIRTUAL_POOL_SIZE actor names
//   (lifecycle-virt-<VU>-<slot % VIRTUAL_POOL_SIZE>). Each step forces a unique slot
//   so every ask triggers a cold activation when that slot was recently evicted.
//   Pool size defaults to MAX_WASM_INSTANCES * 2 (2× cap = every ask evicts one actor,
//   giving a steady cold-activation rate without an infinite unique-name storm).
//   VIRTUAL_POOL_SIZE env var overrides this.
//
// Run:
//   k6 run -e ACTOR_TYPE=regular -e LANG=rust-embedded -e DURATION=30s \
//          load-test/k6/scenarios/spawn_lifecycle.js

import http from "k6/http";
import { check } from "k6";
import exec from "k6/execution";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, errorRate, requestCounter } from "../common.js";

const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const DURATION   = __ENV.DURATION   || "";
const ACTOR_TYPE = __ENV.ACTOR_TYPE || "regular";
const LANG       = __ENV.LANG || "rust-embedded";
// For virtual mode: number of actor name slots to cycle through per VU.
// Must be > MAX_WASM_INSTANCES so each ask evicts an old actor and tests cold activation.
// Default: 2 × MAX_WASM_INSTANCES (or 10 for rust-embedded which has no pool cap).
const MAX_WASM_INSTANCES = parseInt(__ENV.MAX_WASM_INSTANCES || "3");
const DEFAULT_VIRTUAL_POOL = (LANG === "rust-embedded") ? 10 : (MAX_WASM_INSTANCES * 2 + 1);
const VIRTUAL_POOL_SIZE = parseInt(__ENV.VIRTUAL_POOL_SIZE || String(DEFAULT_VIRTUAL_POOL));

// Ask timeout: embedded is fast; Python WASM can take up to 30s for cold activation.
// Override via ASK_TIMEOUT env so run.sh can pass the right value per language.
const DEFAULT_ASK_TIMEOUT = (ACTOR_TYPE === "virtual")
  ? (LANG === "python" ? 30 : LANG === "typescript" ? 20 : 15)
  : 10;
const ASK_TIMEOUT = parseInt(__ENV.ASK_TIMEOUT || String(DEFAULT_ASK_TIMEOUT));
const appId     = (LANG === "rust-embedded") ? "perf-embedded" : (APP_IDS[LANG] || `perf-${LANG}`);
const serverUrl = (LANG === "rust-embedded") ? EMBEDDED_URL : BASE_URL;
const actorType = (LANG === "rust-embedded") ? "gen_server" : "PerfActor";

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
    "perf_spawn_ms":       ["p(99)<60000"],
    "perf_warm_call_ms":   ["p(99)<10000"],
    "perf_error_rate":     ["rate<0.05"],
  },
};

// perf_spawn_ms: first-ask latency (includes activation for virtual; near-zero for warm regular)
const spawnLatency    = new Trend("perf_spawn_ms",     true);
// perf_warm_call_ms: subsequent warm ask latency (only meaningful for regular)
const warmCallLatency = new Trend("perf_warm_call_ms", true);

// Track per-VU activation state for regular mode.
let vuActivated = false;

export default function () {
  const durSecs  = parseDurationSecs(DURATION);
  const remaining = DURATION ? durSecs * (1 - exec.scenario.progress) : Infinity;
  // Skip the last 5s of the test window so in-flight requests don't race the server timeout.
  // Never skip based on ASK_TIMEOUT — for a 30s test with a 30s timeout that kills the whole run.
  if (remaining <= 5) return;

  const httpTimeout = DURATION
    ? `${Math.max(1000, Math.min((ASK_TIMEOUT + 1) * 1000, (remaining - 1) * 1000))}ms`
    : `${ASK_TIMEOUT + 1}s`;

  if (ACTOR_TYPE === "virtual") {
    // ── Virtual mode: rotating pool of actor names ───────────────────────────
    // Cycles through VIRTUAL_POOL_SIZE slots. Each slot is asked in turn so actors
    // older than VIRTUAL_POOL_SIZE iterations are always evicted by LRU, giving a
    // steady cold-activation rate without spawning infinitely unique actors.
    const slot = __ITER % VIRTUAL_POOL_SIZE;
    const actorName = `lifecycle-virt-${__VU}-${slot}`;
    const t0 = Date.now();
    const res = http.post(
      `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=${ASK_TIMEOUT}`,
      JSON.stringify({ op: "echo", payload: "init" }),
      { headers: authHeaders(), tags: { op: "spawn_virtual" }, timeout: httpTimeout }
    );
    spawnLatency.add(Date.now() - t0, { phase: "cold_activation" });
    requestCounter.add(1, { op: "spawn_virtual" });

    const ok = check(res, {
      "virtual activate 200": (r) => r.status === 200,
      "success": (r) => { try { const b = JSON.parse(r.body); return b.success === true || b.ok === true; } catch (_) { return false; } },
    });
    errorRate.add(!ok);
    if (!ok && __ITER <= 5) {
      // Log first few failures so server error message is visible in k6 output.
      console.error(`[spawn_lifecycle] VU=${__VU} ITER=${__ITER} status=${res.status} body=${res.body ? res.body.slice(0,300) : "(empty)"}`);
    }

    // Do NOT stop virtual actors after each iteration.
    // The server's LRU eviction handles cleanup (pool cap set via PERF_MAX_VIRTUAL_POOL; default 0 = unlimited).
    // Sending DELETE blocks for ~100ms (server stop_actor sleep) per actor and also risks
    // a race where the activation isn't fully committed before the stop arrives, causing
    // the next activation to stall indefinitely.  Letting actors age out is correct here.

  } else {
    // ── Regular mode: persistent actor per VU ────────────────────────────────
    // Actor activates on first ask, stays alive for all subsequent iterations.
    // First-ask (activation) latency is tracked separately from warm calls.
    const actorName = `lifecycle-regular-vu${__VU}`;

    if (!vuActivated) {
      // First ask: activation (cold start)
      const t0 = Date.now();
      const res = http.post(
        `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=${ASK_TIMEOUT}`,
        JSON.stringify({ op: "echo", payload: "init" }),
        { headers: authHeaders(), tags: { op: "spawn_regular" }, timeout: httpTimeout }
      );
      spawnLatency.add(Date.now() - t0, { phase: "activation" });
      requestCounter.add(1, { op: "spawn_regular" });

      const ok = check(res, {
        "regular activate 200": (r) => r.status === 200,
        "success": (r) => { try { const b = JSON.parse(r.body); return b.success === true || b.ok === true; } catch (_) { return false; } },
      });
      errorRate.add(!ok);
      if (!ok) {
        console.error(`[spawn_lifecycle] regular VU=${__VU} ITER=${__ITER} status=${res.status} body=${res.body ? res.body.slice(0,300) : "(empty)"}`);
      }
      if (ok) vuActivated = true;
    } else {
      // Warm call: actor already active
      const t0 = Date.now();
      const res = http.post(
        `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=10`,
        JSON.stringify({ op: "echo", payload: "warm" }),
        { headers: authHeaders(), tags: { op: "call_warm" }, timeout: httpTimeout }
      );
      warmCallLatency.add(Date.now() - t0, { phase: "warm" });
      requestCounter.add(1, { op: "call_warm" });

      const ok = check(res, { "warm call 200": (r) => r.status === 200 });
      errorRate.add(!ok);
    }
  }
}
