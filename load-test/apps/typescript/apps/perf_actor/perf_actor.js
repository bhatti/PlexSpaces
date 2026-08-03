// SPDX-License-Identifier: AGPL-3.0-or-later
// PerfActor — TypeScript WASM actor for PlexSpaces load testing.
//
// Operations: echo, compute (Mersenne prime), kv_put/kv_get, pg_broadcast, shard_task, get_stats.
// Identical semantics to the Python / Go / Rust WASM variants.
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
// ─── Lucas-Lehmer test ────────────────────────────────────────────────────────
function isMersennePrime(p) {
    if (p === 2)
        return true;
    if (p < 2)
        return false;
    const mp = (1n << BigInt(p)) - 1n;
    let s = 4n;
    for (let i = 0; i < p - 2; i++) {
        s = ((s * s) - 2n) % mp;
    }
    return s === 0n;
}
function gradientStep(values, lr = 0.01) {
    const n = values.length;
    if (n === 0)
        return { gradient: 0, count: 0 };
    const mean = values.reduce((a, b) => a + b, 0) / n;
    const gradient = values.reduce((acc, v) => acc + (v - mean) ** 2, 0) / n;
    const sample = values.slice(0, 3).map(v => v - lr * (v - mean));
    return { gradient, count: n, mean, sample };
}
// ─── Actor ────────────────────────────────────────────────────────────────────
class PerfActor extends PlexSpacesActor {
    getDefaultState() {
        return { echo_count: 0, compute_count: 0, kv_count: 0, pg_count: 0, shard_count: 0 };
    }
    onInit(_config) {
        // nothing needed — state is initialized via getDefaultState
    }
    onEcho(payload) {
        this.state.echo_count++;
        return { ok: true, echo: payload, count: this.state.echo_count };
    }
    onCompute(payload) {
        const p = typeof payload.p === "number" ? payload.p : 7;
        const result = isMersennePrime(p);
        this.state.compute_count++;
        return { ok: true, p, is_mersenne_prime: result, count: this.state.compute_count };
    }
    onKv_put(payload) {
        const key = typeof payload.key === "string" ? payload.key : "perf_key";
        const value = typeof payload.value === "string" ? payload.value : "perf_val";
        host.kv.put(key, JSON.stringify(value));
        this.state.kv_count++;
        return { ok: true, key, count: this.state.kv_count };
    }
    onKv_get(payload) {
        const key = typeof payload.key === "string" ? payload.key : "perf_key";
        const value = host.kv.get(key);
        return { ok: true, key, value };
    }
    onPg_broadcast(payload) {
        const group = typeof payload.group === "string" ? payload.group : "perf-group";
        const message = payload.message ?? { event: "ping" };
        host.processGroups.join(group);
        host.processGroups.broadcast(group, "perf_event", JSON.stringify(message));
        this.state.pg_count++;
        return { ok: true, group, count: this.state.pg_count };
    }
    onShard_task(payload) {
        const shardIndex = typeof payload.shard_index === "number" ? payload.shard_index : 0;
        const lr = typeof payload.lr === "number" ? payload.lr : 0.01;
        const rawValues = Array.isArray(payload.values)
            ? payload.values
            : Array.from({ length: 100 }, (_, i) => i);
        const stats = gradientStep(rawValues, lr);
        this.state.shard_count++;
        return { ok: true, shard_index: shardIndex, count: this.state.shard_count, ...stats };
    }
    onGet_stats(_payload) {
        return {
            ok: true,
            echo_count: this.state.echo_count,
            compute_count: this.state.compute_count,
            kv_count: this.state.kv_count,
            pg_count: this.state.pg_count,
            shard_count: this.state.shard_count,
        };
    }
}
// ─── Router ───────────────────────────────────────────────────────────────────
const router = new ActorRouter({ PerfActor: () => new PerfActor() });
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
