// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// EFlows4HPC → PlexSpaces: HPC Ensemble (TypeScript WASM)
//
// Single actor: coordinator (run) and workers (onTasks_ready). Uses host.ts (tuple space)
// and host.processGroups (join/broadcast). Convention: ensemble:coord-1, ensemble:worker-0/1.
import { WorkflowActor, host } from "@plexspaces/sdk";
const TASK_MS = 18;
const RESULT_PREFIX = "ensemble";
const WORKER_GROUP = "ensemble-workers";
const GATHER_POLL_MAX = 200;
export class EnsembleActor extends WorkflowActor {
    getDefaultState() {
        return {
            ensemble_id: "",
            num_tasks: 0,
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
    run(payload) {
        const t0 = host.nowMs();
        if (this.state.created_at_ms === 0)
            this.state.created_at_ms = t0;
        this.state.ensemble_id = String(payload.ensemble_id ?? payload.job_id ?? this.state.ensemble_id);
        this.state.updated_at_ms = host.nowMs();
        if (this.state.cancel_requested) {
            this.state.status = "cancelled";
            return this.finish(t0, 0, "cancelled");
        }
        if (this.state.status === "completed")
            return this.finish(t0, 0, "completed");
        const numTasks = Number(payload.num_tasks ?? 10) || 0;
        if (numTasks <= 0)
            return this.finish(t0, 0, "no_tasks");
        this.state.num_tasks = numTasks;
        this.state.num_completed = 0;
        this.state.status = "scattering";
        let computeMs = 0;
        for (let i = 0; i < numTasks; i++) {
            const err = host.ts.write([RESULT_PREFIX, this.state.ensemble_id, "task", `t${i}`, i]);
            if (err && err.startsWith("ERROR"))
                host.log("warn", `ts write failed: ${err}`);
            computeMs += 2;
        }
        this.state.updated_at_ms = host.nowMs();
        this.state.status = "running";
        try {
            host.processGroups.broadcast(WORKER_GROUP, "tasks_ready", {
                ensemble_id: this.state.ensemble_id,
                num_tasks: numTasks,
            });
        }
        catch (e) {
            host.log("warn", `pg_broadcast failed: ${e}`);
        }
        this.state.total_coord_ms += host.nowMs() - t0;
        const pattern = [RESULT_PREFIX, this.state.ensemble_id, "result", null, null];
        for (let i = 0; i < GATHER_POLL_MAX; i++) {
            if (this.state.cancel_requested) {
                this.state.status = "cancelled";
                return this.finish(t0, computeMs, "cancelled");
            }
            const results = host.ts.readAll(pattern);
            this.state.num_completed = Array.isArray(results) ? results.length : 0;
            if (this.state.num_completed >= numTasks)
                break;
            this.state.updated_at_ms = host.nowMs();
        }
        this.state.status = "completed";
        this.state.updated_at_ms = host.nowMs();
        return this.finish(t0, computeMs, "completed");
    }
    finish(t0, computeMs, status) {
        const elapsed = host.nowMs() - t0;
        const coordMs = Math.max(0, (elapsed > computeMs ? elapsed : computeMs) - computeMs);
        this.state.total_compute_ms += computeMs;
        this.state.total_coord_ms += coordMs;
        return {
            status,
            ensemble_id: this.state.ensemble_id,
            num_tasks: this.state.num_tasks,
            num_completed: this.state.num_completed,
            total_compute_ms: this.state.total_compute_ms,
            total_coord_ms: this.state.total_coord_ms,
        };
    }
    signal(name, _data) {
        if (name === "cancel") {
            this.state.cancel_requested = true;
            this.state.updated_at_ms = host.nowMs();
        }
    }
    query(name, _params) {
        if (name === "status") {
            return {
                ensemble_id: this.state.ensemble_id,
                status: this.state.status,
                num_tasks: this.state.num_tasks,
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
    /** Worker: called when process group broadcast "tasks_ready" is received (msgType used for routing). */
    onTasks_ready(payload) {
        if (!this.state.worker_joined) {
            try {
                host.processGroups.join(WORKER_GROUP);
                this.state.worker_joined = true;
                host.log("info", `Joined process group ${WORKER_GROUP}`);
            }
            catch (e) {
                host.log("warn", `pg_join failed: ${e}`);
            }
        }
        const ensembleId = String(payload.ensemble_id ?? "");
        const numTasks = Number(payload.num_tasks ?? 0) || 0;
        if (!ensembleId || numTasks <= 0)
            return { ok: true, message: "warmup" };
        const t0 = host.nowMs();
        const pattern = [RESULT_PREFIX, ensembleId, "task", null, null];
        let processed = 0;
        let computeMs = 0;
        while (true) {
            const taken = host.ts.take(pattern);
            if (taken == null)
                break;
            const taskId = Array.isArray(taken) && taken.length > 3 ? taken[3] : "";
            computeMs += TASK_MS;
            host.ts.write([RESULT_PREFIX, ensembleId, "result", taskId, { ok: true, ms: TASK_MS }]);
            processed++;
        }
        const elapsed = host.nowMs() - t0;
        this.state.total_compute_ms += computeMs;
        this.state.total_coord_ms += Math.max(0, elapsed - computeMs);
        return { ok: true, processed, ensemble_id: ensembleId };
    }
}
const actorInstance = new EnsembleActor();
export const actor = {
    init: (configJson) => actorInstance.init(configJson),
    handle: (from, msgType, payloadJson) => actorInstance.handle(from, msgType, payloadJson),
    getState: () => actorInstance.getState(),
    setState: (stateJson) => actorInstance.setState(stateJson),
};
