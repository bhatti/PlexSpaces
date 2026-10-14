// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Job Queue Outbox Actor — TypeScript WASM
//
// Demonstrates high-scale outbox dispatch with admission control:
//   - Leader creates 100 virtual workers via host.createShardGroup()
//   - Leader dispatches jobs to all workers via host.outbox.dispatch()
//   - Upgrade simulation: leader host.ask()s workers for upgrade status
//   - Heartbeat OVERWRITE: LWW idempotency with conflictResolution="overwrite"
//   - Scaling benchmark: measures throughput at 10/50/100 worker scales
//   - Rich metrics: wall_time_ms, ask RTT (min/max/avg), per-dispatch stats
//
// SDK API used:
//   host.createShardGroup({groupId, actorType, shardCount, placement}) -> CreateShardGroupResult
//   host.outbox.dispatch(ctx, dispatcherId, targets, payload, msgType, key, conflictResolution)
//   host.ask(to, msgType, payload, timeoutMs) -> unknown
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
const DISPATCHER_ID = "job-outbox-dispatcher";
const DEFAULT_WORKERS = 100;
const HEARTBEAT_IDEM_KEY = "hb-leader";
class JobQueueLeaderActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "",
            total_dispatched: 0,
            total_accepted: 0,
            total_rejected: 0,
            dispatch_count: 0,
            wall_time_ms: 0,
            heartbeat_count: 0,
            ask_rtt_min_ms: 0,
            ask_rtt_max_ms: 0,
            ask_rtt_sum_ms: 0,
            ask_rtt_count: 0,
            shard_group_id: "",
            worker_ids: [],
        };
    }
    onInit(config) {
        if (typeof config.actor_id === "string" && config.actor_id) {
            this.state.actor_id = config.actor_id;
        }
        this.state.worker_ids = [];
        this.state.shard_group_id = "";
    }
    onInit_workers(payload) {
        const count = typeof payload.count === "number" ? payload.count : DEFAULT_WORKERS;
        const groupId = `ts-job-workers-${Date.now() % 100000}`;
        try {
            const result = host.createShardGroup({
                groupId,
                actorType: "JobQueueWorker",
                shardCount: count,
                placement: { strategy: "from_registry" },
            });
            this.state.shard_group_id = result.groupId || groupId;
            this.state.worker_ids = result.shardActorIds ?? [];
            return {
                success: true,
                group_id: this.state.shard_group_id,
                worker_count: this.state.worker_ids.length,
                shard_count: result.shardCount,
            };
        }
        catch (e) {
            return { success: false, error: String(e) };
        }
    }
    doDispatch(targets, innerPayload) {
        if (targets.length === 0)
            return { accepted: 0, rejected: 0, wall_time_ms: 0 };
        const ctx = JSON.stringify({ tenant: "default", namespace: "default" });
        const payloadBytes = new TextEncoder().encode(JSON.stringify(innerPayload));
        const start = Date.now();
        let accepted = 0;
        let rejected = 0;
        try {
            const result = host.outbox.dispatch(ctx, DISPATCHER_ID, targets, payloadBytes, "cast", "", "");
            accepted = result[0];
            rejected = result[1];
        }
        catch {
            rejected = targets.length;
        }
        const elapsed = Date.now() - start;
        this.state.total_dispatched += targets.length;
        this.state.total_accepted += accepted;
        this.state.total_rejected += rejected;
        this.state.dispatch_count += 1;
        this.state.wall_time_ms += elapsed;
        return { accepted, rejected, wall_time_ms: elapsed };
    }
    onDispatch_jobs(payload) {
        const workers = this.state.worker_ids;
        if (workers.length === 0) {
            return { success: false, error: "no workers — call init_workers first" };
        }
        const innerPayload = payload.payload ?? { task: "process_item" };
        const { accepted, rejected, wall_time_ms } = this.doDispatch(workers, innerPayload);
        const throughput = wall_time_ms > 0 ? Math.floor((accepted * 1000) / wall_time_ms) : 0;
        return {
            success: true,
            accepted,
            rejected,
            wall_time_ms,
            throughput_per_s: throughput,
            total_dispatched: this.state.total_dispatched,
        };
    }
    onUpgrade_workers(payload) {
        const count = typeof payload.count === "number" ? payload.count : 4;
        const workers = this.state.worker_ids;
        if (workers.length === 0) {
            return { success: false, error: "no workers — call init_workers first" };
        }
        const targets = workers.slice(0, Math.min(count, workers.length));
        let rttMin = 0, rttMax = 0, rttSum = 0;
        let asked = 0;
        const replies = [];
        for (const targetId of targets) {
            const start = Date.now();
            try {
                const reply = host.ask(targetId, "upgrade", { version: "2.0.0" }, 10000);
                const elapsed = Date.now() - start;
                if (asked === 0 || elapsed < rttMin)
                    rttMin = elapsed;
                if (elapsed > rttMax)
                    rttMax = elapsed;
                rttSum += elapsed;
                asked++;
                replies.push(reply);
            }
            catch {
                // worker may not be active yet
            }
        }
        const rttAvg = asked > 0 ? Math.floor(rttSum / asked) : 0;
        this.state.ask_rtt_min_ms = this.state.ask_rtt_count === 0 ? rttMin : Math.min(this.state.ask_rtt_min_ms, rttMin);
        this.state.ask_rtt_max_ms = Math.max(this.state.ask_rtt_max_ms, rttMax);
        this.state.ask_rtt_sum_ms += rttSum;
        this.state.ask_rtt_count += asked;
        return {
            success: true,
            workers_asked: asked,
            ask_rtt_min_ms: rttMin,
            ask_rtt_max_ms: rttMax,
            ask_rtt_avg_ms: rttAvg,
            replies,
        };
    }
    onSend_heartbeat(payload) {
        const seq = typeof payload.seq === "number" ? payload.seq : 1;
        const ctx = JSON.stringify({ tenant: "default", namespace: "default" });
        const payloadBytes = new TextEncoder().encode(JSON.stringify({ seq, ts: Date.now() }));
        let accepted = 0;
        try {
            const result = host.outbox.dispatch(ctx, DISPATCHER_ID, [this.state.actor_id || "leader:JobQueueLeader"], payloadBytes, "heartbeat", HEARTBEAT_IDEM_KEY, "overwrite");
            accepted = result[0];
        }
        catch {
            // best-effort
        }
        this.state.heartbeat_count += 1;
        return { success: true, seq, accepted, heartbeat_count: this.state.heartbeat_count };
    }
    onScaling_benchmark() {
        const available = this.state.worker_ids;
        if (!available || available.length === 0) {
            return { success: false, error: "no workers — call init_workers first" };
        }
        const scales = [10, 50, 100];
        const rows = [];
        let baseThroughput = 0;
        for (const n of scales) {
            const targets = available.slice(0, Math.min(n, available.length));
            const { accepted, wall_time_ms } = this.doDispatch(targets, { task: "benchmark_job" });
            const throughput = wall_time_ms > 0 ? Math.floor((accepted * 1000) / wall_time_ms) : 0;
            if (rows.length === 0)
                baseThroughput = throughput > 0 ? throughput : 1;
            const speedupPct = Math.floor((throughput * 100) / baseThroughput);
            rows.push({ workers: targets.length, accepted, wall_time_ms, throughput, speedup_pct: speedupPct });
        }
        return { success: true, scales: rows };
    }
    onGet_metrics() {
        const rttAvg = this.state.ask_rtt_count > 0
            ? Math.floor(this.state.ask_rtt_sum_ms / this.state.ask_rtt_count)
            : 0;
        const throughput = this.state.wall_time_ms > 0
            ? Math.floor((this.state.total_accepted * 1000) / this.state.wall_time_ms)
            : 0;
        return {
            success: true,
            total_dispatched: this.state.total_dispatched,
            total_accepted: this.state.total_accepted,
            total_rejected: this.state.total_rejected,
            dispatch_count: this.state.dispatch_count,
            wall_time_ms: this.state.wall_time_ms,
            throughput_per_s: throughput,
            heartbeat_count: this.state.heartbeat_count,
            ask_rtt_min_ms: this.state.ask_rtt_min_ms,
            ask_rtt_max_ms: this.state.ask_rtt_max_ms,
            ask_rtt_avg_ms: rttAvg,
            worker_count: this.state.worker_ids.length,
            shard_group_id: this.state.shard_group_id,
        };
    }
    // ── UC1-UC5: Flow Control Use Cases ──────────────────────────────────────
    onThundering_herd_test(payload) {
        const workers = this.state.worker_ids;
        if (workers.length === 0)
            return { error: "call init_workers first" };
        const count = typeof payload.count === "number" && payload.count > 0
            ? Math.min(payload.count, workers.length) : workers.length;
        const targets = workers.slice(0, count);
        const { accepted, rejected, wall_time_ms } = this.doDispatch(targets, { task: "thundering_herd", priority: "high" });
        return { targets: targets.length, accepted, rejected, wall_time_ms };
    }
    onConfig_wave_deploy(payload) {
        const workers = this.state.worker_ids;
        if (workers.length === 0)
            return { error: "call init_workers first" };
        const waveSize = typeof payload.wave_size === "number" ? payload.wave_size : 20;
        const innerPayload = { task: "config_download", version: "2.1.0" };
        let totalAccepted = 0, totalRejected = 0, waveCount = 0;
        const waves = [];
        for (let i = 0; i < workers.length; i += waveSize) {
            const targets = workers.slice(i, i + waveSize);
            const { accepted, rejected, wall_time_ms } = this.doDispatch(targets, innerPayload);
            totalAccepted += accepted;
            totalRejected += rejected;
            waveCount++;
            waves.push({ wave: waveCount, accepted, rejected, wall_time_ms });
        }
        return { wave_count: waveCount, total_accepted: totalAccepted, total_rejected: totalRejected, waves };
    }
    onEnhanced_heartbeat(payload) {
        const count = typeof payload.count === "number" ? payload.count : 50;
        const workers = this.state.worker_ids.length > 0
            ? this.state.worker_ids : ["JobQueueWorker:worker-0"];
        const ctx = JSON.stringify({ tenant: "default", namespace: "default" });
        let totalAccepted = 0, totalRejected = 0;
        for (let i = 1; i <= count; i++) {
            const payloadBytes = new TextEncoder().encode(JSON.stringify({ type: "heartbeat", seq: i, cpu: 0.42 }));
            try {
                const result = host.outbox.dispatch(ctx, DISPATCHER_ID, workers, payloadBytes, "cast", HEARTBEAT_IDEM_KEY, "overwrite");
                totalAccepted += result[0];
                totalRejected += result[1];
            }
            catch {
                totalRejected += workers.length;
            }
        }
        this.state.heartbeat_count = count;
        return { heartbeats_sent: count, total_accepted: totalAccepted, total_rejected: totalRejected, worker_count: workers.length };
    }
    onTelemetry_test(payload) {
        const workers = this.state.worker_ids;
        if (workers.length === 0)
            return { error: "call init_workers first" };
        const count = typeof payload.count === "number" ? payload.count : 50;
        let fastPath = 0, fallback = 0;
        const ctx = JSON.stringify({ tenant: "default", namespace: "default" });
        for (let i = 0; i < count; i++) {
            const target = workers[i % workers.length];
            try {
                host.ask(target, "process", { type: "telemetry", seq: i }, 2000);
                fastPath++;
            }
            catch {
                const payloadBytes = new TextEncoder().encode(JSON.stringify({ type: "telemetry", seq: i }));
                try {
                    host.outbox.dispatch(ctx, DISPATCHER_ID, [target], payloadBytes, "cast", "", "");
                }
                catch { /* best-effort fallback */ }
                fallback++;
            }
        }
        return { total_sent: count, fast_path_success: fastPath, outbox_fallback: fallback };
    }
    onReliable_delivery_test(payload) {
        const workers = this.state.worker_ids;
        if (workers.length === 0)
            return { error: "call init_workers first" };
        const count = typeof payload.count === "number" ? payload.count : 50;
        const targets = [];
        for (let i = 0; i < count; i++)
            targets.push(workers[i % workers.length]);
        const { accepted, rejected, wall_time_ms } = this.doDispatch(targets, { task: "reliable_delivery_item" });
        return { dispatched: count, accepted, rejected, wall_time_ms };
    }
    // Backward-compat handlers kept for existing test.sh compatibility
    onDispatch(payload) {
        const targets = payload.targets ?? [];
        const innerPayload = payload.payload ?? { task: "process" };
        const { accepted, rejected } = this.doDispatch(targets, innerPayload);
        return { success: true, accepted, rejected, total_dispatched: this.state.total_dispatched };
    }
    onStatus() {
        const ctx = JSON.stringify({ tenant: "default", namespace: "default" });
        let relayStatus = {};
        try {
            const raw = host.outbox.status(ctx, DISPATCHER_ID);
            relayStatus = raw ? JSON.parse(raw) : {};
        }
        catch {
            relayStatus = { error: "status unavailable" };
        }
        return { success: true, dispatcher_id: DISPATCHER_ID, relay_status: relayStatus };
    }
}
class JobQueueWorkerActor extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", jobs_received: 0, last_heartbeat_seq: 0 };
    }
    onInit(config) {
        if (typeof config.actor_id === "string" && config.actor_id) {
            this.state.actor_id = config.actor_id;
        }
    }
    onProcess(_payload) {
        this.state.jobs_received++;
        return {};
    }
    // Cast alias — outbox dispatches as "cast" by default
    onCast(payload) {
        return this.onProcess(payload);
    }
    onUpgrade(_payload) {
        return {
            success: true,
            status: "need_package",
            version: "2.0.0",
            worker_id: this.state.actor_id,
        };
    }
    onHeartbeat(payload) {
        const seq = typeof payload.seq === "number" ? payload.seq : 0;
        if (seq > this.state.last_heartbeat_seq) {
            this.state.last_heartbeat_seq = seq;
        }
        return {};
    }
    onStats() {
        return {
            success: true,
            jobs_received: this.state.jobs_received,
            last_heartbeat_seq: this.state.last_heartbeat_seq,
            actor_id: this.state.actor_id,
        };
    }
}
// ============================================================================
// Registration
// ============================================================================
const router = new ActorRouter({
    JobQueueLeader: () => new JobQueueLeaderActor(),
    JobQueueWorker: () => new JobQueueWorkerActor(),
    JobQueueOutboxCoordinator: () => new JobQueueLeaderActor(), // backward compat
    OutboxWorker: () => new JobQueueWorkerActor(), // backward compat
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
