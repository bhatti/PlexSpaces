// SPDX-License-Identifier: AGPL-3.0-or-later
// actor_capacity.js — Find the maximum number of actors the node can hold.
//
// DESIGN: parallel spawn VUs + 1 overseer VU polling RSS.
//   - shared-iterations: N spawn VUs share a pool of MAX_ACTORS iterations.
//     Each iteration activates one unique actor (name = cap-LANG-MODE-ITER).
//     Parallelism: VUS (default 50) concurrent spawns → ~1500+ actors/sec.
//   - overseer (1 VU): polls server RSS every POLL_INTERVAL_MS; calls
//     exec.test.abort() when RSS > STOP_AT_MB or error rate > 5%.
//
// ACTOR_MODE: "eager" = each actor lives until process exit (no eviction).
//             "virtual" = virtual actors, subject to LRU eviction.
//
// Expected results (rust-embedded, 2GB limit):
//   eager:   50K–200K actors, ~2–5KB/actor marginal, limited by available RAM
//   virtual: ~100 live at any time (LRU cap), hundreds of thousands requested total
//
// Run (rust-embedded eager, 2GB limit):
//   k6 run -e LANG=rust-embedded load-test/k6/scenarios/actor_capacity.js
//
// Run (virtual actors):
//   k6 run -e LANG=rust-embedded -e ACTOR_MODE=virtual load-test/k6/scenarios/actor_capacity.js
//
// Run (WASM language, smaller limit):
//   k6 run -e LANG=go -e ACTOR_MODE=virtual -e MEMORY_LIMIT_MB=512 load-test/k6/scenarios/actor_capacity.js
//
// Run from run.sh:
//   bash load-test/k6/run.sh --mode max-actors --lang rust-embedded

import http from "k6/http";
import { check, sleep } from "k6";
import exec from "k6/execution";
import { Counter, Gauge, Trend } from "k6/metrics";
import { authHeaders, BASE_URL, EMBEDDED_URL, APP_IDS, errorRate, requestCounter } from "../common.js";

const LANG            = __ENV.LANG            || "rust-embedded";
const ACTOR_MODE      = __ENV.ACTOR_MODE      || "eager";
// Concurrent spawn VUs. Higher = faster spawning, more server-side concurrency pressure.
const VUS             = parseInt(__ENV.VUS             || "50");
// Total actors to attempt. Test stops early when memory limit is hit.
const MAX_ACTORS      = parseInt(__ENV.MAX_ACTORS      || "500000");
// Stop when node RSS exceeds this (MB). Default 2048=2GB for embedded, lower for WASM.
const MEMORY_LIMIT_MB = parseInt(__ENV.MEMORY_LIMIT_MB || "2048");
// Abort threshold = 90% of the limit (leaves headroom for RSS snapshots to settle).
const STOP_AT_MB      = Math.floor(MEMORY_LIMIT_MB * 0.90);
// How often the overseer VU polls RSS and actor count.
const POLL_INTERVAL_MS = parseInt(__ENV.POLL_INTERVAL_MS || "3000");
// Hard wall-clock timeout. Prevents indefinite runs.
const MAX_DURATION    = __ENV.MAX_DURATION || "30m";

const isEmbedded = (LANG === "rust-embedded");
const serverUrl  = isEmbedded ? EMBEDDED_URL : BASE_URL;
const appId      = APP_IDS[LANG] || `perf-${LANG}`;
const actorType  = isEmbedded ? "gen_server" : "PerfActor";

// Calculate overseer duration from MAX_DURATION + small buffer.
function parseDurationToS(d) {
  if (!d) return 1800;
  let s = 0;
  const h = d.match(/(\d+)h/); if (h) s += parseInt(h[1]) * 3600;
  const m = d.match(/(\d+)m/); if (m) s += parseInt(m[1]) * 60;
  const sv = d.match(/(\d+)s/); if (sv) s += parseInt(sv[1]);
  return s > 0 ? s : 1800;
}
const overseerDuration = `${parseDurationToS(MAX_DURATION) + 30}s`;

export const options = {
  scenarios: {
    // Spawn VUs: each iteration activates one unique actor.
    // shared-iterations distributes MAX_ACTORS across VUS VUs in parallel.
    spawn_actors: {
      executor: "shared-iterations",
      vus: VUS,
      iterations: MAX_ACTORS,
      maxDuration: MAX_DURATION,
    },
    // Overseer: polls RSS + actor count, aborts when limit is reached.
    overseer: {
      executor: "constant-vus",
      vus: 1,
      duration: overseerDuration,
      exec: "overseerFn",
      gracefulStop: "5s",
    },
  },
  thresholds: {
    "perf_error_rate": ["rate<0.05"],
  },
};

const spawnedCounter  = new Counter("perf_actors_spawned_total");
const rssGauge        = new Gauge("perf_node_rss_mb");
const actorLiveGauge  = new Gauge("perf_actor_count");
const overheadKbG     = new Gauge("perf_overhead_per_actor_kb");
const spawnLatency    = new Trend("perf_spawn_latency_ms", true);

// Virtual-mode tracking.
const virtualLive     = new Gauge("perf_virtual_live");

// Poll RSS and active actor count from /api/v1/dashboard/nodes.
// Returns { rssMb: number|null, actors: number|null }.
function getNodeMetrics() {
  const res = http.get(`${serverUrl}/api/v1/dashboard/nodes`, {
    headers: authHeaders(),
    timeout: "8s",
    tags: { op: "metrics_poll" },
  });
  if (res.status === 200) {
    try {
      const body = JSON.parse(res.body);
      const nodes = body.nodes || [];
      // Find the node that has metrics (perf-embedded-node or the main server)
      for (let i = 0; i < nodes.length; i++) {
        const m = nodes[i].metrics || {};
        if (m.active_actors !== undefined || m.memory_used_bytes > 0) {
          const rssMb = m.memory_used_bytes > 0 ? m.memory_used_bytes / (1024 * 1024) : null;
          const actors = typeof m.active_actors === "number" ? m.active_actors : null;
          return { rssMb, actors };
        }
      }
    } catch (_) {}
  }
  return { rssMb: null, actors: null };
}

export function setup() {
  const nm = getNodeMetrics();
  const rss = nm.rssMb;
  console.log(
    `[actor_capacity] lang=${LANG}  mode=${ACTOR_MODE}  vus=${VUS}` +
    `  baseline_rss=${rss !== null ? rss.toFixed(0) : "?"}MB` +
    `  baseline_actors=${nm.actors ?? "?"}` +
    `  stop_at=${STOP_AT_MB}MB  limit=${MEMORY_LIMIT_MB}MB` +
    `  max_actors=${MAX_ACTORS}`
  );
  return { baseline_rss: rss || 0 };
}

// ─── Spawn VU body ────────────────────────────────────────────────────────────
// Each iteration activates one unique actor. __ITER is unique per iteration in
// shared-iterations mode — no collision across VUs.

export default function (data) {
  const actorName = `cap-${LANG}-${ACTOR_MODE}-${__ITER}`;
  const url = `${serverUrl}/api/v1/actors/${appId}/${actorName}:${actorType}/ask?timeout=10`;

  const start = Date.now();
  const res = http.post(
    url,
    JSON.stringify({ op: "echo", payload: actorName }),
    { headers: authHeaders(), tags: { op: "capacity_spawn" }, timeout: "15s" }
  );
  const latMs = Date.now() - start;
  spawnLatency.add(latMs);
  requestCounter.add(1, { op: "capacity_spawn" });

  if (res.status === 200) {
    spawnedCounter.add(1);
    errorRate.add(0);
  } else {
    errorRate.add(1);
  }
}

// ─── Overseer VU ─────────────────────────────────────────────────────────────
// Polls RSS + live actor count. Aborts when memory limit reached.

export function overseerFn(data) {
  const baselineRss = (data && data.baseline_rss) ? data.baseline_rss : 0;

  for (;;) {
    sleep(POLL_INTERVAL_MS / 1000);

    const nm = getNodeMetrics();
    const rss = nm.rssMb;
    const liveActors = nm.actors;

    if (rss !== null) {
      rssGauge.add(rss);
    }
    if (liveActors !== null) {
      actorLiveGauge.add(liveActors);
      if (ACTOR_MODE === "virtual") {
        virtualLive.add(liveActors);
      }
    }

    if (rss !== null && liveActors !== null && liveActors > 0 && rss > baselineRss) {
      const overheadKb = ((rss - baselineRss) * 1024) / liveActors;
      overheadKbG.add(overheadKb);
    }

    if (rss !== null && liveActors !== null) {
      console.log(
        `[actor_capacity/overseer] actors=${liveActors}  rss=${rss.toFixed(0)}MB` +
        (rss > baselineRss && liveActors > 0
          ? `  overhead=${((rss - baselineRss) * 1024 / liveActors).toFixed(1)}KB/actor`
          : "")
      );
    }

    if (rss !== null && rss > STOP_AT_MB) {
      console.log(
        `[actor_capacity] ABORT: RSS ${rss.toFixed(0)}MB > ${STOP_AT_MB}MB limit` +
        ` at actors=${liveActors ?? "?"}`
      );
      exec.test.abort("Memory limit reached");
      return;
    }
  }
}

// ─── Summary ──────────────────────────────────────────────────────────────────

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
  const spawned   = data.metrics["perf_actors_spawned_total"]?.values?.count ?? 0;
  const liveMax   = data.metrics["perf_actor_count"]?.values?.max ?? 0;
  const rssMax    = data.metrics["perf_node_rss_mb"]?.values?.max ?? 0;
  const ohKb      = data.metrics["perf_overhead_per_actor_kb"]?.values?.value ?? 0;
  const errRate   = ((data.metrics["perf_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const totalReq  = data.metrics["perf_requests_total"]?.values?.count ?? 0;
  const hm        = data.metrics["perf_spawn_latency_ms"];
  const rpsVal    = totalReq > 0 ? (totalReq / +durSec).toFixed(0) : "0";

  const isVirtual = ACTOR_MODE === "virtual";
  const liveLast  = data.metrics["perf_virtual_live"]?.values?.value ?? 0;

  const erlangPerProcess = 0.3;
  let vsErlang = "–";
  if (!isVirtual && ohKb > 0) {
    vsErlang = `${(ohKb / erlangPerProcess).toFixed(0)}× Erlang (~0.3KB)`;
  }

  const lines = [
    "",
    `╔${HR}╗`,
    row(`TEST: actor/capacity  │ LANG: ${LANG}  │ MODE: ${ACTOR_MODE}`),
    row(`spawn_vus: ${VUS}  │ memory_limit: ${MEMORY_LIMIT_MB}MB  │ stop_at: ${STOP_AT_MB}MB  │ duration: ${durSec}s`),
    `╠${HR}╣`,
    row("CAPACITY RESULTS"),
  ];

  if (isVirtual) {
    lines.push(row(`  spawns_attempted:      ${rp(spawned, 10)}  (every POST, including re-activations)`));
    lines.push(row(`  live_at_peak:           ${rp(liveMax, 10)}  (server-reported, polled every ${POLL_INTERVAL_MS}ms)`));
    lines.push(row(`  live_at_end:            ${rp(liveLast, 10)}  (final poll before teardown)`));
    lines.push(row(`  peak_rss:               ${rp(rssMax.toFixed ? rssMax.toFixed(0) : rssMax, 8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)`));
    lines.push(row("  NOTE: eager strategy = no LRU eviction. virtual = evict oldest at cap."));
  } else {
    lines.push(row(`  max_live_actors:      ${rp(liveMax, 10)}  (server-reported peak)`));
    lines.push(row(`  spawns_confirmed:     ${rp(spawned, 10)}  (200 responses)`));
    lines.push(row(`  peak_rss:             ${rp(rssMax.toFixed ? rssMax.toFixed(0) : rssMax, 8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)`));
    lines.push(row(`  overhead_per_actor:   ${rp(typeof ohKb === "number" ? ohKb.toFixed(1) : ohKb, 8)} KB  (marginal, w/ baseline subtracted)`));
    lines.push(row(`  vs_erlang_process:    ${vsErlang}`));
    lines.push(row("  NOTE: eager = no eviction. Actors live until node shuts down."));
  }

  lines.push(`╠${HR}╣`);
  lines.push(row("SPAWN PERFORMANCE"));
  lines.push(row(`  total_requests:       ${rp(totalReq, 10)}   spawn_rps: ${rpsVal}/s`));
  lines.push(row(`  spawn_error_rate:     ${errRate}%`));
  lines.push(row(`  spawn_vus:            ${VUS}  (parallel)`));
  lines.push(row("  spawn latency (HTTP round-trip incl. actor activation):"));
  lines.push(hm
    ? row(`    p50=${rp(fmtMs(hm.values.med),8)} p95=${rp(fmtMs(hm.values["p(95)"]),8)} p99=${fmtMs(hm.values["p(99)"])}`)
    : row("    (no data)"));

  const aborted = data.state.isAborted ?? false;
  const stopped_reason = aborted
    ? `Memory threshold ${STOP_AT_MB}MB (90% of ${MEMORY_LIMIT_MB}MB) reached`
    : `Max actors (${MAX_ACTORS}) reached — no memory limit hit`;
  lines.push(`╠${HR}╣`);
  lines.push(row(`STOPPED: ${stopped_reason}`));
  lines.push(`╚${HR}╝`, "");

  return { stdout: lines.join("\n") };
}
