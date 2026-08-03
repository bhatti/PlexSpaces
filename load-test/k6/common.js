// SPDX-License-Identifier: AGPL-3.0-or-later
// Shared helpers for all k6 load test scenarios.
//
// Usage:
//   import { baseUrl, authHeaders, actorUrl, askActor, assertOk, grpcClient } from "../common.js";

import http from "k6/http";
import { check, fail } from "k6";
import { Counter, Rate, Trend } from "k6/metrics";

// ─── Configuration ─────────────────────────────────────────────────────────
// All values can be overridden via k6 environment variables (-e KEY=VALUE).

export const BASE_URL  = __ENV.BASE_URL  || "http://localhost:8091";
// The embedded Rust perf actor runs as a standalone server on port 8092 by default.
// Set EMBEDDED_URL to override (e.g. when running on a different host/port).
export const EMBEDDED_URL = __ENV.EMBEDDED_URL || "http://localhost:8092";
export const GRPC_HOST = __ENV.GRPC_HOST || "localhost:8091";  // same port — content-type routing
export const AUTH_TOKEN = __ENV.PLEXSPACES_TEST_TOKEN || "";
export const NO_AUTH = __ENV.PLEXSPACES_NO_AUTH === "1";

// Map short language names to their deployed application IDs.
export const APP_IDS = {
  python:        "perf-python",
  go:            "perf-go",
  typescript:    "perf-typescript",
  "rust-wasm":   "perf-rust-wasm",
  "rust-embedded": "perf-embedded",
};

// ─── Auth ──────────────────────────────────────────────────────────────────

export function authHeaders(extra = {}) {
  const headers = { "Content-Type": "application/json", ...extra };
  if (NO_AUTH) {
    // Server started with PLEXSPACES_DISABLE_AUTH=1 — no token needed
    headers["x-tenant-id"] = "default";
  } else if (AUTH_TOKEN) {
    headers["Authorization"] = `Bearer ${AUTH_TOKEN}`;
  } else {
    headers["x-tenant-id"] = "default";
  }
  return headers;
}

// ─── HTTP actor helpers ────────────────────────────────────────────────────

/**
 * Build the REST URL for an actor operation.
 * @param {string} appId     - deployed application_id (e.g. "perf-python")
 * @param {string} actorType - actor type name (e.g. "PerfActor")
 * @param {string} instance  - actor instance suffix (e.g. "default" or VU index)
 * @param {number} timeout   - ask timeout in seconds
 */
export function actorUrl(appId, actorType, instance, timeout = 30) {
  return `${BASE_URL}/api/v1/actors/${appId}/${instance}:${actorType}/ask?timeout=${timeout}`;
}

/**
 * Send a request to an actor and return the parsed JSON response.
 * Returns null on HTTP error so callers can check and record failures.
 */
export function askActor(appId, actorType, instance, payload, timeout = 30) {
  const url = actorUrl(appId, actorType, instance, timeout);
  const res = http.post(url, JSON.stringify(payload), { headers: authHeaders(), timeout: `${timeout + 5}s` });
  if (res.status !== 200) {
    return { _httpStatus: res.status, error: `HTTP ${res.status}` };
  }
  try {
    return JSON.parse(res.body);
  } catch (_) {
    return { error: "invalid JSON response", body: res.body };
  }
}

/**
 * Assert that a response from askActor is successful.
 * Records the check result so k6 includes it in the summary.
 */
export function assertOk(name, response) {
  const ok = check(response, {
    [`${name}: no HTTP error`]: (r) => !r._httpStatus,
    // Actor responses are wrapped: { success: true, payload: { ok: true, ... } }
    // Support both direct { ok: true } and wrapped { success: true } formats.
    [`${name}: success`]: (r) => r && (r.success === true || r.ok === true || (r.payload && r.payload.ok === true)),
    [`${name}: no error`]: (r) => !r.error && !r.error_message,
  });
  return ok;
}

// ─── gRPC helpers ─────────────────────────────────────────────────────────
// k6 gRPC support requires k6 >= 0.43 with the grpc module.
// Proto descriptors are loaded from the proto/ dir in the repo root.
// We use reflection (grpc_reflection) when available; otherwise the caller
// must pass the proto file path via GRPC_PROTO env var.

export function grpcBaseParams() {
  // Note: plaintext/timeout are connect() options, NOT invoke() options.
  // invoke() only accepts: metadata, timeout (as Duration string), tags, discardResponseBody.
  const params = {};
  if (NO_AUTH) {
    params.metadata = { "x-tenant-id": "default" };
  } else if (AUTH_TOKEN) {
    params.metadata = { authorization: `Bearer ${AUTH_TOKEN}` };
  } else {
    params.metadata = { "x-tenant-id": "default" };
  }
  return params;
}

// ─── Custom metrics ────────────────────────────────────────────────────────

export const errorRate       = new Rate("perf_error_rate");
export const echoLatency     = new Trend("perf_echo_latency_ms",    true);
export const computeLatency  = new Trend("perf_compute_latency_ms", true);
export const kvLatency       = new Trend("perf_kv_latency_ms",      true);
export const pgLatency       = new Trend("perf_pg_latency_ms",      true);
export const shardLatency    = new Trend("perf_shard_latency_ms",   true);
export const requestCounter  = new Counter("perf_requests_total");

// ─── Utilities ─────────────────────────────────────────────────────────────

/**
 * Pick an actor instance name. Using VU + iteration ensures each VU
 * hits a dedicated actor instance, avoiding lock contention in the mailbox.
 * Use instance="default" when you want all VUs to share one actor.
 */
export function vuInstance() {
  return `vu${__VU}`;
}

/**
 * Generate a list of float values for shard tasks.
 * @param {number} size - number of values
 * @param {number} seed - offset to make each shard's data distinct
 */
export function makeShardValues(size = 100, seed = 0) {
  return Array.from({ length: size }, (_, i) => (i + seed) * 0.1);
}

/**
 * Polls a URL until the JSON response satisfies a predicate or timeout is reached.
 * Used to wait for ShardGroup to become ACTIVE before sending work.
 */
export function pollUntil(url, predicate, intervalMs = 500, maxMs = 30000) {
  const deadline = Date.now() + maxMs;
  while (Date.now() < deadline) {
    const res = http.get(url, { headers: authHeaders() });
    if (res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        if (predicate(body)) return body;
      } catch (_) {}
    }
    // k6 does not have a blocking sleep in setup/teardown; use a busy-wait with
    // a short inner loop. In VU code use `sleep(intervalMs/1000)` instead.
  }
  fail(`pollUntil timed out after ${maxMs}ms waiting on ${url}`);
}
