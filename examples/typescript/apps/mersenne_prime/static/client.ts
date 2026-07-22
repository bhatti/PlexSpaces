// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Browser thin-node client for mersenne_prime distributed computation.
//
// Flow (per browser tab):
//   1. Connect  → WsThinClient registers with PlexSpaces, calls coordinator "join"
//   2. Start Run → any connected tab calls coordinator "start" → work dispatched to all workers
//   3. Workers receive "assign_work" tells, compute Lucas-Lehmer, send back "result"
//   4. Poll status every 1.5s — shows all workers across ALL tabs via ProcessGroup

import { ActorID, WsThinClient } from "@plexspaces/sdk";

// ─── DOM refs ────────────────────────────────────────────────────────────────
const wsUrlInput       = document.getElementById("ws-url") as HTMLInputElement;
const jwtInput         = document.getElementById("jwt-token") as HTMLInputElement;
const leaderNodeInput  = document.getElementById("leader-node-id") as HTMLInputElement;
const candidateCountInput = document.getElementById("candidate-count") as HTMLInputElement;
const connectBtn       = document.getElementById("connect-btn") as HTMLButtonElement;
const disconnectBtn    = document.getElementById("disconnect-btn") as HTMLButtonElement;
const startRunBtn      = document.getElementById("start-run-btn") as HTMLButtonElement;

// metrics strip
const mStatus     = document.getElementById("m-status")!;
const mProgress   = document.getElementById("m-progress")!;
const mPct        = document.getElementById("m-pct")!;
const mPrimes     = document.getElementById("m-primes")!;
const mWorkers    = document.getElementById("m-workers")!;
const mCores      = document.getElementById("m-cores")!;
const mThroughput = document.getElementById("m-throughput")!;
const mElapsed    = document.getElementById("m-elapsed")!;
const mSpeedup    = document.getElementById("m-speedup")!;
const mNode       = document.getElementById("m-node")!;
const mMyCores    = document.getElementById("m-my-cores")!;
const progDetail  = document.getElementById("prog-detail")!;
const progressBarInner = document.getElementById("progress-bar-inner") as HTMLDivElement;
const candidatesDiv    = document.getElementById("candidates") as HTMLDivElement;
const workersList      = document.getElementById("workers-list")!;
const logDiv           = document.getElementById("log")!;

// ─── State ───────────────────────────────────────────────────────────────────
let client: WsThinClient | null = null;
let worker: Worker | null = null;
let pollTimer: ReturnType<typeof setInterval> | null = null;
let elapsedTimer: ReturnType<typeof setInterval> | null = null;
let coordinatorId = "";
let myActorId = "";
let startedAt = 0;
let isRunning = false;   // true while a computation run is in progress
const APP_NS = "ts-mersenne-prime";
const CANDIDATES = [
  2, 3, 5, 7, 13, 17, 19, 31,
  61, 89, 107, 127,
  521, 607, 1279, 2203, 2281, 3217,
  4253, 4423, 9689, 9941,
];

// ─── Helpers ─────────────────────────────────────────────────────────────────
function log(text: string, cls = "info"): void {
  const div = document.createElement("div");
  div.className = `log-entry ${cls}`;
  div.textContent = `${new Date().toLocaleTimeString()} ${text}`;
  logDiv.insertBefore(div, logDiv.firstChild);
  while (logDiv.children.length > 150) logDiv.removeChild(logDiv.lastChild!);
}

function shortId(actorId: string): string {
  const idx = actorId.indexOf("//");
  return idx >= 0 ? actorId.slice(0, idx) : actorId.slice(0, 12);
}

function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m${Math.floor((ms % 60000) / 1000)}s`;
}

function setButtonState(connected: boolean, running: boolean): void {
  connectBtn.disabled = connected;
  disconnectBtn.disabled = !connected;
  startRunBtn.disabled = !connected || running;
  startRunBtn.textContent = running ? "Running…" : "Start New Run";
}

// ─── Elapsed timer ───────────────────────────────────────────────────────────
function startElapsedTimer(): void {
  if (elapsedTimer) clearInterval(elapsedTimer);
  elapsedTimer = setInterval(() => {
    if (startedAt > 0 && isRunning) {
      const s = Math.floor((Date.now() - startedAt) / 1000);
      mElapsed.textContent = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
    }
  }, 500);
}

// ─── Candidate tiles ─────────────────────────────────────────────────────────
interface CandidateDetail {
  p: number; status: 'pending' | 'assigned' | 'done';
  is_prime: boolean | null; duration_ms: number | null; worker: string | null;
}

function renderCandidates(details: CandidateDetail[], total: number): void {
  const byP = new Map(details.map(d => [d.p, d]));
  candidatesDiv.innerHTML = "";
  for (const p of CANDIDATES.slice(0, total)) {
    const d = byP.get(p);
    const div = document.createElement("div");
    if (!d || d.status === 'pending') {
      div.className = "candidate pending";
      div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>−1</div><div class="cand-dur">pending</div>`;
    } else if (d.status === 'assigned') {
      div.className = "candidate assigned";
      div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>−1</div><div class="cand-dur">⚙ computing…</div><div class="cand-worker">${d.worker ? shortId(d.worker) : ""}</div>`;
    } else if (d.is_prime) {
      div.className = "candidate done-prime";
      div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>−1 ✓</div><div class="cand-dur">${d.duration_ms != null ? fmtDuration(d.duration_ms) : ""}</div><div class="cand-worker">${d.worker ? shortId(d.worker) : ""}</div>`;
    } else {
      div.className = "candidate done-composite";
      div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>−1 ✗</div><div class="cand-dur">${d.duration_ms != null ? fmtDuration(d.duration_ms) : ""}</div>`;
    }
    candidatesDiv.appendChild(div);
  }
}

// ─── Worker cards ────────────────────────────────────────────────────────────
interface WorkerDetail {
  actor_id: string; cpu_cores: number; items_done: number;
  avg_duration_ms: number; total_duration_ms: number;
  current_p: number | null; idle_ms: number;
}

function renderWorkers(workers: WorkerDetail[], totalWorkers: number): void {
  if (!workers.length) {
    workersList.innerHTML = `<div id="workers-empty">No workers yet — open a tab and connect</div>`;
    return;
  }
  workersList.innerHTML = "";

  // Summary row
  const totalCores = workers.reduce((s, w) => s + w.cpu_cores, 0);
  const totalDone  = workers.reduce((s, w) => s + w.items_done, 0);
  const summary = document.createElement("div");
  summary.className = "worker-summary";
  summary.innerHTML = `<span>${workers.length} worker${workers.length !== 1 ? "s" : ""} connected</span><span>${totalCores} total cores · ${totalDone} items processed</span>`;
  workersList.appendChild(summary);

  for (const w of workers) {
    const isMe = w.actor_id === myActorId;
    const busy = w.current_p != null;
    const throughputPerS = w.total_duration_ms > 0
      ? (w.items_done / w.total_duration_ms * 1000).toFixed(1) : "—";
    const card = document.createElement("div");
    card.className = `worker-card${isMe ? " mine" : ""}`;
    card.innerHTML = `
      <div class="worker-name">
        <span>${isMe ? "★ " : ""}${shortId(w.actor_id)}</span>
        <span class="badge ${busy ? "" : "idle"}">${busy ? `⚙ 2^${w.current_p}` : "idle"}</span>
      </div>
      <div class="worker-stats">
        <div class="wstat">Cores <span>${w.cpu_cores}</span></div>
        <div class="wstat">Done <span>${w.items_done}</span></div>
        <div class="wstat">Avg <span>${w.avg_duration_ms > 0 ? fmtDuration(w.avg_duration_ms) : "—"}</span></div>
        <div class="wstat">Throughput <span>${throughputPerS}/s</span></div>
        <div class="wstat">Status <span>${w.idle_ms < 5000 ? "active" : "idle " + fmtDuration(w.idle_ms)}</span></div>
      </div>`;
    workersList.appendChild(card);
  }
}

// ─── Status update ───────────────────────────────────────────────────────────
interface StatusResp {
  total?: number; completed?: number; assigned?: number; pending?: number;
  found_primes?: number[];
  workers_active?: number;
  worker_details?: WorkerDetail[];
  candidates_detail?: CandidateDetail[];
  elapsed_ms?: number; throughput_per_s?: number; total_work_ms?: number;
  speedup?: number | null; efficiency_pct?: number | null;
  started_at?: number; pg_members?: string[];
}

function applyStatus(status: StatusResp): void {
  const total     = status.total ?? 0;
  const completed = status.completed ?? 0;
  const assigned  = status.assigned ?? 0;
  const pending   = status.pending ?? 0;
  const found     = status.found_primes ?? [];
  const pct       = total > 0 ? Math.round((completed / total) * 100) : 0;
  const workers   = status.worker_details ?? [];
  const throughput = status.throughput_per_s ?? 0;

  progressBarInner.style.width = `${pct}%`;
  mProgress.textContent = `${completed}/${total}`;
  mPct.textContent = `${pct}%`;
  mPrimes.textContent = String(found.length);
  mWorkers.textContent = String(status.workers_active ?? workers.length);
  const totalCores = workers.reduce((s, w) => s + w.cpu_cores, 0);
  mCores.textContent = totalCores > 0 ? `${totalCores} total cores` : "—";
  mThroughput.textContent = throughput > 0 ? throughput.toFixed(2) : "—";

  if (status.speedup != null) {
    const eff = status.efficiency_pct != null ? ` (${status.efficiency_pct}% eff)` : "";
    mSpeedup.textContent = `${status.speedup.toFixed(2)}x${eff}`;
  } else {
    mSpeedup.textContent = "—";
  }

  progDetail.textContent = `${completed} done · ${assigned} active · ${pending} pending`;
  if (status.started_at && status.started_at > 0) startedAt = status.started_at;

  // Detect completion: all done, none pending/assigned
  const runComplete = total > 0 && completed >= total && assigned === 0;
  if (runComplete && isRunning) {
    isRunning = false;
    mStatus.textContent = "Complete ✓";
    if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
    setButtonState(true, false);
    log(`Done! Found ${found.length} primes in ${fmtDuration(status.elapsed_ms ?? 0)}.`, "prime");
  }

  if (status.candidates_detail) renderCandidates(status.candidates_detail, total);
  renderWorkers(workers, status.workers_active ?? workers.length);
}

// ─── Web Worker ──────────────────────────────────────────────────────────────
function spawnWorker(code: string): Worker {
  const blob = new Blob([code], { type: "application/javascript" });
  const url = URL.createObjectURL(blob);
  const w = new Worker(url, { type: "classic" });
  URL.revokeObjectURL(url);
  return w;
}

// ─── Connect ──────────────────────────────────────────────────────────────────
connectBtn.addEventListener("click", async () => {
  const wsUrl = wsUrlInput.value.trim();
  const jwtToken = jwtInput.value.trim() || undefined;
  const leaderNodeId = leaderNodeInput.value.trim() || "test-node-8091";

  if (!wsUrl) { log("Please enter a WebSocket URL", "error"); return; }

  connectBtn.disabled = true;
  mStatus.textContent = "Connecting…";

  try {
    client = new WsThinClient({ wsUrl, jwtToken, nodeId: WsThinClient.newUlid(), namespace: APP_NS });
    const nodeId = await client.connect();

    const myCores = navigator.hardwareConcurrency ?? 1;
    mNode.textContent = nodeId.slice(-10);
    mMyCores.textContent = `${myCores} cores`;
    log(`Connected as …${nodeId.slice(-8)} (${myCores} cores)`);

    myActorId = client.localActorId(nodeId.slice(-8), "WorkerNode", APP_NS);
    coordinatorId = new ActorID("mersenne", "CoordinatorActor", APP_NS, leaderNodeId).toString();
    const codeServerId = new ActorID("compute-js", "CodeServerActor", APP_NS, leaderNodeId).toString();

    // Fetch worker JS
    const codeResp = await client.ask(codeServerId, "get_worker_js", {}, 10_000) as { code?: string };
    if (!codeResp?.code) throw new Error("CodeServerActor returned empty worker JS");
    log("Lucas-Lehmer worker JS received");

    // Spawn Web Worker
    worker = spawnWorker(codeResp.code);
    worker.onmessage = (e: MessageEvent<{ p: number; is_prime: boolean; duration_ms: number }>) => {
      const { p, is_prime, duration_ms } = e.data;
      log(is_prime ? `2^${p}−1 PRIME ✓ (${fmtDuration(duration_ms)})` : `2^${p}−1 composite (${fmtDuration(duration_ms)})`,
        is_prime ? "prime" : "info");
      client?.tell(coordinatorId, "result", { p, is_prime, duration_ms, actor_id: myActorId }).catch(() => {});
    };

    // Handle incoming work assignments
    client.onMessage((_from: string, msgType: string, payload: unknown) => {
      if (msgType === "assign_work") {
        const pw = payload as { p?: number; done?: boolean };
        if (pw.done || pw.p == null) { log("No more work for this worker"); return; }
        log(`Assigned: 2^${pw.p}−1`, "assign");
        worker?.postMessage({ p: pw.p });
      }
    });

    // Register as worker (does NOT start a run)
    await client.ask(coordinatorId, "join", { actor_id: myActorId, cpu_cores: myCores }, 10_000);
    log(`Joined coordinator as worker (${myCores} cores) — click "Start New Run" to begin`);
    mStatus.textContent = "Idle (connected)";

    // Fetch initial status to show existing state
    try {
      const status = await client.ask(coordinatorId, "status", {}, 5_000) as StatusResp;
      applyStatus(status);
    } catch { /**/ }

    setButtonState(true, false);
    startElapsedTimer();

  } catch (err) {
    log(`Error: ${(err as Error).message}`, "error");
    mStatus.textContent = "Error";
    connectBtn.disabled = false;
    client = null;
  }
});

// ─── Start Run ────────────────────────────────────────────────────────────────
startRunBtn.addEventListener("click", async () => {
  if (!client) return;
  const count = Math.min(parseInt(candidateCountInput.value) || 20, CANDIDATES.length);
  setButtonState(true, true);
  isRunning = true;
  mStatus.textContent = "Running…";
  startedAt = Date.now();

  try {
    const startResp = await client.ask(coordinatorId, "start", { count }, 10_000) as {
      total?: number; workers_dispatched?: number;
    };
    log(`Run started — ${startResp.total ?? count} candidates, ${startResp.workers_dispatched ?? "?"} workers`);
  } catch (err) {
    log(`Start failed: ${(err as Error).message}`, "error");
    isRunning = false;
    setButtonState(true, false);
    return;
  }

  // Poll status
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = setInterval(async () => {
    if (!client) return;
    try {
      const status = await client.ask(coordinatorId, "status", {}, 5_000) as StatusResp;
      applyStatus(status);
    } catch { /**/ }
  }, 1500);
});

// ─── Disconnect ───────────────────────────────────────────────────────────────
disconnectBtn.addEventListener("click", async () => {
  if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = null; }
  // Leave coordinator registry
  if (client && myActorId) {
    client.tell(coordinatorId, "leave", { actor_id: myActorId }).catch(() => {});
  }
  worker?.terminate(); worker = null;
  await client?.disconnect(); client = null;
  isRunning = false;
  mStatus.textContent = "Disconnected";
  setButtonState(false, false);
  log("Disconnected");
});
