// SPDX-License-Identifier: AGPL-3.0-or-later
// actor_capacity.js — Find the maximum number of actors the node can hold.
//
// Default: rust-embedded, eager (regular) actors — native tokio actors, ~1-2KB each.
//   These actors are never evicted; all spawned instances stay alive.
//
// ACTOR_MODE=virtual — virtual (on-demand) actors backed by the virtual actor manager.
//   The manager caps max live instances and evicts the oldest when the limit is exceeded.
//   This test tracks BOTH requested_total (every POST attempt) AND live_actual
//   (server-reported count after each batch) to measure eviction rate.
//
// Optional: any WASM language via --lang (e.g. go, typescript, python, rust-wasm).
//   For WASM languages each "actor" is a virtual actor backed by a wasmtime Store
//   which is much larger (~137MB for Python). Use a lower MEMORY_LIMIT_MB.
//
// Method:
//   1. Record baseline RSS via /api/v1/dashboard/system-info
//   2. Activate actors in batches of BATCH_SIZE (HTTP POST /ask → keeps actor alive for eager;
//      virtual actors may be evicted by the manager between batches)
//   3. After each batch, poll RSS and (for virtual mode) the live actor count
//   4. Stop when RSS > MEMORY_LIMIT_MB (default 2048 = 2GB) or error rate > 5%
//   5. Report: requested_total, live_actual, evictions, RSS, overhead per actor
//
// Run (rust-embedded eager, 2GB limit):
//   k6 run load-test/k6/scenarios/actor_capacity.js
//
// Run (virtual actors):
//   k6 run -e ACTOR_MODE=virtual load-test/k6/scenarios/actor_capacity.js
//
// Run (wasm language, smaller limit):
//   k6 run -e LANG=go -e ACTOR_MODE=virtual -e MEMORY_LIMIT_MB=512 load-test/k6/scenarios/actor_capacity.js
//
// Run from run.sh:
//   bash load-test/k6/run.sh --mode max-actors --lang rust-embedded
//   bash load-test/k6/run.sh --mode max-actors --lang rust-embedded --actor-type virtual

import http from "k6/http";
import { check, sleep } from "k6";
import { Counter, Gauge } from "k6/metrics";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, errorRate, requestCounter } from "../common.js";

const LANG            = __ENV.LANG            || "rust-embedded";
// ACTOR_MODE: "eager" = regular actors (no eviction); "virtual" = virtual actors (may be evicted)
const ACTOR_MODE      = __ENV.ACTOR_MODE      || "eager";
const BATCH           = parseInt(__ENV.BATCH            || "100");
// Stop when RSS exceeds this. Default 2048MB (2GB) for embedded. Lower for WASM.
const MEMORY_LIMIT_MB = parseInt(__ENV.MEMORY_LIMIT_MB  || "2048");
// Actor test stop threshold is 80% of the watchdog limit to leave headroom.
const STOP_AT_MB      = Math.floor(MEMORY_LIMIT_MB * 0.80);
// Safety ceiling: prevent runaway loops (200 * 100 = 20k actors max)
const MAX_BATCHES     = parseInt(__ENV.MAX_BATCHES       || "200");
// How long to sleep between batches (let RSS settle after spawns)
const SLEEP_BETWEEN_BATCHES_S = parseFloat(__ENV.SLEEP_BETWEEN_BATCHES_S || "0.5");

const isEmbedded = (LANG === "rust-embedded");
const serverUrl  = isEmbedded ? EMBEDDED_URL : BASE_URL;
const appId      = APP_IDS[LANG] || `perf-${LANG}`;
const actorType  = isEmbedded ? "gen_server" : "PerfActor";

export const options = {
  scenarios: {
    actor_capacity: {
      executor: "per-vu-iterations",
      vus: 1,
      iterations: MAX_BATCHES,
      maxDuration: "60m",
    },
  },
  thresholds: {
    "perf_error_rate": ["rate<0.05"],
  },
};

const actorCount    = new Gauge("perf_actor_count");
const rssGauge      = new Gauge("perf_node_rss_mb");
const overheadKbG   = new Gauge("perf_overhead_per_actor_kb");

// Virtual-mode only: track total requests attempted vs live instances reported by server.
const virtualRequested = new Counter("perf_virtual_requested_total");
const virtualLive      = new Gauge("perf_virtual_live");

let totalSpawned    = 0;   // for eager: actors confirmed spawned; for virtual: last live count
let requestedTotal  = 0;   // virtual only: cumulative POST attempts
let stopped         = false;

function getRss() {
  const res = http.get(`${serverUrl}/api/v1/dashboard/system-info`, {
    headers: authHeaders(),
    timeout: "10s",
  });
  if (res.status !== 200) return null;
  try {
    const body = JSON.parse(res.body);
    return (
      body.memory_usage_mb      ||
      body.system_info?.memory_rss_mb ||
      body.node?.rss_mb         ||
      null
    );
  } catch (_) { return null; }
}

// Poll the server for how many live actors it reports for our app.
// Returns null if not available or parse fails.
function getLiveActorCount() {
  const res = http.get(`${serverUrl}/api/v1/dashboard/applications`, {
    headers: authHeaders(),
    timeout: "10s",
  });
  if (res.status !== 200) return null;
  try {
    const body = JSON.parse(res.body);
    const apps = body.applications || [];
    for (const app of apps) {
      if (app.name === appId || app.application_id === appId) {
        return app.actor_count ?? app.active_instances ?? null;
      }
    }
    return null;
  } catch (_) { return null; }
}

export function setup() {
  const rss = getRss();
  console.log(`[actor_capacity] lang=${LANG}  mode=${ACTOR_MODE}  baseline_rss=${rss ?? "?"}MB  stop_at=${STOP_AT_MB}MB  limit=${MEMORY_LIMIT_MB}MB`);
  return { baseline_rss: rss || 0 };
}

export default function (data) {
  if (stopped) {
    sleep(0.1);
    return;
  }

  let batchErrors = 0;

  for (let i = 0; i < BATCH; i++) {
    const actorName = `cap-${LANG}-${ACTOR_MODE}-${(ACTOR_MODE === "eager" ? totalSpawned : requestedTotal) + i}`;
    const url = `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=10`;
    const res = http.post(
      url,
      JSON.stringify({ op: "echo", payload: actorName }),
      { headers: authHeaders(), tags: { op: "capacity_spawn" }, timeout: "15s" }
    );
    requestCounter.add(1, { op: "capacity_spawn" });

    if (ACTOR_MODE === "virtual") {
      virtualRequested.add(1);
      requestedTotal++;
    }

    if (res.status !== 200) {
      batchErrors++;
      errorRate.add(1);
    } else {
      errorRate.add(0);
    }
  }

  // For eager mode: track confirmed successful spawns.
  if (ACTOR_MODE === "eager") {
    totalSpawned += BATCH - batchErrors;
    actorCount.add(totalSpawned);
  }

  // Let RSS settle after spawning
  if (SLEEP_BETWEEN_BATCHES_S > 0) {
    sleep(SLEEP_BETWEEN_BATCHES_S);
  }

  const rss = getRss();
  if (rss !== null) {
    rssGauge.add(rss);

    if (ACTOR_MODE === "virtual") {
      // Poll live actor count from server (virtual manager may have evicted some)
      const live = getLiveActorCount();
      if (live !== null) {
        virtualLive.add(live);
        totalSpawned = live;   // keep in sync with what's actually alive
        actorCount.add(live);
        const evicted = requestedTotal - live;
        console.log(`[actor_capacity/virtual] requested=${requestedTotal}  live=${live}  evicted=${evicted}  rss=${rss}MB`);
      } else {
        actorCount.add(requestedTotal);
        console.log(`[actor_capacity/virtual] requested=${requestedTotal}  live=unknown  rss=${rss}MB`);
      }
    } else {
      const baseline = data.baseline_rss || 0;
      if (totalSpawned > 0 && rss > baseline) {
        const overheadKb = ((rss - baseline) * 1024) / totalSpawned;
        overheadKbG.add(overheadKb);
        console.log(`[actor_capacity/eager] actors=${totalSpawned}  rss=${rss}MB  overhead=${overheadKb.toFixed(1)}KB/actor`);
      }
    }

    if (rss > STOP_AT_MB) {
      console.log(`[actor_capacity] STOP: RSS ${rss}MB > stop threshold ${STOP_AT_MB}MB (limit ${MEMORY_LIMIT_MB}MB) at requested=${requestedTotal} live=${totalSpawned}`);
      stopped = true;
    }
  }

  if (batchErrors > BATCH * 0.05) {
    console.log(`[actor_capacity] STOP: error rate too high (${batchErrors}/${BATCH}) at requested=${requestedTotal}`);
    stopped = true;
  }
}

export function handleSummary(data) {
  function fmtMs(v) {
    if (v === undefined || v === null || isNaN(+v)) return "–";
    const n = +v;
    if (n === 0) return "0µs";
    if (n < 1.0) return `${Math.round(n * 1000)}µs`;
    return `${n.toFixed(1)}ms`;
  }
  function rp(s, w) { const t = String(s); return t + " ".repeat(Math.max(0, w - t.length)); }
  function row(s)   { const c = `  ${s}`; return `║${(c + " ".repeat(63)).slice(0, 63)}║`; }
  const HR = "═".repeat(63);

  const durSec    = (data.state.testRunDurationMs / 1000).toFixed(0);
  const actors    = data.metrics["perf_actor_count"]?.values?.max ?? 0;
  const rss       = data.metrics["perf_node_rss_mb"]?.values?.max ?? 0;
  const ohKb      = data.metrics["perf_overhead_per_actor_kb"]?.values?.value ?? 0;
  const errRate   = ((data.metrics["perf_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const totalReq  = data.metrics["perf_requests_total"]?.values?.count ?? 0;

  const hm        = data.metrics["http_req_duration"];
  const rpsVal    = totalReq > 0 ? (totalReq / (data.state.testRunDurationMs / 1000)).toFixed(1) : "0";

  const isVirtual = ACTOR_MODE === "virtual";
  const reqTotal  = data.metrics["perf_virtual_requested_total"]?.values?.count ?? 0;
  const liveLast  = data.metrics["perf_virtual_live"]?.values?.value ?? 0;
  const evicted   = isVirtual ? Math.max(0, reqTotal - liveLast) : 0;

  const erlangPerProcess = 0.3;  // ~300 bytes per Erlang process
  let vsErlang = "–";
  if (!isVirtual && ohKb > 0) {
    vsErlang = `${(ohKb / erlangPerProcess).toFixed(0)}× Erlang process`;
  }

  const lines = [
    "",
    `╔${HR}╗`,
    row(`TEST: actor/capacity  │ LANG: ${LANG}  │ MODE: ${ACTOR_MODE}`),
    row(`batch_size: ${BATCH}  │ memory_limit: ${MEMORY_LIMIT_MB}MB  │ stop_at: ${STOP_AT_MB}MB  │ duration: ${durSec}s`),
    `╠${HR}╣`,
    row("CAPACITY RESULTS"),
  ];

  if (isVirtual) {
    lines.push(row(`  requested_total:        ${rp(reqTotal, 10)}  (every POST attempt)`));
    lines.push(row(`  live_at_peak:            ${rp(actors, 10)}  (server-reported max)`));
    lines.push(row(`  live_at_end:             ${rp(liveLast, 10)}  (server-reported final)`));
    lines.push(row(`  evictions_detected:      ${rp(evicted, 10)}  (requested - live_at_end)`));
    lines.push(row(`  peak_rss:                ${rp(rss.toFixed ? rss.toFixed(0) : rss, 8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)`));
    lines.push(row("  NOTE: virtual manager evicts oldest instances when cap is reached"));
  } else {
    lines.push(row(`  max_actors_spawned:   ${rp(actors, 10)}`));
    lines.push(row(`  peak_rss:             ${rp(rss.toFixed ? rss.toFixed(0) : rss, 8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)`));
    lines.push(row(`  overhead_per_actor:   ${rp(typeof ohKb === "number" ? ohKb.toFixed(1) : ohKb, 8)} KB`));
    lines.push(row(`  vs_erlang_process:    ${vsErlang}   (Erlang: ~0.3KB)`));
    lines.push(row("  NOTE: eager actors have NO eviction limit — all spawned instances stay alive"));
  }

  lines.push(`╠${HR}╣`);
  lines.push(row("SPAWN PERFORMANCE"));
  lines.push(row(`  total_requests:       ${rp(totalReq, 10)}   spawn_rps: ${rpsVal}/s`));
  lines.push(row(`  spawn_error_rate:     ${errRate}%`));
  lines.push(row("  spawn http_req_duration (incl. actor activation):"));
  lines.push(hm
    ? row(`    p50=${rp(fmtMs(hm.values.med),8)} p95=${rp(fmtMs(hm.values["p(95)"]),8)} p99=${fmtMs(hm.values["p(99)"])}`)
    : row("    (no data)"));

  const batchesDone = data.metrics["iterations"]?.values?.count ?? 0;
  const stopped_reason = batchesDone < MAX_BATCHES
    ? (rss > STOP_AT_MB * 0.99 ? `Memory threshold (${STOP_AT_MB}MB = 80% of ${MEMORY_LIMIT_MB}MB) reached` : "Error rate exceeded")
    : "Max batches reached (no limit found)";
  lines.push(`╠${HR}╣`);
  lines.push(row(`STOPPED: ${stopped_reason}`));
  lines.push(`╚${HR}╝`, "");

  return { stdout: lines.join("\n") };
}
