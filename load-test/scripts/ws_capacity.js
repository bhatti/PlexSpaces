#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// ws_capacity.js — Find the maximum number of concurrent WebSocket thin-node connections.
//
// Strategy:
//   - Open WS connections in batches of BATCH_SIZE every RAMP_INTERVAL_MS.
//     Each connection does the thin-node registration handshake and then holds open,
//     sending heartbeats every HB_INTERVAL_MS.
//   - A monitor loop polls /api/v1/dashboard/nodes to track RSS + server-side
//     active connection count (plexspaces_ws_thin_nodes_active via metrics endpoint).
//   - When RSS > STOP_AT_MB or connection error rate spikes, the monitor stops the ramp.
//   - After stopping, holds remaining connections for HOLD_DURATION_S then closes all.
//   - Final report: max concurrent connections, peak RSS.
//
// Prerequisites:
//   npm install   (installs the 'ws' package)
//
// Usage:
//   node ws_capacity.js
//   BATCH_SIZE=100 TARGET=10000 MEMORY_LIMIT_MB=4096 node ws_capacity.js
//   WS_URL=ws://localhost:8092/ws node ws_capacity.js
//
// Environment variables:
//   WS_URL           WebSocket endpoint URL   (default: ws://localhost:8092/ws)
//   BASE_URL         HTTP endpoint for polls  (default: http://localhost:8092)
//   TARGET           Max connections to open  (default: 50000)
//   BATCH_SIZE       Connections per ramp step (default: 100)
//   RAMP_INTERVAL_MS Delay between batches ms  (default: 500)
//   HB_INTERVAL_MS   Heartbeat period ms       (default: 10000)
//   HOLD_DURATION_S  Hold after ramp stops s   (default: 10)
//   MEMORY_LIMIT_MB  Hard stop threshold MB    (default: 2048)
//   STOP_PERCENT     % of MEMORY_LIMIT to stop (default: 90)
//   POLL_INTERVAL_MS Monitor poll interval ms  (default: 3000)
//   MAX_DURATION_S   Wall clock timeout secs   (default: 1800 = 30m)
//   NO_AUTH          Set to 1 for no auth      (default: 1)
//   AUTH_TOKEN       Bearer token if auth on   (default: "")

"use strict";
const WebSocket = require("ws");
const http  = require("http");
const https = require("https");
const { execSync } = require("child_process");

// ─── Config ───────────────────────────────────────────────────────────────────
const WS_URL          = process.env.WS_URL   || "ws://localhost:8092/ws";
const BASE_URL        = process.env.BASE_URL || "http://localhost:8092";
const TARGET          = parseInt(process.env.TARGET           || "50000");
const BATCH_SIZE      = parseInt(process.env.BATCH_SIZE       || "100");
const RAMP_INTERVAL_MS = parseInt(process.env.RAMP_INTERVAL_MS || "500");
const HB_INTERVAL_MS  = parseInt(process.env.HB_INTERVAL_MS  || "10000");
const HOLD_DURATION_S = parseInt(process.env.HOLD_DURATION_S || "10");
const MEMORY_LIMIT_MB = parseInt(process.env.MEMORY_LIMIT_MB || "2048");
const STOP_PERCENT    = parseInt(process.env.STOP_PERCENT    || "100");
const STOP_AT_MB      = Math.floor(MEMORY_LIMIT_MB * STOP_PERCENT / 100);
const POLL_INTERVAL_MS = parseInt(process.env.POLL_INTERVAL_MS || "3000");
const MAX_DURATION_S  = parseInt(process.env.MAX_DURATION_S  || "1800");
const NO_AUTH         = process.env.NO_AUTH !== "0";
const AUTH_TOKEN      = process.env.AUTH_TOKEN || "";

// ─── Protobuf helpers (same encoding as ws_connections.js k6 scenario) ────────
function encodeVarint(v) {
  const bytes = [];
  while (true) {
    const bits = v & 0x7f;
    v = (v >>> 7);
    if (v !== 0) bytes.push(bits | 0x80);
    else { bytes.push(bits); break; }
  }
  return bytes;
}
function encodeTag(fieldNum, wireType) { return encodeVarint((fieldNum << 3) | wireType); }

function encodeStringField(fieldNum, str) {
  const bytes = [];
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i);
    if (c < 0x80) bytes.push(c);
    else if (c < 0x800) bytes.push(0xC0 | (c >> 6), 0x80 | (c & 0x3F));
    else bytes.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F));
  }
  return [...encodeTag(fieldNum, 2), ...encodeVarint(bytes.length), ...bytes];
}
function encodeEmbeddedField(fieldNum, bodyBytes) {
  return [...encodeTag(fieldNum, 2), ...encodeVarint(bodyBytes.length), ...bodyBytes];
}
function encodeVarintField(fieldNum, v) { return [...encodeTag(fieldNum, 0), ...encodeVarint(v)]; }

function makeRegisterFrame(requestId) {
  const reg   = [...encodeVarintField(10, 2)];                  // node_role = THIN
  const frame = [...encodeStringField(1, requestId), ...encodeEmbeddedField(20, reg)];
  return Buffer.from(frame);
}
function makeHeartbeatFrame(requestId, nodeId) {
  const hb    = [...encodeStringField(1, requestId), ...encodeStringField(2, nodeId)];
  const frame = [...encodeStringField(1, requestId), ...encodeEmbeddedField(24, hb)];
  return Buffer.from(frame);
}

function parseAssignedNodeId(buf) {
  try {
    const bytes = buf instanceof Buffer ? buf : Buffer.from(buf);
    let offset = 0;
    let assignedNodeId = "";
    while (offset < bytes.length) {
      let tag = 0, shift = 0;
      while (offset < bytes.length) {
        const b = bytes[offset++];
        tag |= (b & 0x7f) << shift; shift += 7;
        if ((b & 0x80) === 0) break;
      }
      const fieldNum = tag >>> 3;
      const wireType = tag & 0x7;
      if (wireType === 0) {
        while (offset < bytes.length) { const b = bytes[offset++]; if ((b & 0x80) === 0) break; }
      } else if (wireType === 2) {
        let len = 0, shift2 = 0;
        while (offset < bytes.length) { const b = bytes[offset++]; len |= (b & 0x7f) << shift2; shift2 += 7; if ((b & 0x80) === 0) break; }
        const start = offset; offset += len;
        if (fieldNum === 21) {
          // node_register_ack — parse inner
          const ack = bytes.slice(start, start + len);
          let ao = 0;
          while (ao < ack.length) {
            let at = 0, as2 = 0;
            while (ao < ack.length) { const b = ack[ao++]; at |= (b & 0x7f) << as2; as2 += 7; if ((b & 0x80) === 0) break; }
            const af = at >>> 3, aw = at & 0x7;
            if (aw === 0) { while (ao < ack.length) { const b = ack[ao++]; if ((b & 0x80) === 0) break; } }
            else if (aw === 2) {
              let al = 0, as3 = 0;
              while (ao < ack.length) { const b = ack[ao++]; al |= (b & 0x7f) << as3; as3 += 7; if ((b & 0x80) === 0) break; }
              if (af === 2) assignedNodeId = ack.slice(ao, ao + al).toString("utf8");
              ao += al;
            } else break;
          }
        }
      } else break;
    }
    return assignedNodeId || null;
  } catch (_) { return null; }
}

// ─── HTTP helper ──────────────────────────────────────────────────────────────
function httpGet(url, timeoutMs = 8000) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const lib = u.protocol === "https:" ? https : http;
    const headers = {};
    if (NO_AUTH) headers["x-tenant-id"] = "default";
    else if (AUTH_TOKEN) headers["Authorization"] = `Bearer ${AUTH_TOKEN}`;
    else headers["x-tenant-id"] = "default";

    const req = lib.get({ hostname: u.hostname, port: u.port, path: u.pathname + u.search, headers }, (res) => {
      let buf = "";
      res.on("data", c => buf += c);
      res.on("end", () => resolve({ status: res.statusCode, body: buf }));
    });
    req.setTimeout(timeoutMs, () => { req.destroy(); reject(new Error("timeout")); });
    req.on("error", reject);
  });
}

function getProcessRssMb() {
  try {
    const hostname = new URL(BASE_URL).hostname;
    if (hostname !== "localhost" && hostname !== "127.0.0.1") return null;
    const port = new URL(BASE_URL).port || "8092";
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
    return parseInt(rssKb, 10) / 1024;
  } catch (_) {
    return null;
  }
}

async function getNodeMetrics() {
  return { rssMb: getProcessRssMb() };
}

async function getServerWsActive() {
  try {
    const res = await httpGet(`${BASE_URL}/api/v1/dashboard/metrics-table`);
    if (res.status === 200) {
      const body = JSON.parse(res.body);
      for (const m of (body.metrics || [])) {
        if (m.name === "plexspaces_ws_thin_nodes_active") return m.value;
      }
    }
  } catch (_) {}
  return null;
}

// ─── Connection lifecycle ─────────────────────────────────────────────────────
// connState: { ws, nodeId, hbTimer, closed }
// openConnection returns { state, errorType } where errorType is null on success,
// "emfile" for fd-exhaustion (client limit, not server), or "server" for server rejection.
function openConnection(idx) {
  return new Promise((resolve) => {
    const requestId = `reg-${idx}-${Date.now()}`;
    const wsHeaders = {};
    if (NO_AUTH) wsHeaders["x-tenant-id"] = "default";
    else if (AUTH_TOKEN) wsHeaders["Authorization"] = `Bearer ${AUTH_TOKEN}`;
    else wsHeaders["x-tenant-id"] = "default";

    let ws;
    try {
      ws = new WebSocket(WS_URL, { headers: wsHeaders });
    } catch (e) {
      // Synchronous throw (rare) — treat as client error.
      resolve({ state: null, errorType: "client" });
      return;
    }
    ws.binaryType = "nodebuffer";

    const state = { ws, nodeId: null, hbTimer: null, closed: false };

    const timeout = setTimeout(() => {
      if (!state.closed) { state.closed = true; ws.terminate(); resolve({ state: null, errorType: "timeout" }); }
    }, 10000);

    ws.on("open", () => {
      ws.send(makeRegisterFrame(requestId));
    });

    ws.on("message", (data) => {
      if (state.nodeId) return;
      const nid = parseAssignedNodeId(data);
      if (nid) {
        clearTimeout(timeout);
        state.nodeId = nid;
        state.hbTimer = setInterval(() => {
          if (state.closed || ws.readyState !== WebSocket.OPEN) {
            clearInterval(state.hbTimer);
            return;
          }
          const hbId = `hb-${nid}-${Date.now()}`;
          ws.send(makeHeartbeatFrame(hbId, nid));
        }, HB_INTERVAL_MS);
        resolve({ state, errorType: null });
      }
    });

    ws.on("error", (err) => {
      clearTimeout(timeout);
      if (!state.closed) {
        state.closed = true;
        // EMFILE = too many open files (client fd limit, not a server rejection)
        const isEmfile = err && (err.code === "EMFILE" || err.code === "ENFILE" ||
          (err.message && err.message.includes("EMFILE")));
        resolve({ state: null, errorType: isEmfile ? "emfile" : "server" });
      }
    });
    ws.on("close", () => {
      state.closed = true;
      if (state.hbTimer) clearInterval(state.hbTimer);
    });
  });
}

function closeConnection(state) {
  if (!state || state.closed) return;
  state.closed = true;
  if (state.hbTimer) clearInterval(state.hbTimer);
  try { state.ws.close(1000); } catch (_) {}
}

// ─── Main ─────────────────────────────────────────────────────────────────────
async function main() {
  console.log(
    `[ws_capacity] ws_url=${WS_URL}  target=${TARGET}  batch_size=${BATCH_SIZE}` +
    `  ramp_interval=${RAMP_INTERVAL_MS}ms  memory_limit=${MEMORY_LIMIT_MB}MB` +
    `  stop_at=${STOP_AT_MB}MB  max_duration=${MAX_DURATION_S}s`
  );

  // Wait for server.
  for (let i = 0; i < 20; i++) {
    try { const r = await httpGet(`${BASE_URL}/api/v1/dashboard/nodes`, 3000); if (r.status < 500) break; }
    catch (_) {}
    await new Promise(r => setTimeout(r, 1000));
  }

  const baseline = await getNodeMetrics();
  const baselineRss = baseline.rssMb || 0;
  console.log(`[ws_capacity] baseline rss=${baselineRss.toFixed(0)}MB`);

  const conns = [];          // Array<connState>
  let stopped = false;
  let stopReason = `target (${TARGET}) reached`;
  let peakConns = 0;
  let peakRss = baselineRss;
  let peakServerWs = 0;
  let connErrors = 0;       // server-side rejections (not client fd exhaustion)
  let emfileErrors = 0;     // client fd-exhaustion errors (ulimit -n too low)
  let consecutiveServerFails = 0;  // stop if server is consistently rejecting
  const startTs = Date.now();

  // Wall-clock timeout.
  const timeoutHandle = setTimeout(() => {
    stopped = true;
    stopReason = `max_duration ${MAX_DURATION_S}s elapsed`;
  }, MAX_DURATION_S * 1000);

  // Monitor loop: polls RSS + server WS active.
  const monitorHandle = setInterval(async () => {
    const [nm, serverWs] = await Promise.all([getNodeMetrics(), getServerWsActive()]);
    const rss = nm.rssMb;
    const live = conns.filter(c => c && !c.closed).length;

    if (rss !== null && rss > peakRss) peakRss = rss;
    if (serverWs !== null && serverWs > peakServerWs) peakServerWs = serverWs;

    const elapsed = ((Date.now() - startTs) / 1000).toFixed(0);
    const ts = new Date().toISOString().replace("T"," ").slice(0,19);
    const emfileNote = emfileErrors > 0 ? `  emfile(client)=${emfileErrors}` : "";
    console.log(
      `[${ts}] ws_conns=${String(live).padEnd(8)} server_ws=${String(serverWs ?? "?").padEnd(8)} rss=${rss !== null ? rss.toFixed(0) : "?"}MB  server_errs=${connErrors}${emfileNote}`
    );

    if (rss !== null && rss > STOP_AT_MB) {
      console.log(`\n[ws_capacity] STOP: RSS ${rss.toFixed(0)}MB > ${STOP_AT_MB}MB threshold`);
      stopped = true;
      stopReason = `RSS ${rss.toFixed(0)}MB > ${STOP_AT_MB}MB (${STOP_PERCENT}% of ${MEMORY_LIMIT_MB}MB)`;
    }
  }, POLL_INTERVAL_MS);

  // ─── Ramp loop ──────────────────────────────────────────────────────────────
  while (!stopped && conns.length < TARGET) {
    const batchTarget = Math.min(BATCH_SIZE, TARGET - conns.length);
    const promises = [];
    for (let i = 0; i < batchTarget; i++) {
      promises.push(openConnection(conns.length + i));
    }
    const results = await Promise.all(promises);
    let batchServerFails = 0;
    for (const r of results) {
      if (r && r.errorType === null && r.state) {
        conns.push(r.state);
      } else {
        conns.push({ closed: true }); // placeholder to keep indices consistent
        if (r && r.errorType === "emfile") {
          emfileErrors++;
          if (emfileErrors === 1) {
            console.log(`\n[ws_capacity] WARNING: EMFILE — client fd limit hit. Run with: ulimit -n 200000 && node ws_capacity.js`);
          }
        } else {
          connErrors++;
          batchServerFails++;
        }
      }
    }
    // If entire batch fails with server errors (not emfile), server is at capacity.
    if (batchServerFails > 0) {
      consecutiveServerFails++;
      if (consecutiveServerFails >= 3) {
        stopped = true;
        stopReason = `server rejecting connections (${connErrors} total server errors)`;
      }
    } else {
      consecutiveServerFails = 0;
    }
    const liveNow = conns.filter(c => c && !c.closed).length;
    if (liveNow > peakConns) peakConns = liveNow;

    if (!stopped) {
      await new Promise(r => setTimeout(r, RAMP_INTERVAL_MS));
    }
  }

  if (!stopped) stopReason = `target (${TARGET}) reached`;
  stopped = true;

  console.log(`\n[ws_capacity] ramp done. holding for ${HOLD_DURATION_S}s...`);
  // Final peak poll during hold.
  for (let i = 0; i < HOLD_DURATION_S; i++) {
    await new Promise(r => setTimeout(r, 1000));
    const [nm, sw] = await Promise.all([getNodeMetrics(), getServerWsActive()]);
    if (nm.rssMb !== null && nm.rssMb > peakRss) peakRss = nm.rssMb;
    if (sw !== null && sw > peakServerWs) peakServerWs = sw;
  }

  clearTimeout(timeoutHandle);
  clearInterval(monitorHandle);

  // Close all connections.
  console.log(`[ws_capacity] closing ${conns.filter(c => c && !c.closed).length} connections...`);
  for (const c of conns) closeConnection(c);
  await new Promise(r => setTimeout(r, 2000)); // let close frames flush

  const elapsed = ((Date.now() - startTs) / 1000).toFixed(0);
  const totalAttempted = conns.length;
  const totalOk = conns.filter(c => c && c.nodeId).length;
  const errRate = totalAttempted > 0 ? (100 * connErrors / totalAttempted).toFixed(2) : "0.00";
  const overheadKbPerConn = (peakRss > baselineRss && peakConns > 0)
    ? ((peakRss - baselineRss) * 1024 / peakConns).toFixed(1)
    : "–";

  const HR = "═".repeat(63);
  console.log("\n");
  console.log(`╔${HR}╗`);
  console.log(`║  TEST: ws/capacity                                            ║`);
  console.log(`║  batch_size: ${String(BATCH_SIZE).padEnd(6)}  ramp_interval: ${RAMP_INTERVAL_MS}ms  duration: ${elapsed}s       ║`);
  console.log(`╠${HR}╣`);
  console.log(`║  CAPACITY RESULTS                                             ║`);
  console.log(`║    max_concurrent_thin_nodes: ${String(peakServerWs || peakConns).padEnd(8)} (Prometheus peak, server-reported)║`);
  console.log(`║    max_local_open_conns:   ${String(peakConns).padEnd(10)} (client-side peak)        ║`);
  console.log(`║    total_registered_ok:    ${String(totalOk).padEnd(10)} (completed handshake)      ║`);
  console.log(`║    server_errors:          ${String(connErrors).padEnd(10)} (timeout / refused by server) ║`);
  console.log(`║    client_emfile_errors:   ${String(emfileErrors).padEnd(10)} (fd exhaustion — raise ulimit) ║`);
  console.log(`║    peak_rss:               ${(typeof peakRss === "number" ? peakRss.toFixed(0) : String(peakRss)).padEnd(6)} MB  (limit: ${MEMORY_LIMIT_MB} MB)          ║`);
  console.log(`║    overhead_per_conn:      ${String(overheadKbPerConn).padEnd(6)} KB  (marginal, w/ baseline)  ║`);
  console.log(`║    server_error_rate:      ${errRate}%                                    ║`);
  console.log(`╠${HR}╣`);
  console.log(`║  STOPPED: ${stopReason.slice(0, 52).padEnd(52)} ║`);
  console.log(`╚${HR}╝`);
}

main().catch(e => { console.error(e); process.exit(1); });
