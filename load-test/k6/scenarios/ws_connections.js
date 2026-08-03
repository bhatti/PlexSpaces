// SPDX-License-Identifier: AGPL-3.0-or-later
// ws_connections.js — WebSocket connection capacity test (rust-embedded only).
//
// Ramps VU count (each VU = one persistent WebSocket connection) through escalating
// levels to find the server's maximum concurrent connection capacity.
//
// Each VU:
//   1. Upgrades to WebSocket on /api/v1/ws
//   2. Sends a ping every PING_INTERVAL_MS and waits for pong
//   3. Holds the connection open for the stage duration
//   4. Reports: upgrade success, pong received, unexpected drop
//
// Scale levels: 50 → 100 → 250 → 500 → 1000 → 2000 → 5000 connections.
// Each level held for STAGE_DURATION (default 30s). Ramp time between levels: 5s.
//
// Run:
//   k6 run -e STAGE_DURATION=30s \
//          load-test/k6/scenarios/ws_connections.js

import ws from "k6/ws";
import { check, sleep } from "k6";
import { Rate, Counter, Gauge, Trend } from "k6/metrics";
import { authHeaders, EMBEDDED_URL } from "../common.js";

const STAGE_DURATION  = __ENV.STAGE_DURATION  || "30s";
const PING_INTERVAL_MS = parseInt(__ENV.PING_INTERVAL_MS || "5000");
// Connection upgrade timeout — how long to wait for server to accept the WebSocket upgrade.
const CONN_TIMEOUT_S  = parseInt(__ENV.CONN_TIMEOUT_S || "10");
// Max VU level for the ramp. Lower for quick smoke runs.
const MAX_LEVEL       = parseInt(__ENV.MAX_LEVEL || "2000");

// Convert http(s):// to ws(s):// for WebSocket URL.
const WS_URL = (EMBEDDED_URL || "http://localhost:8092").replace(/^http(s?):\/\//, "ws$1://") + "/api/v1/ws";

// Custom metrics
const connErrorRate   = new Rate("ws_conn_error_rate");
const connDropRate    = new Rate("ws_conn_drop_rate");
const pongErrorRate   = new Rate("ws_pong_error_rate");
const totalConns      = new Counter("ws_conn_total");
const activeConns     = new Gauge("ws_conn_active");
const connLatency     = new Trend("ws_conn_upgrade_ms", true);
const pongLatency     = new Trend("ws_pong_latency_ms", true);

// Ramp levels — stop at MAX_LEVEL
const ALL_LEVELS = [50, 100, 250, 500, 1000, 2000, 5000];
const LEVELS = ALL_LEVELS.filter(l => l <= MAX_LEVEL);

function buildStages() {
  const stages = [];
  for (let i = 0; i < LEVELS.length; i++) {
    stages.push({ duration: "5s",           target: LEVELS[i] });   // ramp up
    stages.push({ duration: STAGE_DURATION, target: LEVELS[i] });   // hold
  }
  stages.push({ duration: "10s", target: 0 });  // ramp down
  return stages;
}

export const options = {
  scenarios: {
    ws_connections: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: buildStages(),
      gracefulRampDown: "15s",
    },
  },
  thresholds: {
    "ws_conn_error_rate":  ["rate<0.10"],  // allow up to 10% upgrade failures (expected near ceiling)
    "ws_conn_drop_rate":   ["rate<0.10"],
    "ws_pong_error_rate":  ["rate<0.10"],
  },
};

// Parse k6 duration strings to milliseconds. Handles compounds like "2m30s".
function parseDurationMs(d) {
  if (!d) return 30000;
  if (d.endsWith("ms")) return parseInt(d, 10) || 30000;
  let ms = 0;
  const h = d.match(/(\d+)h/); if (h) ms += parseInt(h[1]) * 3600000;
  const m = d.match(/(\d+)m/); if (m) ms += parseInt(m[1]) * 60000;
  const s = d.match(/(\d+)s/); if (s) ms += parseInt(s[1]) * 1000;
  return ms > 0 ? ms : 30000;
}

const stageDurationMs = parseDurationMs(STAGE_DURATION);

export default function () {
  let upgraded        = false;
  let closeInitiated  = false;   // true when we requested the close
  let dropped         = false;
  let pongCount       = 0;
  let pongErrors      = 0;
  let lastPingSent    = 0;

  const upgradeStart = Date.now();

  const headers = authHeaders();
  // WebSocket does not use Content-Type — strip it and keep auth/tenant headers.
  delete headers["Content-Type"];

  const res = ws.connect(WS_URL, { headers, timeout: `${CONN_TIMEOUT_S}s` }, function (socket) {
    socket.on("open", function () {
      upgraded = true;
      connLatency.add(Date.now() - upgradeStart);
      activeConns.add(1);

      // Send pings on a fixed interval; track round-trip
      socket.setInterval(function () {
        lastPingSent = Date.now();
        socket.send(JSON.stringify({ type: "ping", vu: __VU, ts: lastPingSent }));
      }, PING_INTERVAL_MS);
    });

    socket.on("message", function (data) {
      try {
        const msg = JSON.parse(data);
        if (msg.type === "pong" || msg.type === "ping") {
          pongCount++;
          if (lastPingSent > 0) {
            pongLatency.add(Date.now() - lastPingSent);
            lastPingSent = 0;
          }
        }
      } catch (_) {}
    });

    socket.on("close", function () {
      if (upgraded) {
        activeConns.add(-1);
        // Only count as a drop if the close was NOT initiated by us.
        if (!closeInitiated) {
          dropped = true;
        }
      }
    });

    socket.on("error", function (_e) {
      pongErrors++;
    });

    // Hold connection open for one stage's worth of time, then close cleanly.
    socket.setTimeout(function () {
      closeInitiated = true;
      socket.close();
    }, stageDurationMs + 1000);
  });

  const upgradeOk = check(res, {
    "WebSocket upgrade 101": (r) => r && r.status === 101,
  });

  connErrorRate.add(!upgradeOk || !upgraded);
  connDropRate.add(dropped);
  pongErrorRate.add(pongErrors > 0);
  totalConns.add(1);
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

  const durSec     = (data.state.testRunDurationMs / 1000).toFixed(0);
  const totalC     = data.metrics["ws_conn_total"]?.values?.count ?? 0;
  const maxActive  = data.metrics["ws_conn_active"]?.values?.max ?? 0;
  const errRate    = ((data.metrics["ws_conn_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const dropRate   = ((data.metrics["ws_conn_drop_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const pongErrR   = ((data.metrics["ws_pong_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const cl         = data.metrics["ws_conn_upgrade_ms"];
  const pl         = data.metrics["ws_pong_latency_ms"];

  const lines = [
    "",
    `╔${HR}╗`,
    row(`TEST: ws/capacity  │ TARGET: rust-embedded  │ MAX_LEVEL: ${MAX_LEVEL}`),
    row(`Levels: ${LEVELS.join("→")}  VUs/conns each`),
    row(`Stage duration: ${STAGE_DURATION}  │  Total run: ${durSec}s  │  Total conns: ${totalC}`),
    `╠${HR}╣`,
    row("CONNECTION CAPACITY"),
    row(`  max_concurrent_conns: ${maxActive}`),
    row(`  upgrade_error_rate:   ${errRate}%   (>10% = server at limit)`),
    row(`  unexpected_drop_rate: ${dropRate}%`),
    row(`  pong_error_rate:      ${pongErrR}%`),
    `╠${HR}╣`,
    row("LATENCY"),
    row("  WebSocket upgrade (time to 101 response):"),
    cl
      ? row(`    p50=${rp(fmtMs(cl.values.med),8)} p95=${rp(fmtMs(cl.values["p(95)"]),8)} p99=${fmtMs(cl.values["p(99)"])}`)
      : row("    (no data)"),
    row("  Ping-pong round-trip latency:"),
    pl
      ? row(`    p50=${rp(fmtMs(pl.values.med),8)} p95=${rp(fmtMs(pl.values["p(95)"]),8)} p99=${fmtMs(pl.values["p(99)"])}`)
      : row("    (no pong data — server may not echo pings)"),
  ];

  const bottleneck = +errRate > 10 || +dropRate > 10
    ? `⚠ Capacity ceiling hit near ${maxActive} connections`
    : `✓ No capacity limit reached up to ${maxActive} connections`;
  lines.push(`╠${HR}╣`);
  lines.push(row(`VERDICT: ${bottleneck}`));
  lines.push(`╚${HR}╝`, "");

  return { stdout: lines.join("\n") };
}
