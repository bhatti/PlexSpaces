// SPDX-License-Identifier: AGPL-3.0-or-later
// ws_connections.js — WebSocket thin-node connection capacity test (rust-embedded only).
//
// Each VU connects to /ws, performs the thin-node registration handshake
// (WsFrame{node_register: NodeRegistration{node_role: THIN}}), then holds the
// connection open sending periodic heartbeats.  This follows the same protocol
// used by the integration tests in crates/node/tests/suite/ws_integration_tests.rs.
//
// Wire format: every WS binary frame is a prost-serialised WsFrame.
// A minimal protobuf encoder/decoder is implemented in pure JS below.
//
// Scale levels: 50 → 100 → 250 → 500 → 1000 → 2000 connections.
// Each level held for STAGE_DURATION (default 30s).
//
// Run:
//   k6 run -e STAGE_DURATION=30s \
//          load-test/k6/scenarios/ws_connections.js

import http from "k6/http";
import ws from "k6/ws";
import { check, sleep } from "k6";
import { Rate, Counter, Gauge, Trend } from "k6/metrics";
import { authHeaders, EMBEDDED_URL } from "../common.js";

const STAGE_DURATION   = __ENV.STAGE_DURATION   || "30s";
const HB_INTERVAL_MS   = parseInt(__ENV.HB_INTERVAL_MS  || "5000");
const CONN_TIMEOUT_S   = parseInt(__ENV.CONN_TIMEOUT_S  || "10");
const MAX_LEVEL        = parseInt(__ENV.MAX_LEVEL || "2000");

// /ws is the WebSocket endpoint (not under /api/v1).
const WS_URL = (EMBEDDED_URL || "http://localhost:8092")
  .replace(/^http(s?):\/\//, "ws$1://") + "/ws";

// Metrics
const connErrorRate  = new Rate("ws_conn_error_rate");
const connDropRate   = new Rate("ws_conn_drop_rate");
const hbErrorRate    = new Rate("ws_heartbeat_error_rate");
const totalConns     = new Counter("ws_conn_total");
// ws_conn_active: k6-side per-VU counter (1 = connected, 0 = closed).
// values.max = 1, since k6 VU state is not shared. Use server_ws_active for true peak.
const activeConns    = new Gauge("ws_conn_active");
// server_ws_active: polled from plexspaces_ws_thin_nodes_active Prometheus gauge.
// values.max = true peak concurrent connections on the server.
const serverWsActive = new Gauge("server_ws_active");
const connLatency    = new Trend("ws_conn_upgrade_ms",    true);
const regLatency     = new Trend("ws_reg_latency_ms",     true);
const hbLatency      = new Trend("ws_heartbeat_latency_ms", true);

const ALL_LEVELS = [50, 100, 250, 500, 1000, 2000];
const LEVELS = ALL_LEVELS.filter(l => l <= MAX_LEVEL);
const POLL_INTERVAL_MS = parseInt(__ENV.POLL_INTERVAL_MS || "2000");

function buildStages() {
  const stages = [];
  for (let i = 0; i < LEVELS.length; i++) {
    stages.push({ duration: "5s",           target: LEVELS[i] });
    stages.push({ duration: STAGE_DURATION, target: LEVELS[i] });
  }
  stages.push({ duration: "10s", target: 0 });
  return stages;
}

// Calculate total test duration in seconds for the poller scenario
function totalDurationS() {
  const sdMs = parseDurationMs(STAGE_DURATION);
  // each level: 5s ramp + STAGE_DURATION hold; plus 10s rampdown
  const ms = LEVELS.length * (5000 + sdMs) + 10000;
  return Math.ceil(ms / 1000);
}

export const options = {
  scenarios: {
    ws_connections: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: buildStages(),
      gracefulRampDown: "15s",
    },
    // Polls the server-side plexspaces_ws_thin_nodes_active gauge every POLL_INTERVAL_MS.
    // server_ws_active.values.max = true peak concurrent connections on the server.
    metrics_poller: {
      executor: "constant-vus",
      vus: 1,
      duration: `${totalDurationS()}s`,
      exec: "pollServerMetrics",
      gracefulStop: "5s",
    },
  },
  thresholds: {
    "ws_conn_error_rate":      ["rate<0.10"],
    "ws_conn_drop_rate":       ["rate<0.10"],
    "ws_heartbeat_error_rate": ["rate<0.10"],
  },
};

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

// ─── Minimal protobuf encoder ────────────────────────────────────────────────
// Supports: varint fields, length-delimited (string + embedded message) fields.
// Enough to encode WsFrame{request_id, node_register} and
// WsFrame{request_id, heartbeat}.

function encodeVarint(v) {
  const bytes = [];
  while (true) {
    const bits = v & 0x7f;
    v = (v >>> 7);
    if (v !== 0) {
      bytes.push(bits | 0x80);
    } else {
      bytes.push(bits);
      break;
    }
  }
  return bytes;
}

function encodeTag(fieldNum, wireType) {
  return encodeVarint((fieldNum << 3) | wireType);
}

// Encode a string field (wire type 2).
// Uses a manual UTF-8 encoder because TextEncoder is not available in k6 WS callbacks.
function encodeStringField(fieldNum, str) {
  const bytes = [];
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i);
    if (c < 0x80) {
      bytes.push(c);
    } else if (c < 0x800) {
      bytes.push(0xC0 | (c >> 6), 0x80 | (c & 0x3F));
    } else {
      bytes.push(0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F));
    }
  }
  return [...encodeTag(fieldNum, 2), ...encodeVarint(bytes.length), ...bytes];
}

// Encode an embedded message field (wire type 2).
function encodeEmbeddedField(fieldNum, bodyBytes) {
  return [
    ...encodeTag(fieldNum, 2),
    ...encodeVarint(bodyBytes.length),
    ...bodyBytes,
  ];
}

// Encode a varint field (wire type 0).
function encodeVarintField(fieldNum, v) {
  return [...encodeTag(fieldNum, 0), ...encodeVarint(v)];
}

function toUint8Array(arr) {
  return new Uint8Array(arr).buffer;
}

// Build WsFrame{request_id, node_register: NodeRegistration{node_role: THIN(2)}}.
// Empty node_id causes the server to assign a ULID (mirrors test_ws_server_assigns_node_id_when_empty).
function makeRegisterFrame(requestId) {
  const reg = [
    ...encodeVarintField(10, 2),   // node_role = NODE_ROLE_THIN
  ];
  const frame = [
    ...encodeStringField(1, requestId),    // WsFrame.request_id
    ...encodeEmbeddedField(20, reg),       // WsFrame.node_register
  ];
  return toUint8Array(frame);
}

// Build WsFrame{request_id, heartbeat: SendHeartbeatRequest{request_id, node_id}}.
function makeHeartbeatFrame(requestId, nodeId) {
  const hb = [
    ...encodeStringField(1, requestId),   // SendHeartbeatRequest.request_id
    ...encodeStringField(2, nodeId),      // SendHeartbeatRequest.node_id
  ];
  const frame = [
    ...encodeStringField(1, requestId),   // WsFrame.request_id
    ...encodeEmbeddedField(24, hb),       // WsFrame.heartbeat
  ];
  return toUint8Array(frame);
}

// ─── Minimal protobuf decoder ────────────────────────────────────────────────
// Reads WsFrame from a binary ArrayBuffer to extract assigned_node_id from
// the NodeRegisterAck payload.

function decodeVarint(bytes, offset) {
  let result = 0, shift = 0;
  while (offset < bytes.length) {
    const b = bytes[offset++];
    result |= (b & 0x7f) << shift;
    shift += 7;
    if ((b & 0x80) === 0) break;
  }
  return { value: result, offset };
}

function decodeString(bytes, offset, length) {
  const slice = bytes.slice(offset, offset + length);
  let s = "";
  for (let i = 0; i < slice.length; i++) {
    s += String.fromCharCode(slice[i]);
  }
  return s;
}

// Parse WsFrame and return { requestId, assignedNodeId } for ack frames.
// Returns null if parsing fails or frame is not a NodeRegisterAck.
function parseRegisterAck(buf) {
  try {
    const bytes = new Uint8Array(buf instanceof ArrayBuffer ? buf : buf.buffer);
    let offset = 0;
    let requestId = "";
    let assignedNodeId = "";

    while (offset < bytes.length) {
      const tagResult = decodeVarint(bytes, offset);
      offset = tagResult.offset;
      const tag = tagResult.value;
      const fieldNum = tag >>> 3;
      const wireType = tag & 0x7;

      if (wireType === 0) {
        // Varint field — skip
        const r = decodeVarint(bytes, offset);
        offset = r.offset;
      } else if (wireType === 2) {
        const lenResult = decodeVarint(bytes, offset);
        offset = lenResult.offset;
        const length = lenResult.value;
        const fieldStart = offset;
        offset += length;

        if (fieldNum === 1) {
          // WsFrame.request_id
          requestId = decodeString(bytes, fieldStart, length);
        } else if (fieldNum === 21) {
          // WsFrame.node_register_ack: WsNodeRegisterAck{success:1, assigned_node_id:2}
          const ackBytes = bytes.slice(fieldStart, fieldStart + length);
          let ao = 0;
          while (ao < ackBytes.length) {
            const at = decodeVarint(ackBytes, ao); ao = at.offset;
            const af = at.value >>> 3;
            const aw = at.value & 0x7;
            if (aw === 0) {
              const av = decodeVarint(ackBytes, ao); ao = av.offset;
            } else if (aw === 2) {
              const al = decodeVarint(ackBytes, ao); ao = al.offset;
              if (af === 2) {
                assignedNodeId = decodeString(ackBytes, ao, al.value);
              }
              ao += al.value;
            } else {
              break;
            }
          }
        } else {
          // Unknown field, already skipped via offset += length
        }
      } else {
        // Unsupported wire type — stop parsing
        break;
      }
    }

    if (assignedNodeId) return { requestId, assignedNodeId };
  } catch (_) {}
  return null;
}

// ─── VU body ─────────────────────────────────────────────────────────────────

export default function () {
  let upgraded        = false;
  let registered      = false;
  let closeInitiated  = false;
  let dropped         = false;
  let assignedNodeId  = "";
  let hbErrors        = 0;
  let regStart        = 0;
  let pendingHbStart  = 0;

  const upgradeStart = Date.now();
  const vuId = `ws-vu-${__VU}`;

  const headers = authHeaders();
  delete headers["Content-Type"];

  const res = ws.connect(WS_URL, { headers, timeout: `${CONN_TIMEOUT_S}s` }, function (socket) {
    socket.on("open", function () {
      upgraded = true;
      connLatency.add(Date.now() - upgradeStart);
      activeConns.add(1);

      // Step 1: send thin-node registration handshake immediately after upgrade.
      regStart = Date.now();
      socket.sendBinary(makeRegisterFrame(`reg-${__VU}`));
    });

    socket.on("binaryMessage", function (data) {
      if (!registered) {
        // Expecting NodeRegisterAck
        const ack = parseRegisterAck(data);
        if (ack && ack.assignedNodeId) {
          registered = true;
          assignedNodeId = ack.assignedNodeId;
          regLatency.add(Date.now() - regStart);

          // Step 2: start sending heartbeats on interval once registered.
          socket.setInterval(function () {
            if (!registered || closeInitiated) return;
            const hbId = `hb-${__VU}-${Date.now()}`;
            pendingHbStart = Date.now();
            socket.sendBinary(makeHeartbeatFrame(hbId, assignedNodeId));
          }, HB_INTERVAL_MS);
        } else {
          // Failed to parse ack — increment error
          hbErrors++;
        }
        return;
      }

      // For heartbeat acks: record round-trip if we have a pending start time.
      if (pendingHbStart > 0) {
        hbLatency.add(Date.now() - pendingHbStart);
        pendingHbStart = 0;
      }
    });

    socket.on("message", function (_data) {
      // Server sends binary frames only; ignore any text messages.
    });

    socket.on("close", function () {
      if (upgraded) {
        activeConns.add(0);
        if (!closeInitiated) {
          dropped = true;
        }
      }
    });

    socket.on("error", function (_e) {
      hbErrors++;
    });

    // Hold connection open for one stage + a small buffer, then close cleanly.
    socket.setTimeout(function () {
      closeInitiated = true;
      socket.close();
    }, stageDurationMs + 2000);
  });

  const upgradeOk = check(res, {
    "WebSocket upgrade 101": (r) => r && r.status === 101,
  });
  const regOk = check(null, {
    "thin-node registered": () => registered,
  });

  connErrorRate.add(!upgradeOk || !upgraded);
  connDropRate.add(dropped);
  hbErrorRate.add(hbErrors > 0 || !regOk);
  totalConns.add(1);
}

// Polls plexspaces_ws_thin_nodes_active from the server's Prometheus metrics-table.
// Runs as a separate scenario (metrics_poller) so values.max = true peak across the test.
export function pollServerMetrics() {
  for (;;) {
    try {
      const res = http.get(`${EMBEDDED_URL}/api/v1/dashboard/metrics-table`, {
        headers: authHeaders(),
        timeout: "5s",
        tags: { op: "metrics_poll" },
      });
      if (res.status === 200) {
        const body = JSON.parse(res.body);
        const metrics = body.metrics || [];
        for (let i = 0; i < metrics.length; i++) {
          if (metrics[i].name === "plexspaces_ws_thin_nodes_active") {
            serverWsActive.add(metrics[i].value);
            break;
          }
        }
      }
    } catch (_) {}
    sleep(POLL_INTERVAL_MS / 1000);
  }
}

// Read current plexspaces_ws_thin_nodes_active gauge from the server's metrics-table.
// Called from handleSummary — runs after all VUs have completed.
function getServerWsActiveMetric() {
  try {
    const res = http.get(`${EMBEDDED_URL}/api/v1/dashboard/metrics-table`, {
      headers: authHeaders(),
      timeout: "5s",
    });
    if (res.status !== 200) return null;
    const body = JSON.parse(res.body);
    const metrics = body.metrics || [];
    for (let i = 0; i < metrics.length; i++) {
      if (metrics[i].name === "plexspaces_ws_thin_nodes_active") {
        return metrics[i].value;
      }
    }
  } catch (_) {}
  return null;
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
  const totalC    = data.metrics["ws_conn_total"]?.values?.count ?? 0;
  // server_ws_active = Prometheus gauge polled every POLL_INTERVAL_MS; values.max = true peak.
  const serverPeak = data.metrics["server_ws_active"]?.values?.max ?? 0;
  // server_active_now: read post-test from metrics-table (should be 0 after teardown).
  const serverActiveNow = getServerWsActiveMetric();
  const serverNowStr = serverActiveNow !== null
    ? `${serverActiveNow} (post-test; should be 0)`
    : "(unavailable)";
  const errRate   = ((data.metrics["ws_conn_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const dropRate  = ((data.metrics["ws_conn_drop_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const hbErrR    = ((data.metrics["ws_heartbeat_error_rate"]?.values?.rate ?? 0) * 100).toFixed(2);
  const cl  = data.metrics["ws_conn_upgrade_ms"];
  const rl  = data.metrics["ws_reg_latency_ms"];
  const hbl = data.metrics["ws_heartbeat_latency_ms"];

  const lines = [
    "",
    `╔${HR}╗`,
    row(`TEST: ws/capacity  │ TARGET: rust-embedded  │ MAX_LEVEL: ${MAX_LEVEL}`),
    row(`Levels: ${LEVELS.join("→")}  VUs/conns each`),
    row(`Stage duration: ${STAGE_DURATION}  │  Total run: ${durSec}s  │  Total conns: ${totalC}`),
    `╠${HR}╣`,
    row("CONNECTION CAPACITY"),
    row(`  max_concurrent_thin_nodes:  ${serverPeak}  (Prometheus peak, polled every ${POLL_INTERVAL_MS}ms)`),
    row(`  server_active_post_test:    ${serverNowStr}`),
    row(`  upgrade_error_rate:         ${errRate}%   (>10% = server at limit)`),
    row(`  unexpected_drop_rate:       ${dropRate}%`),
    row(`  heartbeat_error_rate:       ${hbErrR}%`),
    row(`  NOTE: metric plexspaces_ws_thin_nodes_active polled from Prometheus`),
    `╠${HR}╣`,
    row("LATENCY"),
    row("  WebSocket upgrade (time to 101 response):"),
    cl
      ? row(`    p50=${rp(fmtMs(cl.values.med),8)} p95=${rp(fmtMs(cl.values["p(95)"]),8)} p99=${fmtMs(cl.values["p(99)"])}`)
      : row("    (no data)"),
    row("  Thin-node registration handshake (WsFrame round-trip):"),
    rl
      ? row(`    p50=${rp(fmtMs(rl.values.med),8)} p95=${rp(fmtMs(rl.values["p(95)"]),8)} p99=${fmtMs(rl.values["p(99)"])}`)
      : row("    (no ack received — check server logs)"),
    row("  Heartbeat round-trip latency:"),
    hbl
      ? row(`    p50=${rp(fmtMs(hbl.values.med),8)} p95=${rp(fmtMs(hbl.values["p(95)"]),8)} p99=${fmtMs(hbl.values["p(99)"])}`)
      : row("    (no heartbeat acks — server may not echo them)"),
  ];

  const bottleneck = +errRate > 10 || +dropRate > 10
    ? `⚠ Capacity ceiling hit near ${serverPeak} thin-node connections`
    : `✓ No capacity limit reached up to ${serverPeak} thin-node connections`;
  lines.push(`╠${HR}╣`);
  lines.push(row(`VERDICT: ${bottleneck}`));
  lines.push(`╚${HR}╝`, "");

  return { stdout: lines.join("\n") };
}
