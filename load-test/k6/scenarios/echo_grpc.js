// SPDX-License-Identifier: AGPL-3.0-or-later
// echo_grpc.js — Measure gRPC round-trip latency per language.
//
// Calls ActorService.AskReply — same echo operation as echo_http.js
// so results are directly comparable.
//
// k6 automatically records grpc_req_duration for all gRPC calls.
// We also record perf_error_rate for the shared threshold across scenarios.
//
// Run (from repo root):
//   k6 run -e LANG=rust-embedded -e VUS=100 -e DURATION=5m \
//          -e ACTOR_TYPE=regular \
//          -e PROTO_ROOT=/abs/path/to/proto \
//          -e PROTO_VALIDATE=/abs/path/to/bufbuild/protovalidate/files \
//          -e PROTO_GOOGLEAPIS=/abs/path/to/googleapis/files \
//          -e PROTO_GRPC_GW=/abs/path/to/grpc-gateway/files \
//          load-test/k6/scenarios/echo_grpc.js

import grpc from "k6/net/grpc";
import encoding from "k6/encoding";
import { check } from "k6";
import { Trend, Rate, Counter } from "k6/metrics";
import { GRPC_HOST, APP_IDS, grpcBaseParams, vuInstance, echoLatency, requestCounter } from "../common.js";

const LANG       = __ENV.LANG       || "rust-embedded";
const VUS        = parseInt(__ENV.VUS        || "100");
const ITERATIONS = parseInt(__ENV.ITERATIONS || "100");
const DURATION   = __ENV.DURATION   || "";   // e.g. "5m" overrides per-VU iterations
const PROTO_ROOT = __ENV.PROTO_ROOT || "proto";
const PROTO_VALIDATE  = __ENV.PROTO_VALIDATE  || "";
const PROTO_GOOGLEAPIS = __ENV.PROTO_GOOGLEAPIS || "";
const PROTO_GRPC_GW   = __ENV.PROTO_GRPC_GW   || "";
// ACTOR_TYPE: "regular" = pre-spawned pool, "virtual" = on-demand activation per VU
const ACTOR_TYPE = __ENV.ACTOR_TYPE || "regular";
// WARM_COUNT: size of the pre-warmed embedded actor pool (perf-vu0..N-1)
const WARM_COUNT = parseInt(__ENV.WARM_COUNT || "10");
// MAX_WASM_INSTANCES: cap the virtual actor pool for WASM languages to bound memory
const MAX_WASM_INSTANCES = parseInt(__ENV.MAX_WASM_INSTANCES || "10");
// LOG_ERRORS: log first N unique error bodies to stdout for debugging (default 5)
const LOG_ERRORS = parseInt(__ENV.LOG_ERRORS || "5");
// Embedded actors run on port 8092; WASM actors use the main server (8091 default).
const EMBEDDED_GRPC_HOST = __ENV.EMBEDDED_GRPC_HOST || "localhost:8092";

const errorRate = new Rate("perf_error_rate");

// Per-activation-state latency trends to diagnose virtual actor spawn overhead.
const firstActivationLatency = new Trend("perf_echo_grpc_first_activation_ms", true);
const steadyStateLatency     = new Trend("perf_echo_grpc_steady_state_ms",     true);

// Per-VU flag: has this VU already activated its virtual actor instance?
const _vuActivated = {};

// Dedup log: only log each unique error body once up to LOG_ERRORS distinct messages.
const _loggedErrors = new Map();

function buildScenario() {
  if (DURATION) {
    return {
      echo_grpc: {
        executor: "constant-vus",
        vus: VUS,
        duration: DURATION,
      },
    };
  }
  return {
    echo_grpc: {
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
    // k6 built-in gRPC latency metric
    "grpc_req_duration": ["p(99)<1000"],
    "perf_error_rate":   ["rate<0.05"],
  },
};

const appId     = APP_IDS[LANG] || `perf-${LANG}`;
const actorType = (LANG === "rust-embedded") ? "gen_server" : "PerfActor";

// client.load must be called in init context (module scope).
const client = new grpc.Client();
const protoPaths = [PROTO_ROOT];
if (PROTO_VALIDATE)   protoPaths.push(PROTO_VALIDATE);
if (PROTO_GOOGLEAPIS) protoPaths.push(PROTO_GOOGLEAPIS);
if (PROTO_GRPC_GW)    protoPaths.push(PROTO_GRPC_GW);
client.load(protoPaths, "plexspaces/v1/actors/actor_runtime.proto");

// Per-VU connection flag.  In k6, module-scope variables are NOT shared across
// VUs — each VU gets its own copy.  Using this instead of client.connected
// (unreliable in many k6 versions) ensures exactly one TCP connection per VU
// regardless of iteration count, preventing ephemeral port exhaustion.
let _connected = false;

export default function () {
  const grpcHost = (LANG === "rust-embedded") ? EMBEDDED_GRPC_HOST : GRPC_HOST;
  if (!_connected) {
    client.connect(grpcHost, { plaintext: true, timeout: "10s" });
    _connected = true;
  }

  // Instance routing mirrors echo_http.js:
  //   virtual   — unique instance per VU slot (capped at MAX_WASM_INSTANCES) triggers on-demand activation
  //   regular embedded — cycle through pre-warmed pool (perf-vu0..WARM_COUNT-1)
  //   regular WASM     — one instance per VU
  let instance;
  if (ACTOR_TYPE === "virtual") {
    const poolIdx = ((__VU - 1) % MAX_WASM_INSTANCES) + 1;
    instance = `virtual-vu${poolIdx}`;
  } else if (LANG === "rust-embedded") {
    instance = `perf-vu${(__VU - 1) % WARM_COUNT}`;
  } else {
    instance = vuInstance();
  }

  const payloadStr = JSON.stringify({ op: "echo", payload: { vu: __VU, iter: __ITER } });

  const start = Date.now();
  const res = client.invoke(
    "plexspaces.actor.v1.ActorService/AskReply",
    {
      namespace:    appId,
      actor_type:   actorType,
      actor_name:   instance,
      // k6 gRPC requires proto bytes fields to be base64-encoded strings.
      payload:      encoding.b64encode(payloadStr),
      http_method:  "POST",     // required: empty defaults to GET which drops payload
      message_type: "call",
    },
    { ...grpcBaseParams(), tags: { transport: "grpc", lang: LANG, op: "echo" } }
  );
  const elapsed = Date.now() - start;

  echoLatency.add(elapsed, { transport: "grpc", lang: LANG });
  requestCounter.add(1, { transport: "grpc", lang: LANG, op: "echo" });

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
    "gRPC OK":  (r) => r && r.status === grpc.StatusOK,
    "success":  (r) => r && r.message && r.message.success === true,
  });
  errorRate.add(!ok);

  if (!ok && _loggedErrors.size < LOG_ERRORS) {
    const body = res ? JSON.stringify(res.message || res.error || res.status) : "(null response)";
    if (!_loggedErrors.has(body)) {
      _loggedErrors.set(body, true);
      console.error(`[echo_grpc error] VU=${__VU} iter=${__ITER} status=${res && res.status} actor_type=${ACTOR_TYPE} instance=${instance} body=${body.substring(0, 300)}`);
    }
  }
}

export function teardown() {
  client.close();
}

export function handleSummary(data) {
  const m = data.metrics["perf_echo_latency_ms"];
  if (!m) return { stdout: `\nNo metrics collected for echo_grpc (${LANG})\n` };

  const p50 = m.values["p(50)"]?.toFixed(1) ?? "–";
  const p95 = m.values["p(95)"]?.toFixed(1) ?? "–";
  const p99 = m.values["p(99)"]?.toFixed(1) ?? "–";
  const rps = (data.metrics["perf_requests_total"]?.values?.count / data.state.testRunDurationMs * 1000).toFixed(1);
  const errors = (data.metrics["perf_error_rate"]?.values?.rate * 100).toFixed(2);

  // k6 built-in gRPC metrics
  const grpcP50  = data.metrics["grpc_req_duration"]?.values["p(50)"]?.toFixed(1) ?? "–";
  const grpcP95  = data.metrics["grpc_req_duration"]?.values["p(95)"]?.toFixed(1) ?? "–";
  const grpcP99  = data.metrics["grpc_req_duration"]?.values["p(99)"]?.toFixed(1) ?? "–";
  const dataRecv = ((data.metrics["data_received"]?.values?.count ?? 0) / 1024 / 1024).toFixed(1);
  const dataSent = ((data.metrics["data_sent"]?.values?.count ?? 0) / 1024 / 1024).toFixed(1);
  const iters    = data.metrics["iterations"]?.values?.count ?? 0;

  // Virtual actor activation breakdown
  let activationLines = "";
  if (ACTOR_TYPE === "virtual" && data.metrics["perf_echo_grpc_first_activation_ms"]) {
    const fa = data.metrics["perf_echo_grpc_first_activation_ms"];
    const ss = data.metrics["perf_echo_grpc_steady_state_ms"];
    const faP50 = fa.values["p(50)"]?.toFixed(1) ?? "–";
    const faP95 = fa.values["p(95)"]?.toFixed(1) ?? "–";
    const faMax = fa.values["max"]?.toFixed(1) ?? "–";
    const ssP50 = ss?.values["p(50)"]?.toFixed(1) ?? "–";
    const ssP95 = ss?.values["p(95)"]?.toFixed(1) ?? "–";
    activationLines = `
║  first-activation p50=${faP50.padStart(6)}ms  p95=${faP95.padStart(6)}ms  max=${faMax.padStart(6)}ms  ║
║  steady-state     p50=${ssP50.padStart(6)}ms  p95=${ssP95.padStart(6)}ms                ║`;
  }

  return { stdout: `
╔═══════════════════════════════════════════════════════════════╗
║  echo | grpc  | ${LANG.padEnd(15)} | ${ACTOR_TYPE.padEnd(9)}               ║
╠═══════════════════════════════════════════════════════════════╣
║  p50=${p50.padStart(7)}ms  p95=${p95.padStart(7)}ms  p99=${p99.padStart(7)}ms       ║
║  RPS=${rps.padStart(8)}  Errors=${errors.padStart(6)}%  iters=${String(iters).padStart(8)}       ║
║  grpc_req p50=${grpcP50.padStart(7)}ms  p95=${grpcP95.padStart(7)}ms  p99=${grpcP99.padStart(7)}ms  ║
║  data recv=${dataRecv.padStart(6)}MB  sent=${dataSent.padStart(6)}MB                  ║${activationLines}
╚═══════════════════════════════════════════════════════════════╝
` };
}
