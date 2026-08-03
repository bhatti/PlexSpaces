// SPDX-License-Identifier: AGPL-3.0-or-later
// grpc_connections.js — Find the maximum inbound gRPC connection limit.
//
// Strategy: ramp VUs (each holding one persistent gRPC connection) to find
// where the server starts refusing connections or errors spike above 5%.
//
// Each VU opens one gRPC connection in setup and keeps it alive by sending
// a lightweight echo every second. The test tracks how many connections are
// alive at each level and when failures begin.
//
// Scale levels: 50 → 100 → 250 → 500 → 1000 → 2000 connections,
// each held for DURATION (default 30s). Stop on first level where
// error rate exceeds 5%.
//
// Run:
//   k6 run -e DURATION=30s \
//          -e PROTO_ROOT=/abs/path/to/proto \
//          load-test/k6/scenarios/grpc_connections.js

import grpc from "k6/net/grpc";
import encoding from "k6/encoding";
import { check, sleep } from "k6";
import { Rate, Counter, Gauge } from "k6/metrics";
import { GRPC_HOST, grpcBaseParams } from "../common.js";

const DURATION = __ENV.DURATION || "30s";
const PROTO_ROOT       = __ENV.PROTO_ROOT       || "proto";
const PROTO_VALIDATE   = __ENV.PROTO_VALIDATE   || "";
const PROTO_GOOGLEAPIS = __ENV.PROTO_GOOGLEAPIS || "";
const PROTO_GRPC_GW    = __ENV.PROTO_GRPC_GW    || "";
const EMBEDDED_GRPC_HOST = __ENV.EMBEDDED_GRPC_HOST || "localhost:8092";
const LANG = __ENV.LANG || "rust-embedded";

const errorRate      = new Rate("grpc_conn_error_rate");
const requestCounter = new Counter("grpc_conn_requests_total");
const activeConns    = new Gauge("grpc_conn_active");

// Ramp through these VU counts (= connection counts), one stage per level.
const LEVELS = [50, 100, 250, 500, 1000, 2000];

export const options = {
  scenarios: {
    grpc_connections: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: LEVELS.map((vus, i) => [
        { duration: "5s",      target: vus },    // ramp up
        { duration: DURATION,  target: vus },    // hold
        ...(i === LEVELS.length - 1 ? [{ duration: "5s", target: 0 }] : []),
      ]).flat(),
      gracefulRampDown: "10s",
    },
  },
  thresholds: {
    "grpc_conn_error_rate": ["rate<0.05"],
    "grpc_req_duration":    ["p(99)<5000"],
  },
};

const client = new grpc.Client();
const protoPaths = [PROTO_ROOT];
if (PROTO_VALIDATE)   protoPaths.push(PROTO_VALIDATE);
if (PROTO_GOOGLEAPIS) protoPaths.push(PROTO_GOOGLEAPIS);
if (PROTO_GRPC_GW)    protoPaths.push(PROTO_GRPC_GW);
client.load(protoPaths, "plexspaces/v1/actors/actor_runtime.proto");

// Per-VU connection flag — prevents reconnect per iteration (ephemeral port exhaustion).
let _connected = false;

export default function () {
  const grpcHost = (LANG === "rust-embedded") ? EMBEDDED_GRPC_HOST : GRPC_HOST;

  // Each VU opens exactly one persistent connection for its lifetime.
  if (!_connected) {
    const connected = client.connect(grpcHost, { plaintext: true, timeout: "5s" });
    if (!connected) {
      errorRate.add(1);
      return;
    }
    _connected = true;
  }

  activeConns.add(1);

  // Send a lightweight echo to verify the connection is alive.
  const payloadStr = JSON.stringify({ op: "echo", payload: "ping" });
  const res = client.invoke(
    "plexspaces.actor.v1.ActorService/AskReply",
    {
      namespace:    "perf-embedded",
      actor_type:   "gen_server",
      actor_name:   `conn-vu${__VU}`,
      payload:      encoding.b64encode(payloadStr),
      http_method:  "POST",
      message_type: "call",
    },
    { ...grpcBaseParams(), tags: { op: "keep_alive" } }
  );

  requestCounter.add(1);
  const ok = check(res, {
    "connection alive": (r) => r && r.status === grpc.StatusOK,
  });
  errorRate.add(!ok);

  // Hold connection open for 1s before the next keep-alive ping.
  sleep(1);
}

export function teardown() {
  client.close();
}
