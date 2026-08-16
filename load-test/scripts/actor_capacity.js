#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// actor_capacity.js — Find the maximum number of actors the node can hold.
//
// Two modes (ACTOR_MODE):
//   regular — unique actor names accumulate without eviction (PERF_MAX_VIRTUAL_POOL=0 on server).
//             Test stops when RSS > STOP_AT_MB. Reports max live actors + per-actor overhead.
//   virtual — unique actor names; server caps pool via PERF_MAX_VIRTUAL_POOL (e.g. 200000).
//             LRU evicts oldest when pool is full. RSS stays bounded. Test runs for MAX_DURATION_S.
//             Virtual actors use eager activation strategy (spawned immediately on first request).
//             Reports spawn throughput under bounded memory (spawn_rps).
//
// Strategy:
//   - CONCURRENCY worker loops fire POST /tell to new unique actor names.
//     /tell triggers virtual actor auto-activation (eager) and returns 202 immediately —
//     workers never block on actor init/reply, so throughput is limited only by server
//     accept rate and network RTT, not by activation latency.
//   - A monitor loop polls /api/v1/dashboard/nodes every POLL_INTERVAL_MS to track
//     RSS and live actor count.
//   - regular: stops when RSS > STOP_AT_MB. virtual: runs until MAX_DURATION_S.
//
// Usage:
//   node actor_capacity.js
//   ACTOR_MODE=regular CONCURRENCY=500 MEMORY_LIMIT_MB=4096 node actor_capacity.js
//   ACTOR_MODE=virtual CONCURRENCY=500 node actor_capacity.js
//   LANG=go BASE_URL=http://localhost:8091 node actor_capacity.js
//
// Environment variables:
//   BASE_URL            Server URL               (default: http://localhost:8092)
//   LANG                Actor language           (default: rust-embedded)
//   ACTOR_MODE          regular | virtual        (default: regular)
//   CONCURRENCY         Parallel tell workers    (default: 500)
//   MEMORY_LIMIT_MB     Hard stop threshold MB   (default: 2048)
//   STOP_PERCENT        % of MEMORY_LIMIT to stop (default: 90) — regular mode only
//   POLL_INTERVAL_MS    Monitor poll interval    (default: 3000)
//   MAX_DURATION_S      Wall clock timeout secs  (default: 1800 = 30m)
//   NO_AUTH             Set to 1 to disable auth (default: 1)
//   AUTH_TOKEN          Bearer token if auth on  (default: "")
//   APP_ID              App ID override          (default: derived from LANG)
//   ACTOR_TYPE          Actor type override      (default: derived from LANG)

"use strict";
const http = require("http");
const https = require("https");
const { execSync } = require("child_process");

// Shared keep-alive agent: workers reuse TCP connections instead of opening a new one per request.
// This eliminates ECONNRESET from flooding the server's TCP accept queue with 500 simultaneous connections.
const keepAliveAgent = new http.Agent({ keepAlive: true, maxSockets: 600 });

// ─── Config ───────────────────────────────────────────────────────────────────
const ACTOR_LANG      = process.env.ACTOR_LANG      || process.env.PERF_LANG || "rust-embedded";
const LANG            = ACTOR_LANG;  // alias used below
const ACTOR_MODE      = process.env.ACTOR_MODE      || "regular";  // regular | virtual
const CONCURRENCY     = parseInt(process.env.CONCURRENCY     || "200");
const MEMORY_LIMIT_MB = parseInt(process.env.MEMORY_LIMIT_MB || "2048");
const STOP_PERCENT    = parseInt(process.env.STOP_PERCENT    || "100");
const STOP_AT_MB      = Math.floor(MEMORY_LIMIT_MB * STOP_PERCENT / 100);
const POLL_INTERVAL_MS = parseInt(process.env.POLL_INTERVAL_MS || "2000");
const MAX_DURATION_S  = parseInt(process.env.MAX_DURATION_S  || "3600");
const NO_AUTH         = process.env.NO_AUTH !== "0";
const AUTH_TOKEN      = process.env.AUTH_TOKEN || "";

const isVirtual = (ACTOR_MODE === "virtual");

const APP_IDS = {
  python:          "perf-python",
  go:              "perf-go",
  typescript:      "perf-typescript",
  "rust-wasm":     "perf-rust-wasm",
  "rust-embedded": "perf-embedded",
};
const ACTOR_TYPES = {
  python:          "PerfActor",
  go:              "PerfActor",
  typescript:      "PerfActor",
  "rust-wasm":     "PerfActor",
  "rust-embedded": "gen_server",
};

const isEmbedded = (LANG === "rust-embedded");

let BASE_URL = process.env.BASE_URL || (isEmbedded ? "http://localhost:8092" : "http://localhost:8091");
if (isEmbedded && !process.env.BASE_URL) {
  BASE_URL = process.env.EMBEDDED_URL || "http://localhost:8092";
}

const APP_ID      = process.env.APP_ID     || APP_IDS[LANG]     || `perf-${LANG}`;
const ACTOR_TYPE  = process.env.ACTOR_TYPE || ACTOR_TYPES[LANG] || "PerfActor";

// ─── HTTP helpers ─────────────────────────────────────────────────────────────
function authHeaders() {
  const h = { "Content-Type": "application/json" };
  if (NO_AUTH) {
    h["x-tenant-id"] = "default";
  } else if (AUTH_TOKEN) {
    h["Authorization"] = `Bearer ${AUTH_TOKEN}`;
  } else {
    h["x-tenant-id"] = "default";
  }
  return h;
}

function request(method, url, body, timeoutMs = 10000) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const lib = u.protocol === "https:" ? https : http;
    const data = body ? JSON.stringify(body) : null;
    const headers = authHeaders();
    if (data) headers["Content-Length"] = Buffer.byteLength(data);

    const req = lib.request({
      hostname: u.hostname, port: u.port || (u.protocol === "https:" ? 443 : 80),
      path: u.pathname + u.search, method, headers,
      agent: u.protocol === "https:" ? undefined : keepAliveAgent,
    }, (res) => {
      let buf = "";
      res.on("data", c => buf += c);
      res.on("end", () => resolve({ status: res.statusCode, body: buf }));
    });
    req.setTimeout(timeoutMs, () => { req.destroy(); reject(new Error("timeout")); });
    req.on("error", reject);
    if (data) req.write(data);
    req.end();
  });
}

// ─── Metrics polling ──────────────────────────────────────────────────────────
function requestWithBody(method, url, body, timeoutMs = 8000) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const lib = u.protocol === "https:" ? https : http;
    const data = body ? JSON.stringify(body) : null;
    const headers = authHeaders();
    if (data) headers["Content-Length"] = Buffer.byteLength(data);

    const req = lib.request({
      hostname: u.hostname, port: u.port || (u.protocol === "https:" ? 443 : 80),
      path: u.pathname + u.search, method, headers,
    }, (res) => {
      let buf = "";
      res.on("data", (c) => buf += c);
      res.on("end", () => resolve({ status: res.statusCode, body: buf }));
    });
    req.setTimeout(timeoutMs, () => { req.destroy(); reject(new Error("timeout")); });
    req.on("error", reject);
    if (data) req.write(data);
    req.end();
  });
}

// Get true process RSS in MB by finding the server process via pgrep and reading ps RSS.
// Falls back to null if not available. This matches what Activity Monitor / `ps` shows.
function getProcessRssMb() {
  try {
    const hostname = new URL(BASE_URL).hostname;
    if (hostname !== "localhost" && hostname !== "127.0.0.1") return null;
    const port = new URL(BASE_URL).port || "8092";
    // Find the PID listening on the target port.
    const pidLine = execSync(
      `lsof -ti tcp:${port} 2>/dev/null | head -1`,
      { timeout: 2000, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }
    ).trim();
    if (!pidLine) return null;
    const pid = parseInt(pidLine, 10);
    if (!pid) return null;
    const rssKb = execSync(
      `ps -o rss= -p ${pid} 2>/dev/null`,
      { timeout: 2000, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }
    ).trim();
    if (!rssKb) return null;
    return parseInt(rssKb, 10) / 1024; // kB → MB
  } catch (_) {
    return null;
  }
}

async function getNodeMetrics() {
  // Get actor count from dashboard API, but use real process RSS from `ps`.
  const rssMb = getProcessRssMb();
  try {
    const res = await requestWithBody("GET", `${BASE_URL}/api/v1/dashboard/nodes`, null, 8000);
    if (res.status === 200) {
      const body = JSON.parse(res.body);
      const nodes = body.nodes || [];
      for (const node of nodes) {
        const m = node.metrics || {};
        if (m.active_actors !== undefined) {
          return {
            rssMb: rssMb !== null ? rssMb : (m.memory_used_bytes > 0 ? m.memory_used_bytes / (1024 * 1024) : null),
            actors: typeof m.active_actors === "number" ? m.active_actors : null,
          };
        }
      }
    }
  } catch (_) {}
  return { rssMb, actors: null };
}

// ─── Main ─────────────────────────────────────────────────────────────────────
async function main() {
  console.log(
    `[actor_capacity] lang=${LANG}  mode=${ACTOR_MODE}  app_id=${APP_ID}  actor_type=${ACTOR_TYPE}\n` +
    `  concurrency=${CONCURRENCY}  base_url=${BASE_URL}\n` +
    `  memory_limit=${MEMORY_LIMIT_MB}MB  stop_at=${STOP_AT_MB}MB  max_duration=${MAX_DURATION_S}s`
  );

  // Wait for server.
  let attempts = 0;
  while (attempts < 20) {
    try {
      const res = await requestWithBody("GET", `${BASE_URL}/api/v1/dashboard/nodes`, null, 3000);
      if (res.status < 500) break;
    } catch (_) {}
    attempts++;
    await new Promise(r => setTimeout(r, 1000));
  }

  const baseline = await getNodeMetrics();
  const baselineRss = baseline.rssMb || 0;
  const baselineActors = baseline.actors || 0;
  console.log(`[actor_capacity] baseline rss=${baselineRss.toFixed(0)}MB  actors=${baselineActors}`);

  // Diagnostic probe: fire one test tell and print full response before starting workers.
  {
    const probeUrl = `${BASE_URL}/api/v1/actors/${APP_ID}/probe-diag-0:${ACTOR_TYPE}`;
    try {
      const pr = await request("POST", probeUrl, { op: "echo", payload: "probe" }, 10000);
      console.log(`[actor_capacity] PROBE tell → HTTP ${pr.status}  body=${pr.body.slice(0, 200)}`);
      if (pr.status !== 200) {
        console.log(`[actor_capacity] WARNING: tell returned non-200 — check APP_ID="${APP_ID}" ACTOR_TYPE="${ACTOR_TYPE}" and server virtual actor registration`);
      }
    } catch (e) {
      console.log(`[actor_capacity] PROBE error: ${e.message}`);
    }
  }

  let stopped = false;
  let stopReason = `max_duration ${MAX_DURATION_S}s elapsed`;
  let peakActors = 0;
  let peakRss = baselineRss;
  let spawnedOk = 0;
  let spawnedErr = 0;
  const startTs = Date.now();

  const timeoutHandle = setTimeout(() => {
    stopped = true;
    stopReason = `max_duration ${MAX_DURATION_S}s elapsed`;
  }, MAX_DURATION_S * 1000);

  // ─── Monitor loop ───────────────────────────────────────────────────────────
  let lastActorCount = baselineActors;
  let lastKnownActors = baselineActors;  // keep last successful value to show when poll times out
  const monitorHandle = setInterval(async () => {
    if (stopped) return;
    const m = await getNodeMetrics();
    const rss = m.rssMb;
    // Use last known actor count when dashboard times out under load
    const actors = m.actors !== null ? m.actors : lastKnownActors;
    const actorStale = m.actors === null;

    if (actors !== null && actors > peakActors) peakActors = actors;
    if (rss !== null && rss > peakRss) peakRss = rss;
    if (m.actors !== null) lastKnownActors = m.actors;

    const elapsed = ((Date.now() - startTs) / 1000).toFixed(0);
    const staleTag = actorStale ? "~" : " ";  // ~ means value is stale (last known)
    const delta = (actors !== null && lastActorCount !== null) ? `  Δ=${actors - lastActorCount}/poll` : "";
    lastActorCount = actors;

    let ohStr = "";
    if (rss !== null && actors !== null && actors > 0 && rss > baselineRss) {
      const oh = ((rss - baselineRss) * 1024) / actors;
      ohStr = `  overhead=${oh.toFixed(1)}KB/actor`;
    }

    const rps = elapsed > 0 ? (spawnedOk / elapsed).toFixed(0) : "?";
    // Print on new line every poll so history is visible (not overwritten)
    const ts = new Date().toISOString().replace("T"," ").slice(0,19);
    console.log(
      `[${ts}] actors=${staleTag}${String(actors ?? "?").padEnd(7)} rss=${rss !== null ? rss.toFixed(0) : "?"}MB` +
      `${delta}${ohStr}  told_ok=${spawnedOk}  err=${spawnedErr}  rps=${rps}`
    );

    if (rss !== null && rss > STOP_AT_MB) {
      console.log(`\n[actor_capacity] STOP: RSS ${rss.toFixed(0)}MB > ${STOP_AT_MB}MB threshold`);
      stopped = true;
      stopReason = `RSS ${rss.toFixed(0)}MB > ${STOP_AT_MB}MB (${STOP_PERCENT}% of ${MEMORY_LIMIT_MB}MB)`;
    }
  }, POLL_INTERVAL_MS);

  // ─── Worker loops ───────────────────────────────────────────────────────────
  // Each worker fires /tell to a new unique actor name and loops immediately.
  // /tell returns 202 after queueing — no waiting for actor init or reply.
  // Virtual actor manager auto-activates the actor on first message (eager strategy).
  let iter = 0;

  let firstErrLogged = 0;

  async function workerLoop() {
    while (!stopped) {
      const myIter = iter++;
      const actorName = `cap-${ACTOR_MODE}-${myIter}`;
      // POST without /ask suffix = cast (fire-and-forget tell).
      // Returns 200 with {success:true} immediately after queuing the message.
      // The virtual actor manager auto-activates the actor (eager strategy) on first message.
      const url = `${BASE_URL}/api/v1/actors/${APP_ID}/${actorName}:${ACTOR_TYPE}`;
      try {
        const res = await request("POST", url, { op: "echo", payload: "cap" }, 10000);
        if (res.status === 200) {
          spawnedOk++;
        } else {
          spawnedErr++;
          if (firstErrLogged < 3) {
            firstErrLogged++;
            console.log(`\n[actor_capacity] ERROR HTTP ${res.status} for ${actorName}: ${res.body.slice(0, 300)}`);
          }
        }
      } catch (e) {
        spawnedErr++;
        if (firstErrLogged < 3) {
          firstErrLogged++;
          console.log(`\n[actor_capacity] ERROR (exception) for ${actorName}: ${e.message}`);
        }
      }
    }
  }

  const workers = [];
  for (let i = 0; i < CONCURRENCY; i++) workers.push(workerLoop());
  await Promise.all(workers);

  clearTimeout(timeoutHandle);
  clearInterval(monitorHandle);

  const final = await getNodeMetrics();
  const finalActors = final.actors !== null ? final.actors : (lastKnownActors > 0 ? lastKnownActors : peakActors);
  const finalRss = final.rssMb !== null ? final.rssMb : peakRss;
  const elapsed = ((Date.now() - startTs) / 1000).toFixed(0);
  const peakForOverhead = Math.max(peakActors, finalActors);
  const overheadKb = (finalRss > baselineRss && peakForOverhead > 0)
    ? ((finalRss - baselineRss) * 1024 / peakForOverhead).toFixed(1)
    : "–";

  const totalAttempted = spawnedOk + spawnedErr;
  const rps = elapsed > 0 ? (spawnedOk / elapsed).toFixed(0) : "?";
  const errRatePct = totalAttempted > 0 ? (100 * spawnedErr / totalAttempted).toFixed(2) : "0.00";

  const HR = "═".repeat(63);
  console.log("\n");
  console.log(`╔${HR}╗`);
  console.log(`║  TEST: actor/capacity  │ LANG: ${LANG.padEnd(12)} MODE: ${ACTOR_MODE.padEnd(10)} ║`);
  console.log(`║  concurrency: ${String(CONCURRENCY).padEnd(6)}  │ memory_limit: ${MEMORY_LIMIT_MB}MB  │ duration: ${elapsed}s  ║`);
  console.log(`╠${HR}╣`);
  if (isVirtual) {
    console.log(`║  VIRTUAL ACTOR CAPACITY (eager activation, LRU pool eviction) ║`);
    console.log(`║    peak_live_actors:      ${String(peakActors).padEnd(12)} (server-reported peak)   ║`);
    console.log(`║    final_live_actors:     ${String(finalActors).padEnd(12)} (final poll)             ║`);
    console.log(`║    peak_rss:              ${(peakRss.toFixed ? String(peakRss.toFixed(0)) : String(peakRss)).padEnd(8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)          ║`);
    console.log(`║    overhead_per_actor:    ${String(overheadKb).padEnd(8)} KB  (marginal, w/ baseline)  ║`);
  } else {
    console.log(`║  REGULAR ACTOR CAPACITY (no eviction, accumulate to OOM)      ║`);
    console.log(`║    max_live_actors:       ${String(peakActors).padEnd(12)} (server-reported peak)   ║`);
    console.log(`║    final_live_actors:     ${String(finalActors).padEnd(12)} (final poll)             ║`);
    console.log(`║    peak_rss:              ${(peakRss.toFixed ? String(peakRss.toFixed(0)) : String(peakRss)).padEnd(8)} MB  (limit: ${MEMORY_LIMIT_MB} MB)          ║`);
    console.log(`║    overhead_per_actor:    ${String(overheadKb).padEnd(8)} KB  (marginal, w/ baseline)  ║`);
  }
  console.log(`╠${HR}╣`);
  console.log(`║  THROUGHPUT                                                   ║`);
  console.log(`║    tells_confirmed:       ${String(spawnedOk).padEnd(12)} (HTTP 202 accepted)       ║`);
  console.log(`║    tell_errors:           ${String(spawnedErr).padEnd(12)} (non-202 / timeout)       ║`);
  console.log(`║    spawn_rps:             ${String(rps).padEnd(12)} actors/s (tells/s)           ║`);
  console.log(`║    error_rate:            ${errRatePct.padEnd(8)} %                              ║`);
  console.log(`╠${HR}╣`);
  console.log(`║  STOPPED: ${stopReason.slice(0, 52).padEnd(52)} ║`);
  console.log(`╚${HR}╝`);
}

main().catch(e => { console.error(e); process.exit(1); });
