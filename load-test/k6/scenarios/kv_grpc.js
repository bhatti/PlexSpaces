// SPDX-License-Identifier: AGPL-3.0-or-later
// kv_grpc.js — Measure KV put + get round-trip latency via gRPC.
//
// Same logic as kv_http.js; compare results directly to isolate transport overhead.
//
// Run:
//   k6 run -e LANG=python -e VUS=100 -e ITERATIONS=100 k6/scenarios/kv_grpc.js

import grpc from "k6/net/grpc";
import encoding from "k6/encoding";
import { check } from "k6";
import { GRPC_HOST, APP_IDS, grpcBaseParams, vuInstance, errorRate, kvLatency, requestCounter } from "../common.js";

const LANG       = __ENV.LANG       || "python";
const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const PROTO_ROOT = __ENV.PROTO_ROOT || "../../proto";

export const options = {
  scenarios: {
    kv_grpc: {
      executor: "per-vu-iterations",
      vus: VUS,
      iterations: ITERATIONS,
      maxDuration: "10m",
    },
  },
  thresholds: {
    "perf_kv_latency_ms": ["p(99)<5000"],
    "perf_error_rate": ["rate<0.05"],
  },
};

const appId     = APP_IDS[LANG] || `perf-${LANG}`;
const actorType = "PerfActor";
const client    = new grpc.Client();

// Per-VU connection flag — prevents reconnect per iteration (ephemeral port exhaustion).
let _connected = false;

export function setup() {
  client.load([PROTO_ROOT], "plexspaces/v1/actors/actor_runtime.proto");
}

function askGrpc(instance, payload) {
  return client.invoke(
    "plexspaces.actor.v1.ActorService/AskReply",
    {
      actor_type:  actorType,
      actor_name:  instance,
      namespace:   appId,
      http_method: "POST",
      message_type: "call",
      payload:     encoding.b64encode(JSON.stringify(payload)),
    },
    { ...grpcBaseParams(), tags: { transport: "grpc", lang: LANG } }
  );
}

export default function () {
  if (!_connected) {
    client.connect(GRPC_HOST, { plaintext: true });
    _connected = true;
  }

  const instance = vuInstance();
  const key = `perf_key_vu${__VU}`;

  const start = Date.now();
  const putRes = askGrpc(instance, { op: "kv_put", key, value: `v${__ITER}` });
  const putOk  = check(putRes, { "kv_put gRPC OK": (r) => r && r.status === grpc.StatusOK });
  errorRate.add(!putOk);

  const getRes = askGrpc(instance, { op: "kv_get", key });
  const elapsed = Date.now() - start;

  kvLatency.add(elapsed, { transport: "grpc", lang: LANG });
  requestCounter.add(2, { transport: "grpc", lang: LANG, op: "kv" });

  const getOk = check(getRes, { "kv_get gRPC OK": (r) => r && r.status === grpc.StatusOK });
  errorRate.add(!getOk);
}

export function teardown() { client.close(); }
