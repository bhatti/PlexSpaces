// SPDX-License-Identifier: AGPL-3.0-or-later
// echo_http.js — Measure plain HTTP round-trip latency per language.
//
// Each VU sends ITERATIONS echo requests to one actor instance.
// The echo operation does zero compute — result measures the overhead of:
//   HTTP parse → auth → routing → WASM sandbox dispatch → response serialise → HTTP write
//
// Run:
//   k6 run -e LANG=python -e VUS=100 -e ITERATIONS=100 k6/scenarios/echo_http.js

import http from "k6/http";
import { sleep } from "k6";
import { check } from "k6";
import exec from "k6/execution";
import { Trend, Rate, Counter } from "k6/metrics";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, vuInstance, errorRate, echoLatency, requestCounter } from "../common.js";

// Per-activation-state latency trends to diagnose virtual actor spawn overhead.
const firstActivationLatency = new Trend("perf_echo_first_activation_ms", true);
const steadyStateLatency     = new Trend("perf_echo_steady_state_ms",     true);

// Per-VU flag: has this VU already activated its virtual actor instance?
const _vuActivated = {};

const LANG       = __ENV.LANG       || "python";
const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const DURATION   = __ENV.DURATION   || "";   // e.g. "2m" — overrides per-VU iterations
// ACTOR_TYPE: "regular" = pre-spawned actors, "virtual" = on-demand activation
const ACTOR_TYPE = __ENV.ACTOR_TYPE || "regular";
// WARM_COUNT: number of pre-warmed embedded actors (perf-vu0..N-1); default matches perf_actor default
const WARM_COUNT = parseInt(__ENV.WARM_COUNT || "10");
// MAX_WASM_INSTANCES: cap the virtual actor pool for WASM languages to bound memory.
// Python WASM: ~137MB/actor. Default 10 = ~1.4GB. Set higher to measure at scale.
const MAX_WASM_INSTANCES = parseInt(__ENV.MAX_WASM_INSTANCES || "10");
// LOG_ERRORS: log first N unique error bodies to stdout for debugging (default 5)
const LOG_ERRORS = parseInt(__ENV.LOG_ERRORS || "5");
// ASK_TIMEOUT: server-side actor reply timeout in seconds.
// Use 30 for virtual actors that need to activate (WASM load can take >5s).
const ASK_TIMEOUT = parseInt(__ENV.ASK_TIMEOUT || "5");

// Dedup log: only log each unique error body once up to LOG_ERRORS distinct messages.
const _loggedErrors = new Map();

// Parse k6 duration strings ("30s", "2m", "5m30s") to seconds.
function parseDurationSecs(d) {
  if (!d) return Infinity;
  let s = 0;
  const m = d.match(/(\d+)m/); if (m) s += parseInt(m[1]) * 60;
  const sec = d.match(/(\d+)s/); if (sec) s += parseInt(sec[1]);
  return s;
}

function buildScenario() {
  if (DURATION) {
    return {
      echo_http: {
        executor: "constant-vus",
        vus: VUS,
        duration: DURATION,
        gracefulStop: "0s",
      },
    };
  }
  return {
    echo_http: {
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
    "perf_echo_latency_ms": ["p(99)<2000"],
    "perf_error_rate": ["rate<0.05"],
  },
};

const appId     = APP_IDS[LANG] || `perf-${LANG}`;
// Embedded Rust actors use "gen_server" as the type slug (from BehaviorType::GenServer).
// WASM-deployed actors use the actorType declared in app-config.toml (typically "PerfActor").
// ACTOR_TYPE=virtual routes to a new unique actor per VU (tests on-demand activation).
const actorType = (LANG === "rust-embedded") ? "gen_server" : "PerfActor";

// Route embedded actor requests to port 8092; all others to the main server.
const serverUrl = (LANG === "rust-embedded") ? EMBEDDED_URL : BASE_URL;

// setup() runs once before any VUs start. For virtual actor mode, send one request
// per pool slot to trigger activation sequentially — avoids concurrent-activation
// race where 10 simultaneous activations of the same actor all time out except one.
export function setup() {
  if (ACTOR_TYPE !== "virtual") return;
  const warmTimeout = ASK_TIMEOUT;
  for (let i = 1; i <= MAX_WASM_INSTANCES; i++) {
    const instance = `virtual-vu${i}`;
    const url = `${serverUrl}/api/v1/actors/${appId}/${instance}:${actorType}/ask?timeout=${warmTimeout}`;
    const res = http.post(url, JSON.stringify({ op: "echo", payload: { setup: true } }), {
      headers: authHeaders(),
    });
    if (res.status !== 200) {
      console.warn(`[setup] warm ${instance} status=${res.status}`);
    }
  }
}

export default function () {
  // In time-based runs: skip requests in the final (ASK_TIMEOUT + 1)s to prevent
  // in-flight requests from racing with the server-side ask timeout and producing 504s.
  let httpTimeout = `${ASK_TIMEOUT + 1}s`;
  if (DURATION) {
    const durSecs = parseDurationSecs(DURATION);
    const remaining = durSecs * (1 - exec.scenario.progress);
    // Skip the last (ASK_TIMEOUT + 3)s to prevent in-flight requests from racing the server ask timeout.
    if (remaining <= ASK_TIMEOUT + 3) {
      return;
    }
    // Clamp HTTP timeout so k6 never waits longer than remaining time.
    const clampedMs = Math.max(500, Math.min((ASK_TIMEOUT + 1) * 1000, (remaining - 0.5) * 1000));
    httpTimeout = `${clampedMs}ms`;
  }

  // For virtual actors: unique instance per VU triggers on-demand activation.
  // For regular rust-embedded: cycle through pre-warmed pool (perf-vu0..N-1).
  // For regular WASM: cycle through pool (vu0..N-1).
  // URL format: {instance}:{actor_type}
  let instance;
  if (ACTOR_TYPE === "virtual") {
    // Cap pool to MAX_WASM_INSTANCES to bound memory usage for WASM languages.
    // VUs above the cap reuse existing instances (measuring steady-state throughput).
    const poolIdx = ((__VU - 1) % MAX_WASM_INSTANCES) + 1;
    instance = `virtual-vu${poolIdx}`;
  } else if (LANG === "rust-embedded") {
    instance = `perf-vu${(__VU - 1) % WARM_COUNT}`;
  } else {
    instance = vuInstance();
  }
  const url = `${serverUrl}/api/v1/actors/${appId}/${instance}:${actorType}/ask?timeout=${ASK_TIMEOUT}`;
  const payload = JSON.stringify({ op: "echo", payload: { vu: __VU, iter: __ITER } });

  const start = Date.now();
  const res = http.post(url, payload, {
    headers: authHeaders(),
    tags: { transport: "http", lang: LANG, op: "echo" },
    timeout: httpTimeout,
  });
  const elapsed = Date.now() - start;

  echoLatency.add(elapsed, { transport: "http", lang: LANG });
  requestCounter.add(1, { transport: "http", lang: LANG, op: "echo" });

  // For virtual actor runs: split first-activation vs steady-state latency.
  if (ACTOR_TYPE === "virtual") {
    if (!_vuActivated[__VU]) {
      firstActivationLatency.add(elapsed);
      _vuActivated[__VU] = true;
    } else {
      steadyStateLatency.add(elapsed);
    }
  }

  const ok = check(res, {
    "HTTP 200":    (r) => r.status === 200,
    // The server wraps actor responses: { success: true, payload: { ok: true, ... } }
    "success":     (r) => { try { const b = JSON.parse(r.body); return b.success === true || b.ok === true; } catch(_) { return false; } },
    "no error":    (r) => { try { const b = JSON.parse(r.body); return !b.error && !b.error_message; } catch(_) { return false; } },
  });
  errorRate.add(!ok);

  if (!ok && _loggedErrors.size < LOG_ERRORS) {
    const body = res.body ? res.body.substring(0, 300) : "(empty)";
    if (!_loggedErrors.has(body)) {
      _loggedErrors.set(body, true);
      console.error(`[echo_http error] VU=${__VU} iter=${__ITER} status=${res.status} actor_type=${ACTOR_TYPE} url=${url} body=${body}`);
    }
  }

  // No sleep — we want to measure maximum sustainable throughput.
}

export function handleSummary(data) {
  // Format ms value using µs for sub-millisecond results (e.g. 0.123ms → 123µs).
  function fmtMs(v) {
    if (v === undefined || v === null || isNaN(+v)) return "–";
    const n = +v;
    if (n === 0) return "0µs";
    if (n < 1.0) return `${Math.round(n * 1000)}µs`;
    return `${n.toFixed(1)}ms`;
  }
  // Right-pad to width w.
  function rp(s, w) { const t = String(s); return t + " ".repeat(Math.max(0, w - t.length)); }
  // Build a box row exactly 63 chars wide between ║.
  function row(s) { const c = `  ${s}`; return `║${(c + " ".repeat(63)).slice(0, 63)}║`; }
  const HR = "═".repeat(63);

  const durSec  = (data.state.testRunDurationMs / 1000).toFixed(0);
  const maxVUs  = data.metrics["vus"]?.values?.max ?? VUS;
  const iters   = data.metrics["iterations"]?.values?.count ?? 0;
  const totalR  = data.metrics["perf_requests_total"]?.values?.count ?? 0;
  const rps     = totalR > 0 ? (totalR / (data.state.testRunDurationMs / 1000)).toFixed(1) : "0";
  const errPct  = ((data.metrics["perf_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const failPct = ((data.metrics["http_req_failed"]?.values?.rate ?? 0) * 100).toFixed(2);
  const recvMB  = ((data.metrics["data_received"]?.values?.count ?? 0) / 1024 / 1024).toFixed(1);
  const sentMB  = ((data.metrics["data_sent"]?.values?.count ?? 0) / 1024 / 1024).toFixed(1);

  const em = data.metrics["perf_echo_latency_ms"];
  const hm = data.metrics["http_req_duration"];

  const lines = [
    "",
    `╔${HR}╗`,
    row(`TEST: echo/http  │ LANG: ${LANG}  │ TYPE: ${ACTOR_TYPE}`),
    row(`VUs: ${maxVUs}  │ duration: ${durSec}s  │ iters: ${iters}  │ RPS: ${rps}/s`),
    `╠${HR}╣`,
    row("LATENCY  (perf_echo_latency_ms — actor echo round-trip)"),
    em
      ? row(`  p50=${rp(fmtMs(em.values.med),8)} p95=${rp(fmtMs(em.values["p(95)"]),8)} p99=${rp(fmtMs(em.values["p(99)"]),8)} avg=${rp(fmtMs(em.values.avg),8)} max=${fmtMs(em.values.max)}`)
      : row("  (no data collected)"),
    row("LATENCY  (http_req_duration — k6 built-in, incl. connection overhead)"),
    hm
      ? row(`  p95=${rp(fmtMs(hm.values["p(95)"]),8)} p99=${rp(fmtMs(hm.values["p(99)"]),8)} avg=${fmtMs(hm.values.avg)}`)
      : row("  (no data)"),
    row(`  http_conn p95=${rp(fmtMs(data.metrics["http_req_connecting"]?.values?.["p(95)"]),8)} blocked p95=${fmtMs(data.metrics["http_req_blocked"]?.values?.["p(95)"])}`),
    `╠${HR}╣`,
    row("THROUGHPUT"),
    row(`  total_requests: ${rp(totalR,10)}  RPS: ${rp(rps,10)} req/s`),
    row(`  data_recv: ${rp(recvMB,8)} MB   data_sent: ${sentMB} MB`),
    `╠${HR}╣`,
    row("ERRORS"),
    row(`  actor_error_rate: ${rp(errPct,7)}%   http_req_failed: ${failPct}%`),
    ...(errPct !== "0.00" || failPct !== "0.00" ? [row("  ⚠ Non-zero errors detected — check server.log")] : []),
  ];

  if (ACTOR_TYPE === "virtual" && data.metrics["perf_echo_first_activation_ms"]) {
    const fa = data.metrics["perf_echo_first_activation_ms"];
    const ss = data.metrics["perf_echo_steady_state_ms"];
    lines.push(`╠${HR}╣`);
    lines.push(row("VIRTUAL ACTOR ACTIVATION BREAKDOWN"));
    lines.push(row(`  first_activation: p50=${rp(fmtMs(fa.values.med),8)} p95=${rp(fmtMs(fa.values["p(95)"]),8)} max=${fmtMs(fa.values.max)}`));
    if (ss) {
      lines.push(row(`  steady_state:     p50=${rp(fmtMs(ss.values.med),8)} p95=${fmtMs(ss.values["p(95)"])}`));
    }
  }

  lines.push(`╚${HR}╝`, "");
  return { stdout: lines.join("\n") };
}
