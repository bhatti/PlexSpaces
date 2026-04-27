// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Merlin → PlexSpaces: Parameter sweep (TypeScript WASM).
//
// Worker pool (elastic size): elastic pool API (poolCheckout/poolCheckin) + tuple space (work queue).
// Coordinator (run): scatter → checkout workers from pool, send work_available → gather → checkin.
// If pool is not configured, falls back to process group broadcast.
// Workers (onWork_available): take tasks from tuple space, run simulation, write results.

import { WorkflowActor, host } from "@plexspaces/sdk";

const SIMULATION_MS = 22;
const TUPLE_PREFIX = "merlin";
const WORKER_GROUP = "merlin-workers";
const POOL_NAME = "merlin-workers";
const MAX_CHECKOUT_WORKERS = 10;
const GATHER_POLL_MAX = 200;
const CHECKOUT_TIMEOUT_MS = 5000;

interface SweepState {
  sweep_id: string;
  num_params: number;
  num_completed: number;
  status: string;
  total_compute_ms: number;
  total_coord_ms: number;
  created_at_ms: number;
  updated_at_ms: number;
  cancel_requested: boolean;
  worker_joined: boolean;
}

export class SweepActor extends WorkflowActor<SweepState> {
  getDefaultState(): SweepState {
    return {
      sweep_id: "",
      num_params: 0,
      num_completed: 0,
      status: "idle",
      total_compute_ms: 0,
      total_coord_ms: 0,
      created_at_ms: 0,
      updated_at_ms: 0,
      cancel_requested: false,
      worker_joined: false,
    };
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const t0 = host.nowMs();
    if (this.state.created_at_ms === 0) this.state.created_at_ms = t0;
    this.state.sweep_id = String(payload.sweep_id ?? payload.job_id ?? this.state.sweep_id);
    this.state.updated_at_ms = host.nowMs();

    if (this.state.cancel_requested) {
      this.state.status = "cancelled";
      return this.finish(t0, 0, "cancelled");
    }
    if (this.state.status === "completed") return this.finish(t0, 0, "completed");

    const numParams = Number(payload.num_params ?? 10) || 0;
    if (numParams <= 0) return this.finish(t0, 0, "no_params");

    this.state.num_params = numParams;
    this.state.num_completed = 0;
    this.state.status = "scattering";
    let computeMs = 0;
    const paramBase = Number(payload.param_base ?? 0) || 0;

    for (let i = 0; i < numParams; i++) {
      const paramVal = paramBase + i;
      const out = host.ts.write([
        TUPLE_PREFIX,
        this.state.sweep_id,
        "task",
        `p${i}`,
        { param: paramVal },
      ]);
      if (out && out.startsWith("ERROR")) host.log("warn", `ts write failed: ${out}`);
      computeMs += 2;
    }
    this.state.updated_at_ms = host.nowMs();
    this.state.status = "running";

    const checkoutHandles: { actor_id: string; pool_name: string; checkout_id: string }[] = [];
    try {
      for (let i = 0; i < Math.min(MAX_CHECKOUT_WORKERS, numParams); i++) {
        const handle = host.poolCheckout(POOL_NAME, CHECKOUT_TIMEOUT_MS);
        if (handle == null) break;
        checkoutHandles.push(handle);
        const out = host.send(handle.actor_id, "work_available", {
          sweep_id: this.state.sweep_id,
          num_params: numParams,
        });
        if (out && out.startsWith("ERROR")) host.log("warn", `send to worker failed: ${out}`);
      }
      if (checkoutHandles.length === 0) {
        host.processGroups.broadcast(WORKER_GROUP, "work_available", {
          sweep_id: this.state.sweep_id,
          num_params: numParams,
        });
      }
    } catch (e) {
      host.log("warn", `pool/broadcast failed: ${e}`);
      try {
        host.processGroups.broadcast(WORKER_GROUP, "work_available", {
          sweep_id: this.state.sweep_id,
          num_params: numParams,
        });
      } catch (e2) {
        host.log("warn", `pg_broadcast failed: ${e2}`);
      }
    }
    this.state.total_coord_ms += host.nowMs() - t0;

    const pattern: unknown[] = [TUPLE_PREFIX, this.state.sweep_id, "result", null, null];
    for (let i = 0; i < GATHER_POLL_MAX; i++) {
      if (this.state.cancel_requested) {
        this.state.status = "cancelled";
        return this.finish(t0, computeMs, "cancelled");
      }
      const results = host.ts.readAll(pattern);
      this.state.num_completed = Array.isArray(results) ? results.length : 0;
      if (this.state.num_completed >= numParams) break;
      this.state.updated_at_ms = host.nowMs();
    }

    for (const h of checkoutHandles) {
      try {
        host.poolCheckin(POOL_NAME, h.actor_id, h.checkout_id, true);
      } catch (e) {
        host.log("warn", `pool_checkin failed: ${e}`);
      }
    }

    this.state.status = "completed";
    this.state.updated_at_ms = host.nowMs();
    return this.finish(t0, computeMs, "completed");
  }

  private finish(
    t0: number,
    computeMs: number,
    status: string
  ): Record<string, unknown> {
    const elapsed = host.nowMs() - t0;
    const coordMs = Math.max(0, (elapsed > computeMs ? elapsed : computeMs) - computeMs);
    this.state.total_compute_ms += computeMs;
    this.state.total_coord_ms += coordMs;
    const approxBytes = this.state.num_params * 100 + this.state.num_completed * 80;
    return {
      status,
      sweep_id: this.state.sweep_id,
      num_params: this.state.num_params,
      num_completed: this.state.num_completed,
      data_size_bytes: approxBytes,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    };
  }

  signal(name: string, _data: Record<string, unknown>): void {
    if (name === "cancel") {
      this.state.cancel_requested = true;
      this.state.updated_at_ms = host.nowMs();
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "status") {
      return {
        sweep_id: this.state.sweep_id,
        status: this.state.status,
        num_params: this.state.num_params,
        num_completed: this.state.num_completed,
        cancel_requested: this.state.cancel_requested,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        created_at_ms: this.state.created_at_ms,
        updated_at_ms: this.state.updated_at_ms,
      };
    }
    return { error: "unknown_query", name };
  }

  /** Worker: take tasks from tuple space, run simulation, write results. Joins process group for broadcast fallback. */
  onWork_available(payload: Record<string, unknown>): Record<string, unknown> {
    if (!this.state.worker_joined) {
      try {
        host.processGroups.join(WORKER_GROUP);
        this.state.worker_joined = true;
        host.log("info", `Joined worker pool ${WORKER_GROUP}`);
      } catch (e) {
        host.log("warn", `pg_join failed: ${e}`);
      }
    }
    const sweepId = String(payload.sweep_id ?? "");
    const numParams = Number(payload.num_params ?? 0) || 0;
    if (!sweepId || numParams <= 0) return { ok: true, message: "warmup" };

    const t0 = host.nowMs();
    const pattern: unknown[] = [TUPLE_PREFIX, sweepId, "task", null, null];
    let processed = 0;
    let computeMs = 0;
    while (true) {
      const taken = host.ts.take(pattern);
      if (taken == null) break;
      const paramId = Array.isArray(taken) && taken.length > 3 ? taken[3] : "";
      computeMs += SIMULATION_MS;
      host.ts.write([
        TUPLE_PREFIX,
        sweepId,
        "result",
        paramId,
        { ok: true, ms: SIMULATION_MS },
      ]);
      processed++;
    }
    const elapsed = host.nowMs() - t0;
    this.state.total_compute_ms += computeMs;
    this.state.total_coord_ms += Math.max(0, elapsed - computeMs);
    return { ok: true, processed, sweep_id: sweepId };
  }
}

const actorInstance = new SweepActor();
export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
