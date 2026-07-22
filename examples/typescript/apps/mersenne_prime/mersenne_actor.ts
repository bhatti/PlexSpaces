// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WASM actors for the distributed Mersenne prime example.
// Componentized to .wasm by jco, deployed to a PlexSpaces node.
//
// Actors:
//   CoordinatorActor — work-queue manager; assigns candidates to thin-node workers,
//                      collects results in TupleSpace, re-assigns on worker "ready".
//   CodeServerActor  — serves the Lucas-Lehmer Web Worker JS string on demand.

import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

// Known Mersenne prime exponents — ordered ascending.
// First 8 (up to 31) are trivially fast; up to index 21 (p=9941) gives real
// browser-visible computation. Test.sh uses first 8 for speed.
const CANDIDATES = [
  2, 3, 5, 7, 13, 17, 19, 31,           // fast  (< 1ms)
  61, 89, 107, 127,                       // medium (ms range)
  521, 607, 1279, 2203, 2281, 3217,       // slow   (100ms–seconds)
  4253, 4423, 9689, 9941,                 // heavy  (seconds–tens of seconds)
];

const WORKER_GROUP = "mersenne_workers";

// Lucas-Lehmer JS served verbatim by CodeServerActor.get_worker_js().
// Loaded by browser as a Web Worker Blob URL — no eval(), pure JS BigInt.
// Records actual wall-clock duration_ms per candidate.
const WORKER_JS = `
self.onmessage = function(e) {
  const p = e.data.p;
  const t0 = Date.now();
  const is_prime = lucasLehmer(BigInt(p));
  self.postMessage({ p, is_prime, duration_ms: Date.now() - t0 });
};
function lucasLehmer(p) {
  if (p === 2n) return true;
  const mp = (1n << p) - 1n;
  let s = 4n;
  for (let i = 0n; i < p - 2n; i++) { s = (s * s - 2n) % mp; }
  return s === 0n;
}
`.trim();

// ─── CoordinatorActor ─────────────────────────────────────────────────────────
//
// Workers join via "join" (persists across runs), "start" resets the work queue
// and dispatches to all already-registered workers.  This means each browser tab
// that connects calls "join" once; any tab can then call "start" to kick off a run.

interface WorkItem {
  p: number;
  status: 'pending' | 'assigned' | 'done';
  worker?: string;
  is_prime?: boolean;
  duration_ms?: number;
  assigned_at?: number;
  completed_at?: number;
}

interface WorkerInfo {
  cpu_cores: number;
  items_done: number;
  total_duration_ms: number;
  last_seen_ms: number;
  current_p?: number;
}

type CoordinatorState = {
  work: Record<string, WorkItem>;   // keyed by String(p)
  workers: Record<string, WorkerInfo>;
  started_at: number;
  single_worker_baseline_ms: number;  // first solo run elapsed, for speedup calc
}

interface StartPayload { count?: number }
interface JoinPayload  { actor_id: string; cpu_cores?: number }
interface ResultPayload { p: number; is_prime: boolean; duration_ms?: number; actor_id?: string }

class CoordinatorActor extends PlexSpacesActor<CoordinatorState> {
  getDefaultState(): CoordinatorState {
    return { work: {}, workers: {}, started_at: 0, single_worker_baseline_ms: 0 };
  }

  // Join: register as a worker without touching the work queue.
  // Each browser tab calls this once on connect.
  onJoin(payload: JoinPayload): unknown {
    if (!payload?.actor_id) return { error: 'actor_id required' };
    const cpuCores = payload.cpu_cores ?? 1;
    const now = host.nowMs();
    const existing = this.state.workers[payload.actor_id];
    this.state.workers[payload.actor_id] = {
      cpu_cores: cpuCores,
      items_done: existing?.items_done ?? 0,
      total_duration_ms: existing?.total_duration_ms ?? 0,
      last_seen_ms: now,
    };
    // Join process group so status() can discover workers via PG membership
    try { host.processGroups.join(WORKER_GROUP); } catch { /* already joined */ }
    return { ok: true, worker_count: Object.keys(this.state.workers).length };
  }

  // Leave: remove from worker registry (called on disconnect)
  onLeave(payload: JoinPayload): unknown {
    if (payload?.actor_id) delete this.state.workers[payload.actor_id];
    return { ok: true };
  }

  // Start: reset work queue and immediately dispatch to all registered workers.
  onStart(payload: StartPayload): unknown {
    const count = typeof payload?.count === 'number' ? payload.count : CANDIDATES.length;
    const selected = CANDIDATES.slice(0, Math.min(count, CANDIDATES.length));
    this.state.work = {};
    for (const p of selected) {
      this.state.work[String(p)] = { p, status: 'pending' };
    }
    this.state.started_at = host.nowMs();

    // Dispatch first work item to each registered worker immediately.
    // Stale workers from a previous run may still be in the map; remove them on send failure.
    let dispatched = 0;
    for (const [actorId, info] of Object.entries(this.state.workers)) {
      const next = this._nextPending(info.cpu_cores);
      if (!next) break;
      next.status = 'assigned';
      next.worker = actorId;
      next.assigned_at = host.nowMs();
      this.state.workers[actorId]!.current_p = next.p;
      try {
        host.send(actorId, 'assign_work', { p: next.p, done: false });
        dispatched++;
      } catch {
        // Worker disconnected — revert and remove
        next.status = 'pending';
        next.worker = undefined;
        delete this.state.workers[actorId];
      }
    }
    return { total: selected.length, workers_dispatched: dispatched };
  }

  // Ready: legacy path for test.sh workers that call "ready" instead of "join"+"start"
  onReady(payload: JoinPayload): unknown {
    this.onJoin(payload);
    // If a run is active (has pending work), assign immediately
    const cpuCores = payload.cpu_cores ?? 1;
    const next = this._nextPending(cpuCores);
    if (!next) {
      try { host.send(payload.actor_id, 'assign_work', { p: null, done: true }); } catch { /* disconnected */ }
      return { done: true };
    }
    next.status = 'assigned';
    next.worker = payload.actor_id;
    next.assigned_at = host.nowMs();
    if (this.state.workers[payload.actor_id]) {
      this.state.workers[payload.actor_id]!.current_p = next.p;
    }
    try {
      host.send(payload.actor_id, 'assign_work', { p: next.p, done: false });
    } catch {
      next.status = 'pending';
      next.worker = undefined;
      delete this.state.workers[payload.actor_id];
      return { error: 'worker unreachable' };
    }
    return { assigned: next.p };
  }

  onResult(payload: ResultPayload): unknown {
    const item = this.state.work[String(payload?.p)];
    if (!item) return { ok: false, error: 'unknown candidate' };

    const now = host.nowMs();
    item.status = 'done';
    item.is_prime = Boolean(payload.is_prime);
    item.duration_ms = payload.duration_ms ?? 0;
    item.completed_at = now;

    const actorId = payload.actor_id ?? item.worker;
    if (actorId && this.state.workers[actorId]) {
      const w = this.state.workers[actorId]!;
      w.items_done += 1;
      w.total_duration_ms += item.duration_ms;
      w.last_seen_ms = now;
      w.current_p = undefined;
    }

    host.ts.write(['result', String(payload.p), item.is_prime ? 'true' : 'false',
      String(item.duration_ms), actorId ?? 'unknown']);
    host.incrCounter('ts-mersenne-prime', item.is_prime ? 'primes_found' : 'composites_found');

    if (actorId) {
      const cpuCores = this.state.workers[actorId]?.cpu_cores ?? 1;
      const next = this._nextPending(cpuCores);
      if (next) {
        next.status = 'assigned';
        next.worker = actorId;
        next.assigned_at = now;
        if (this.state.workers[actorId]) this.state.workers[actorId]!.current_p = next.p;
        try {
          host.send(actorId, 'assign_work', { p: next.p, done: false });
        } catch {
          next.status = 'pending';
          next.worker = undefined;
          delete this.state.workers[actorId];
        }
      } else {
        if (this.state.workers[actorId]) this.state.workers[actorId]!.current_p = undefined;
        try { host.send(actorId, 'assign_work', { p: null, done: true }); } catch { /* disconnected, ignore */ }
      }
    }
    return { ok: true };
  }

  onStatus(): unknown {
    const items = Object.values(this.state.work) as WorkItem[];
    const done = items.filter(w => w.status === 'done');
    const assigned = items.filter(w => w.status === 'assigned');
    const pending = items.filter(w => w.status === 'pending');
    const foundPrimes = done.filter(w => w.is_prime).map(w => w.p).sort((a, b) => a - b);

    const now = host.nowMs();
    const elapsed_ms = this.state.started_at > 0 ? now - this.state.started_at : 0;
    const throughput_per_s = elapsed_ms > 0 ? (done.length / elapsed_ms) * 1000 : 0;

    // Collect PG members for "all connected workers" even across browser tabs
    let pg_members: string[] = [];
    try { pg_members = (host.processGroups.members(WORKER_GROUP) as string[] | null) ?? []; } catch { /**/ }

    const worker_count = Math.max(
      Object.keys(this.state.workers).length,
      pg_members.length,
    );

    // Per-worker details
    const worker_details = Object.entries(this.state.workers).map(([id, w]) => ({
      actor_id: id,
      cpu_cores: w.cpu_cores,
      items_done: w.items_done,
      avg_duration_ms: w.items_done > 0 ? Math.round(w.total_duration_ms / w.items_done) : 0,
      total_duration_ms: w.total_duration_ms,
      current_p: w.current_p ?? null,
      idle_ms: now - w.last_seen_ms,
    }));

    // Speedup vs. single-worker baseline (stored on first completed solo run)
    const total_work_ms = worker_details.reduce((s, w) => s + w.total_duration_ms, 0);
    if (done.length === items.length && items.length > 0 && worker_count === 1
        && elapsed_ms > 0 && this.state.single_worker_baseline_ms === 0) {
      this.state.single_worker_baseline_ms = elapsed_ms;
    }
    const speedup = this.state.single_worker_baseline_ms > 0 && elapsed_ms > 0
      ? this.state.single_worker_baseline_ms / elapsed_ms : null;
    const efficiency_pct = speedup != null && worker_count > 0
      ? Math.round((speedup / worker_count) * 100) : null;

    const candidates_detail = items.map(item => ({
      p: item.p, status: item.status,
      is_prime: item.is_prime ?? null,
      duration_ms: item.duration_ms ?? null,
      worker: item.worker ?? null,
    }));

    return {
      total: items.length,
      completed: done.length,
      assigned: assigned.length,
      pending: pending.length,
      found_primes: foundPrimes,
      workers_active: worker_count,
      worker_details,
      candidates_detail,
      elapsed_ms,
      throughput_per_s: Math.round(throughput_per_s * 100) / 100,
      total_work_ms,
      speedup,
      efficiency_pct,
      started_at: this.state.started_at,
      pg_members,
    };
  }

  private _nextPending(cpuCores: number): WorkItem | null {
    const pending = (Object.values(this.state.work) as WorkItem[])
      .filter(w => w.status === 'pending')
      .sort((a, b) => cpuCores <= 2 ? a.p - b.p : b.p - a.p);
    return pending[0] ?? null;
  }
}

// ─── CodeServerActor ──────────────────────────────────────────────────────────

class CodeServerActor extends PlexSpacesActor<Record<string, never>> {
  getDefaultState(): Record<string, never> { return {}; }

  onGet_worker_js(): unknown {
    return { code: WORKER_JS };
  }
}

// ─── Router ───────────────────────────────────────────────────────────────────

const router = new ActorRouter({
  "CoordinatorActor": () => new CoordinatorActor(),
  "CodeServerActor": () => new CodeServerActor(),
});

export const actor = {
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
