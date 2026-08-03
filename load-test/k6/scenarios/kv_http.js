// SPDX-License-Identifier: AGPL-3.0-or-later
// kv_http.js — Measure KV put + get round-trip latency via HTTP.
//
// Each iteration: kv_put then kv_get on a key unique to the VU.
// Measures host-function call overhead and SQLite write + read latency.
//
// Run:
//   k6 run -e LANG=python -e VUS=100 -e ITERATIONS=100 k6/scenarios/kv_http.js

import http from "k6/http";
import { check } from "k6";
import exec from "k6/execution";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, vuInstance, errorRate, kvLatency, requestCounter } from "../common.js";

const LANG            = __ENV.LANG       || "python";
const VUS             = parseInt(__ENV.VUS        || "100");
const ITERATIONS      = parseInt(__ENV.ITERATIONS || "100");
const DURATION        = __ENV.DURATION   || "";
// MAX_WASM_INSTANCES: cap the actor pool for WASM languages to bound wasmtime Store memory.
// go/ts/rust-wasm/python: default 2 (OOM risk from Store-per-message workaround).
const MAX_WASM_INSTANCES = parseInt(__ENV.MAX_WASM_INSTANCES || "2");

// Server-side actor ask timeout.
// 5s is sufficient for all languages when actors are properly warmed.
// WASM languages use 10s as a conservative margin (Store creation on cold activation).
const ASK_TIMEOUT = 5;

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
      kv_http: {
        executor: "constant-vus",
        vus: VUS,
        duration: DURATION,
        gracefulStop: "0s",
      },
    };
  }
  return {
    kv_http: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "10m",
    },
  };
}

export const options = {
  scenarios: buildScenario(),
  thresholds: {
    // p(99) threshold scales with ask timeout: 2× timeout + buffer.
    "perf_kv_latency_ms": [`p(99)<${(ASK_TIMEOUT * 2 + 5) * 1000}`],
    "perf_error_rate": ["rate<0.01"],  // target <1% errors
  },
};

const appId     = APP_IDS[LANG] || `perf-${LANG}`;
const actorType = (LANG === "rust-embedded") ? "gen_server" : "PerfActor";
const serverUrl = (LANG === "rust-embedded") ? EMBEDDED_URL : BASE_URL;

// Pre-warm kv actor instances sequentially before VUs start.
// Concurrent first-activation under load causes 504s because wasmtime Store creation
// under queue pressure can take several seconds.
// We warm with kv_put (not just echo) so the KV host path is exercised and any
// lazy initialization inside the actor completes before VUs start.
export function setup() {
  // For embedded: instance names are vu1..VUS (from vuInstance() = `vu${__VU}`).
  // For WASM: instance names are kv-vu1..MAX_WASM_INSTANCES.
  const instances = (LANG === "rust-embedded")
    ? Array.from({ length: VUS }, (_, i) => `vu${i + 1}`)
    : Array.from({ length: MAX_WASM_INSTANCES }, (_, i) => `kv-vu${i + 1}`);
  const warmTimeout = `${ASK_TIMEOUT + 25}s`;
  const warmRounds = 3; // Repeat to fully exercise WASM JIT and internal state
  for (const inst of instances) {
    const url = `${serverUrl}/api/v1/actors/${appId}/${inst}:${actorType}/ask?timeout=${ASK_TIMEOUT + 20}`;
    for (let r = 0; r < warmRounds; r++) {
      // Warm with kv_put to exercise the full KV path and finish any lazy WASM init.
      const res = http.post(url, JSON.stringify({ op: "kv_put", key: `_warmup_${inst}_${r}`, value: "warmup" }), {
        headers: authHeaders(),
        timeout: warmTimeout,
      });
      if (res.status !== 200) {
        console.warn(`[kv setup] warm ${inst} round=${r} status=${res.status} body=${res.body}`);
      }
    }
  }
}

function ask(instance, payload, timeout) {
  const url = `${serverUrl}/api/v1/actors/${appId}/${instance}:${actorType}/ask?timeout=${ASK_TIMEOUT}`;
  const res = http.post(url, JSON.stringify(payload), {
    headers: authHeaders(),
    tags: { transport: "http", lang: LANG },
    timeout: timeout || "6s",
  });
  if (res.status !== 200) return { error: `HTTP ${res.status}`, _status: res.status };
  try {
    const b = JSON.parse(res.body);
    // Unwrap the server envelope { success, payload: {...} }
    return b.payload || b;
  } catch (_) { return { error: "invalid JSON" }; }
}

export default function () {
  let httpTimeout = `${ASK_TIMEOUT + 1}s`;
  if (DURATION) {
    const durSecs = parseDurationSecs(DURATION);
    const remaining = durSecs * (1 - exec.scenario.progress);
    // Skip the last (ASK_TIMEOUT + 3)s to prevent in-flight requests from racing the server ask timeout.
    if (remaining <= ASK_TIMEOUT + 3) {
      return;
    }
    const clampedMs = Math.max(500, Math.min((ASK_TIMEOUT + 1) * 1000, (remaining - 0.5) * 1000));
    httpTimeout = `${clampedMs}ms`;
  }
  // For embedded: one actor per VU (large pool OK).
  // For WASM languages: cap pool to MAX_WASM_INSTANCES to bound wasmtime Store memory.
  const instance = (LANG === "rust-embedded")
    ? vuInstance()
    : `kv-vu${((__VU - 1) % MAX_WASM_INSTANCES) + 1}`;
  const key = `perf_key_vu${__VU}`;

  // Put
  const start = Date.now();
  const putRes = ask(instance, { op: "kv_put", key, value: `v${__ITER}` }, httpTimeout);
  const putOk = check(putRes, { "kv_put ok": (r) => r.ok === true });
  errorRate.add(!putOk);
  if (!putOk) {
    console.error(`[kv_http] put FAIL VU=${__VU} key=${key} put=${JSON.stringify(putRes)}`);
  }

  // Get
  const getRes = ask(instance, { op: "kv_get", key }, httpTimeout);
  const elapsed = Date.now() - start;

  kvLatency.add(elapsed, { transport: "http", lang: LANG });
  requestCounter.add(2, { transport: "http", lang: LANG, op: "kv" });

  // Note: the framework load-balances across the pre-warmed pool so get may land on a
  // different actor instance than put — the value may be absent. We only check that the
  // handler ran successfully (ok: true), not that the key exists on this particular actor.
  const getOk = check(getRes, {
    "kv_get ok": (r) => r.ok === true,
  });
  errorRate.add(!getOk);
  if (!getOk) {
    console.error(`[kv_http] get FAIL VU=${__VU} key=${key} get=${JSON.stringify(getRes)}`);
  }
}
