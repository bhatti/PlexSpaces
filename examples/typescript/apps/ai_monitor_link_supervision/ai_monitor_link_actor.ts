// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// AI Monitor/Link Supervision (TypeScript WASM)
//
// Demonstrates monitor and link primitives for fault-tolerant AI pipelines.
// Uses FLP/Byzantine fault detection as a realistic motivating scenario.
//
// Primitives:
//   host.monitor(actorId)    — one-way DOWN notification; supervisor stays alive
//   host.demonitor(ref)      — cancel watch when actor replaced
//   host.link(actorId)       — bidirectional EXIT fate-sharing (abnormal exits only)
//   host.unlink(actorId)     — decouple before graceful shutdown (no cascade)

import { ActorID, ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Build a canonical sibling ID from a bare child ID and the current actor's
 * own canonical ID. Supervised siblings have deterministic IDs:
 *   {child_id}//{actor_type}::{namespace}@{node}
 * where child_id == actor_type (from ChildSpec.id == ChildSpec.actor_type).
 * If bareId already contains "//" it is canonical and returned unchanged.
 */
function siblingId(bareId: string, selfActorId: string): string {
  if (!bareId) return bareId;
  if (bareId.includes("//")) return bareId;
  try {
    const self = ActorID.parse(selfActorId);
    return self.withTypeAndName(bareId, bareId).toString();
  } catch {
    return bareId;
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

type MonitorEntry = { workerId: string; monitorRef: string };
type DownEvent = { monitorRef: string; actorId: string; reason: string };

type InferenceWorkerState = {
  actorId: string;
  workerId: string;
  mode: "normal" | "byzantine";
  totalRequests: number;
  errorCount: number;
  linkedPeers: string[];
};

type ValidatorState = {
  actorId: string;
  totalValidations: number;
  passCount: number;
  failCount: number;
  byzantineCount: number;
  monitorRefs: MonitorEntry[];
  downEvents: DownEvent[];
};

type SupervisorState = {
  actorId: string;
  workerPool: string[];
  monitorRefs: MonitorEntry[];
  downEventsReceived: number;
  totalDispatched: number;
  nextWorkerIdx: number;
};

type AuditLogState = {
  eventsReceived: number;
  lastEventType: string;
  lastActorId: string;
};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

const FLP_THRESHOLD = 1.0 / 3.0;

const BYZANTINE_RESPONSES = [
  "42 is the answer to everything",
  "The sky is green on Tuesdays",
  "null",
  "ERROR: model checkpoint corrupted",
];

function isByzantineResponse(result: string): boolean {
  const lower = result.toLowerCase();
  if (
    lower.includes("42 is the answer") ||
    lower.includes("sky is green") ||
    lower === "null" ||
    lower.includes("checkpoint corrupted") ||
    lower.startsWith("error: ")
  ) {
    return true;
  }
  return result.trim().length < 10;
}

function normalInference(prompt: string): string {
  const lower = prompt.toLowerCase();
  if (lower.includes("actor")) {
    return "The actor model is a mathematical model of concurrent computation where each actor processes messages asynchronously.";
  }
  if (lower.includes("fault") || lower.includes("tolerance")) {
    return "Fault tolerance is achieved through redundancy, isolation, and supervision trees that restart failed components.";
  }
  if (lower.includes("flp") || lower.includes("impossibility")) {
    return "The FLP theorem proves no deterministic async protocol guarantees consensus with even one crash-faulty process.";
  }
  if (lower.includes("byzantine")) {
    return "Byzantine faults are arbitrary failures where a node may send inconsistent messages. Requires 3f+1 replicas.";
  }
  return `Processed: ${prompt.slice(0, 60)}`;
}

// ─────────────────────────────────────────────────────────────────────────────
// InferenceWorker
// ─────────────────────────────────────────────────────────────────────────────

class InferenceWorkerActor extends PlexSpacesActor<InferenceWorkerState> {
  getDefaultState(): InferenceWorkerState {
    return {
      actorId: "",
      workerId: "default-worker",
      mode: "normal",
      totalRequests: 0,
      errorCount: 0,
      linkedPeers: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = host.selfId?.() ?? "";
    const args = (config.args as Record<string, unknown>) ?? {};
    if (typeof args.worker_id === "string") {
      this.state.workerId = args.worker_id;
    }
  }

  protected "on__EXIT__"(payload: Record<string, unknown>): Record<string, unknown> {
    const exitFrom = String(payload.exit_from ?? "");
    const exitReason = String(payload.exit_reason ?? "");
    this.state.linkedPeers = this.state.linkedPeers.filter((p) => p !== exitFrom);
    host.log?.("info", `[${this.state.workerId}] __EXIT__ exit_from=${exitFrom} exit_reason=${exitReason}`);
    return {};
  }

  protected onInfer(payload: Record<string, unknown>): Record<string, unknown> {
    this.state.totalRequests++;
    const prompt = String(payload.prompt ?? "");
    const requestId = String(payload.request_id ?? "");

    if (this.state.mode === "byzantine") {
      const idx = this.state.totalRequests % BYZANTINE_RESPONSES.length;
      this.state.errorCount++;
      return {
        status: "ok",
        request_id: requestId,
        result: BYZANTINE_RESPONSES[idx],
        worker_id: this.state.workerId,
        mode: "byzantine",
      };
    }

    return {
      status: "ok",
      request_id: requestId,
      result: normalInference(prompt),
      worker_id: this.state.workerId,
      mode: "normal",
    };
  }

  protected onSet_mode(payload: Record<string, unknown>): Record<string, unknown> {
    const mode = String(payload.mode ?? "normal") as "normal" | "byzantine";
    this.state.mode = mode;
    return { status: "ok", mode };
  }

  protected onLink_with(payload: Record<string, unknown>): Record<string, unknown> {
    const peerId = siblingId(String(payload.peer_id ?? ""), this.state.actorId);
    if (!peerId) {
      return { error: "peer_id required" };
    }
    host.link?.(peerId);
    if (!this.state.linkedPeers.includes(peerId)) {
      this.state.linkedPeers.push(peerId);
    }
    return { status: "ok", peer_id: peerId };
  }

  protected onUnlink_from(payload: Record<string, unknown>): Record<string, unknown> {
    const rawPeer = String(payload.peer_id ?? this.state.linkedPeers[0] ?? "");
    const peerId = siblingId(rawPeer, this.state.actorId);
    host.unlink?.(peerId);
    this.state.linkedPeers = this.state.linkedPeers.filter((p) => p !== peerId);
    return { status: "ok", peer_id: peerId };
  }

  protected onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      status: "ok",
      worker_id: this.state.workerId,
      mode: this.state.mode,
      total_requests: this.state.totalRequests,
      error_count: this.state.errorCount,
      linked_peers: this.state.linkedPeers,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// ValidatorAgent
// ─────────────────────────────────────────────────────────────────────────────

class ValidatorAgentActor extends PlexSpacesActor<ValidatorState> {
  getDefaultState(): ValidatorState {
    return {
      actorId: "",
      totalValidations: 0,
      passCount: 0,
      failCount: 0,
      byzantineCount: 0,
      monitorRefs: [],
      downEvents: [],
    };
  }

  protected override onInit(_config: Record<string, unknown>): void {
    this.state.actorId = host.selfId?.() ?? "";
  }

  protected "on__DOWN__"(payload: Record<string, unknown>): Record<string, unknown> {
    const monitorRef = String(payload.monitor_ref ?? "");
    const downFrom = String(payload.down_from ?? "");
    const downReason = String(payload.down_reason ?? "");
    this.state.downEvents.push({ monitorRef, actorId: downFrom, reason: downReason });
    this.state.monitorRefs = this.state.monitorRefs.filter((m) => m.monitorRef !== monitorRef);
    host.log?.("info", `[validator_agent] __DOWN__ ref=${monitorRef} down_from=${downFrom} down_reason=${downReason}`);
    return {};
  }

  protected onMonitor_worker(payload: Record<string, unknown>): Record<string, unknown> {
    const canonical = siblingId(String(payload.worker_id ?? ""), this.state.actorId);
    if (!canonical) {
      return { error: "worker_id required" };
    }
    const monitorRef = host.monitor?.(canonical) ?? `ref-${Date.now()}`;
    this.state.monitorRefs.push({ workerId: canonical, monitorRef });
    return { status: "ok", monitor_ref: monitorRef, worker_id: canonical };
  }

  protected onDemonitor_worker(payload: Record<string, unknown>): Record<string, unknown> {
    const canonical = siblingId(String(payload.worker_id ?? ""), this.state.actorId);
    const entry = this.state.monitorRefs.find((m) => m.workerId === canonical);
    if (entry) {
      host.demonitor?.(entry.monitorRef);
      this.state.monitorRefs = this.state.monitorRefs.filter((m) => m.workerId !== canonical);
      return { status: "ok", worker_id: canonical };
    }
    return { status: "not_found", worker_id: canonical };
  }

  protected onValidate(payload: Record<string, unknown>): Record<string, unknown> {
    this.state.totalValidations++;
    const result = String(payload.result ?? "");
    const workerId = String(payload.worker_id ?? "");

    const byzantine = isByzantineResponse(result);
    if (byzantine) {
      this.state.byzantineCount++;
      this.state.failCount++;
    } else {
      this.state.passCount++;
    }

    const flpRatio =
      this.state.totalValidations > 0
        ? this.state.byzantineCount / this.state.totalValidations
        : 0;
    const flpExceeded = flpRatio >= FLP_THRESHOLD;

    return {
      status: "ok",
      valid: !byzantine,
      worker_id: workerId,
      byzantine_suspected: byzantine,
      flp_threshold_exceeded: flpExceeded,
      flp_ratio: Math.round(flpRatio * 1000) / 1000,
    };
  }

  protected onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    const flpRatio =
      this.state.totalValidations > 0
        ? this.state.byzantineCount / this.state.totalValidations
        : 0;
    return {
      status: "ok",
      total_validations: this.state.totalValidations,
      pass_count: this.state.passCount,
      fail_count: this.state.failCount,
      byzantine_count: this.state.byzantineCount,
      flp_threshold: FLP_THRESHOLD,
      flp_ratio: Math.round(flpRatio * 1000) / 1000,
      monitor_count: this.state.monitorRefs.length,
      down_events_received: this.state.downEvents.length,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// PipelineSupervisor
// ─────────────────────────────────────────────────────────────────────────────

class PipelineSupervisorActor extends PlexSpacesActor<SupervisorState> {
  getDefaultState(): SupervisorState {
    return {
      actorId: "",
      workerPool: [],
      monitorRefs: [],
      downEventsReceived: 0,
      totalDispatched: 0,
      nextWorkerIdx: 0,
    };
  }

  protected override onInit(_config: Record<string, unknown>): void {
    this.state.actorId = host.selfId?.() ?? "";
  }

  protected "on__DOWN__"(payload: Record<string, unknown>): Record<string, unknown> {
    const monitorRef = String(payload.monitor_ref ?? "");
    const downFrom = String(payload.down_from ?? "");
    const downReason = String(payload.down_reason ?? "");
    this.state.downEventsReceived++;
    this.state.workerPool = this.state.workerPool.filter((w) => w !== downFrom);
    this.state.monitorRefs = this.state.monitorRefs.filter((m) => m.monitorRef !== monitorRef);
    host.log?.("info", `[pipeline_supervisor] __DOWN__ down_from=${downFrom} down_reason=${downReason}`);
    return {};
  }

  protected onMonitor_worker(payload: Record<string, unknown>): Record<string, unknown> {
    const canonical = siblingId(String(payload.worker_id ?? ""), this.state.actorId);
    if (!canonical) {
      return { error: "worker_id required" };
    }
    const monitorRef = host.monitor?.(canonical) ?? `ref-${Date.now()}`;
    this.state.monitorRefs.push({ workerId: canonical, monitorRef });
    if (!this.state.workerPool.includes(canonical)) {
      this.state.workerPool.push(canonical);
    }
    return { status: "ok", monitor_ref: monitorRef, worker_id: canonical };
  }

  protected onDemonitor_worker(payload: Record<string, unknown>): Record<string, unknown> {
    const canonical = siblingId(String(payload.worker_id ?? ""), this.state.actorId);
    const entry = this.state.monitorRefs.find((m) => m.workerId === canonical);
    if (entry) {
      host.demonitor?.(entry.monitorRef);
      this.state.monitorRefs = this.state.monitorRefs.filter((m) => m.workerId !== canonical);
    }
    this.state.workerPool = this.state.workerPool.filter((w) => w !== canonical);
    return { status: "ok", worker_id: canonical };
  }

  protected onDispatch(payload: Record<string, unknown>): Record<string, unknown> {
    if (this.state.workerPool.length === 0) {
      return { status: "error", reason: "no_workers_available" };
    }
    const idx = this.state.nextWorkerIdx % this.state.workerPool.length;
    this.state.nextWorkerIdx++;
    const workerId = this.state.workerPool[idx];
    this.state.totalDispatched++;

    const prompt = String(payload.prompt ?? "");
    const requestId = String(payload.request_id ?? "");
    const result = host.ask?.(workerId, "infer", { prompt, request_id: requestId }, 30_000)
      ?? { error: "ask failed" };

    const resultRecord = result as Record<string, unknown>;
    const byzantineDetected =
      typeof resultRecord === "object" && resultRecord !== null && resultRecord.mode === "byzantine";

    return {
      status: "ok",
      worker_used: workerId,
      request_id: requestId,
      result,
      byzantine_detected: byzantineDetected,
    };
  }

  protected onStatus(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      status: "ok",
      worker_pool: this.state.workerPool,
      monitor_count: this.state.monitorRefs.length,
      down_events_received: this.state.downEventsReceived,
      total_dispatched: this.state.totalDispatched,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// AuditLogActor
// ─────────────────────────────────────────────────────────────────────────────

class AuditLogActor extends PlexSpacesActor<AuditLogState> {
  getDefaultState(): AuditLogState {
    return { eventsReceived: 0, lastEventType: "", lastActorId: "" };
  }

  protected onLog_event(payload: Record<string, unknown>): void {
    this.state.eventsReceived++;
    this.state.lastEventType = String(payload.event_type ?? "");
    this.state.lastActorId = String(payload.actor_id ?? "");
    const details = String(payload.details ?? "");
    host.log?.(
      "info",
      `[audit_log] #${this.state.eventsReceived} ${this.state.lastEventType} actor=${this.state.lastActorId} ${details}`
    );
  }

  protected onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      status: "ok",
      events_received: this.state.eventsReceived,
      last_event_type: this.state.lastEventType,
      last_actor_id: this.state.lastActorId,
    };
  }
}

// ─────────────────────────────────────────────────────────────────────────────
// Router — dispatch to the correct actor based on role
// ─────────────────────────────────────────────────────────────────────────────

const router = new ActorRouter({
  inference_worker: () => new InferenceWorkerActor(),
  validator_agent: () => new ValidatorAgentActor(),
  pipeline_supervisor: () => new PipelineSupervisorActor(),
  audit_log: () => new AuditLogActor(),
});

export const actor = {
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
