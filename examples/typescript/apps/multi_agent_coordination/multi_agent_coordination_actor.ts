// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Multi-Agent Coordination — TypeScript WASM Actor
//
// Demonstrates ten coordination patterns for multi-agent AI systems using
// PlexSpaces primitives. Inspired by the METR/HuggingFace incident (Aug 2026)
// where ~1,200 agents self-organized via shared state and invented protocols
// (HOLD, VETO, STOP, owner) that map directly to Linda tuplespace operations.
//
// Patterns demonstrated:
//   1. Blackboard / Shared State     (TupleSpace: out/rd/in)
//   2. Scatter-Gather / Fan-out      (Shard Groups)
//   3. Generator-Verifier            (ask loop)
//   4. Pipeline / Sequential          (chained ask)
//   5. Pub-Sub / Event Bus           (Process Groups)
//   6. Consensus / Voting            (TupleSpace vote tuples)
//   7. Dynamic Task Delegation       (TupleSpace write/take)
//   8. Veto Protocol                 (TupleSpace veto tuples)
//   9. Two-Phase Commit / Barrier    (TupleSpace ready/signal tuples)
//  10. Capability Discovery / Registry (TupleSpace service tuples)
//
// Actors:
//   CoordinatorWorkflow (Workflow), ResearchAgent, AnalysisAgent,
//   VerifierAgent, SynthesizerAgent, BenchmarkAgent (GenServer),
//   AuditEventAgent (GenEvent), CoordinationFSM (GenFSM)

import { ActorID, ActorRouter, PlexSpacesActor, WorkflowActor, host } from "@plexspaces/sdk";

// ============================================================
// Helpers
// ============================================================

function applicationIdFromActorId(actorId: string): string {
  try {
    return ActorID.parse(actorId).namespace;
  } catch {
    return "";
  }
}

function tsRegisterService(serviceType: string, actorId: string): void {
  const existing = host.ts.read(["svc", serviceType, null]);
  if (!existing) {
    host.ts.write(["svc", serviceType, actorId]);
  }
}

function tsDiscoverService(serviceType: string): string | null {
  const tup = host.ts.read(["svc", serviceType, null]);
  if (tup && tup.length >= 3) {
    return String(tup[2]);
  }
  return null;
}

function siblingActorTarget(role: string): string {
  const discovered = tsDiscoverService(role);
  if (discovered) return discovered;
  const selfId = host.selfId();
  try {
    return ActorID.parse(selfId).withName(role).toString();
  } catch {
    return role;
  }
}

function generateId(): string {
  const ts = Date.now().toString(36);
  const rnd = Math.random().toString(36).slice(2, 8);
  return `${ts}-${rnd}`;
}

function fireAuditEvent(eventType: string, source: string, data: Record<string, unknown>): void {
  try {
    host.processGroups.broadcast(
      "coordination-events",
      "coordination_event",
      { type: eventType, source, data, timestamp: host.nowMs() }
    );
  } catch {
    // Swallow — audit is best-effort
  }
}

// ============================================================
// ResearchAgent (GenServer)
// Pattern: Blackboard (write findings), Task Delegation (claim), Generator-Verifier (generate)
// ============================================================

interface ResearchState {
  [key: string]: unknown;
  actorId: string;
  findingsGenerated: number;
  tasksClaimed: number;
}

class ResearchAgent extends PlexSpacesActor<ResearchState> {
  getDefaultState(): ResearchState {
    return { actorId: "", findingsGenerated: 0, tasksClaimed: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("research", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  onResearch(payload: Record<string, unknown>): Record<string, unknown> {
    const topic = String(payload.topic || "general security");
    const feedback = payload.feedback ? String(payload.feedback) : null;
    const depth = Number(payload.depth || 1);

    const words = topic.split(/\s+/).length;
    let confidence = Math.min(0.3 + words * 0.08 + depth * 0.1, 0.95);
    if (feedback) {
      confidence = Math.min(confidence + 0.2, 0.98);
    }

    const findingId = `finding-${generateId()}`;
    const content = feedback
      ? `Refined analysis of ${topic}: ${feedback}. Updated assessment with higher confidence.`
      : `Security analysis of ${topic}: identified ${words} key areas requiring review. ` +
        `Potential vulnerabilities detected in input validation and access control.`;

    const ts = host.nowMs();
    host.ts.write(["finding", findingId, topic, content, confidence, ts]);

    this.state.findingsGenerated++;

    fireAuditEvent("finding_written", this.state.actorId, { finding_id: findingId, topic });

    return { finding_id: findingId, content, confidence };
  }

  onPrepare_tasks(payload: Record<string, unknown>): Record<string, unknown> {
    const count = Number(payload.count || 5);
    const prefix = String(payload.prefix || "test");
    const runId = generateId();
    const batchKey = `${prefix}-${runId}`;
    const taskIds: string[] = [];
    for (let i = 0; i < count; i++) {
      const taskId = `${batchKey}-${i}`;
      host.ts.write(["dtask", batchKey, taskId, "pending", `Task ${i}: investigate area ${i}`, i + 1]);
      taskIds.push(taskId);
    }
    return { tasks_written: count, task_ids: taskIds, batch_key: batchKey };
  }

  onClaim_task(payload: Record<string, unknown>): Record<string, unknown> {
    const batchKey = payload.batch_key ? String(payload.batch_key) : null;
    const claimed = batchKey
      ? host.ts.take(["dtask", batchKey, null, "pending", null, null])
      : host.ts.take(["dtask", null, null, "pending", null, null]);
    if (claimed && claimed.length >= 5) {
      this.state.tasksClaimed++;
      const taskId = String(claimed[2]);
      const description = String(claimed[4]);
      fireAuditEvent("task_claimed", this.state.actorId, { task_id: taskId });
      return { task_id: taskId, description, claimed: true };
    }
    return { task: null, claimed: false };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      findings_generated: this.state.findingsGenerated,
      tasks_claimed: this.state.tasksClaimed,
    };
  }
}

// ============================================================
// AnalysisAgent (GenServer)
// Pattern: Blackboard (read findings), Pipeline (stage 2)
// ============================================================

interface AnalysisState {
  [key: string]: unknown;
  actorId: string;
  analysesPerformed: number;
}

class AnalysisAgent extends PlexSpacesActor<AnalysisState> {
  getDefaultState(): AnalysisState {
    return { actorId: "", analysesPerformed: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("analysis", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  onAnalyze(payload: Record<string, unknown>): Record<string, unknown> {
    const findings = host.ts.readAll(["finding", null, null, null, null, null]);
    if (findings.length === 0) {
      return { analysis_id: null, summary: "No findings available", severity: "none", finding_count: 0 };
    }

    const findingIds: string[] = [];
    const topics: string[] = [];
    let maxConfidence = 0;

    for (const f of findings) {
      if (f.length >= 5) {
        findingIds.push(String(f[1]));
        topics.push(String(f[2]));
        const conf = Number(f[4]) || 0;
        if (conf > maxConfidence) maxConfidence = conf;
      }
    }

    const uniqueTopics = [...new Set(topics)];
    let severity: string;
    if (maxConfidence > 0.8) severity = "critical";
    else if (maxConfidence > 0.6) severity = "high";
    else if (maxConfidence > 0.4) severity = "medium";
    else severity = "low";

    const analysisId = `analysis-${generateId()}`;
    const summary = `Cross-referenced ${findings.length} findings across ${uniqueTopics.length} topics. ` +
      `Areas: ${uniqueTopics.join(", ")}. Overall severity: ${severity}.`;

    host.ts.write(["analysis", analysisId, JSON.stringify(findingIds), summary, severity]);

    this.state.analysesPerformed++;

    fireAuditEvent("analysis_completed", this.state.actorId, {
      analysis_id: analysisId, finding_count: findings.length, severity,
    });

    return { analysis_id: analysisId, summary, severity, finding_count: findings.length };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return { analyses_performed: this.state.analysesPerformed };
  }
}

// ============================================================
// VerifierAgent (GenServer)
// Pattern: Generator-Verifier (validate), Voting, Veto Protocol
// ============================================================

interface VerifierState {
  [key: string]: unknown;
  actorId: string;
  verifications: number;
  vetoesIssued: number;
  votesCast: number;
}

class VerifierAgent extends PlexSpacesActor<VerifierState> {
  getDefaultState(): VerifierState {
    return { actorId: "", verifications: 0, vetoesIssued: 0, votesCast: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("verifier", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  onVerify(payload: Record<string, unknown>): Record<string, unknown> {
    const analysisId = String(payload.analysis_id || "unknown");
    const summary = String(payload.summary || "");
    const severity = String(payload.severity || "medium");
    const confidence = Number(payload.confidence ?? 0.5);

    this.state.verifications++;

    if (confidence < 0.3) {
      host.ts.write(["veto", analysisId, "Insufficient evidence: confidence below threshold", host.nowMs()]);
      this.state.vetoesIssued++;
      fireAuditEvent("veto_issued", this.state.actorId, { analysis_id: analysisId, confidence });
      return {
        approved: false,
        veto_issued: true,
        feedback: `Confidence ${confidence.toFixed(2)} is below 0.30 threshold. Provide stronger evidence.`,
      };
    }

    return { approved: true, feedback: "Verified: evidence meets threshold", veto_issued: false };
  }

  onVote(payload: Record<string, unknown>): Record<string, unknown> {
    const proposalId = String(payload.proposal_id || "unknown");
    const voterId = String(payload.voter_id || "anonymous");
    const analysis = (payload.analysis || {}) as Record<string, unknown>;
    const severity = String(analysis.severity || "medium");

    let decision: string;
    if (severity === "critical" || severity === "high") {
      decision = "approve";
    } else if (severity === "medium") {
      const voterNum = parseInt(voterId.replace(/\D/g, ""), 10);
      decision = (voterNum % 2 !== 0) ? "approve" : "reject";
    } else {
      decision = "reject";
    }

    host.ts.write(["vote", proposalId, voterId, decision, host.nowMs()]);
    this.state.votesCast++;

    fireAuditEvent("vote_cast", this.state.actorId, { proposal_id: proposalId, voter_id: voterId, decision });

    return { voter_id: voterId, decision };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      verifications: this.state.verifications,
      vetoes_issued: this.state.vetoesIssued,
      votes_cast: this.state.votesCast,
    };
  }
}

// ============================================================
// SynthesizerAgent (GenServer)
// Pattern: Veto Protocol (check vetoes), Pipeline (final stage)
// ============================================================

interface SynthesizerState {
  [key: string]: unknown;
  actorId: string;
  synthesesPerformed: number;
}

class SynthesizerAgent extends PlexSpacesActor<SynthesizerState> {
  getDefaultState(): SynthesizerState {
    return { actorId: "", synthesesPerformed: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("synthesizer", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  onSynthesize(payload: Record<string, unknown>): Record<string, unknown> {
    const analyses = host.ts.readAll(["analysis", null, null, null, null]);
    let includedCount = 0;
    let vetoedCount = 0;
    const reportParts: string[] = [];

    for (const a of analyses) {
      if (a.length < 5) continue;
      const aId = String(a[1]);
      const summary = String(a[3]);
      const severity = String(a[4]);

      const veto = host.ts.read(["veto", aId, null, null]);
      if (veto) {
        vetoedCount++;
        continue;
      }

      includedCount++;
      reportParts.push(`[${severity.toUpperCase()}] ${summary}`);
    }

    const allVetoes = host.ts.readAll(["veto", null, null, null]);
    if (allVetoes.length > vetoedCount) {
      vetoedCount = allVetoes.length;
    }

    const report = reportParts.length > 0
      ? `Security Audit Report\n${"=".repeat(40)}\n${reportParts.join("\n\n")}\n\nTotal findings included: ${includedCount}, vetoed: ${vetoedCount}`
      : `No unvetoed analyses available. ${vetoedCount} analyses were vetoed.`;

    this.state.synthesesPerformed++;

    fireAuditEvent("synthesis_completed", this.state.actorId, { included_count: includedCount, vetoed_count: vetoedCount });

    return {
      report,
      included_count: includedCount,
      vetoed_count: vetoedCount,
      timestamp: host.nowMs(),
    };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return { syntheses_performed: this.state.synthesesPerformed };
  }
}

// ============================================================
// BenchmarkAgent (GenServer)
// Pattern: All 10 patterns micro-benchmarked
// ============================================================

interface BenchmarkState {
  [key: string]: unknown;
  actorId: string;
  lastResults: Record<string, unknown>[];
}

class BenchmarkAgent extends PlexSpacesActor<BenchmarkState> {
  getDefaultState(): BenchmarkState {
    return { actorId: "", lastResults: [] };
  }

  protected onInit(config: Record<string, unknown>): void {
    this.state.actorId = host.selfId();
  }

  onRun_pattern_benchmark(payload: Record<string, unknown>): Record<string, unknown> {
    const pattern = String(payload.pattern || "blackboard");
    const iterations = Number(payload.iterations || 10);
    const result = this.benchmarkPattern(pattern, iterations);
    return result;
  }

  onRun_all_benchmarks(payload: Record<string, unknown>): Record<string, unknown> {
    const iterations = Number(payload.iterations || 10);
    const patterns = [
      "blackboard", "scatter_gather", "generator_verifier", "pipeline",
      "pubsub", "voting", "task_delegation", "veto", "barrier", "capability_discovery",
    ];
    const results: Record<string, unknown>[] = [];
    for (const p of patterns) {
      results.push(this.benchmarkPattern(p, iterations));
    }
    this.state.lastResults = results;
    return { results, pattern_count: patterns.length };
  }

  onGet_results(_payload: Record<string, unknown>): Record<string, unknown> {
    return { results: this.state.lastResults };
  }

  private benchmarkPattern(pattern: string, iterations: number): Record<string, unknown> {
    const timings: number[] = [];

    for (let i = 0; i < iterations; i++) {
      const start = host.nowMs();
      this.runPatternIteration(pattern, i);
      const elapsed = host.nowMs() - start;
      timings.push(elapsed);
    }

    timings.sort((a, b) => a - b);
    const sum = timings.reduce((s, t) => s + t, 0);
    const avg = sum / timings.length;
    const min = timings[0] || 0;
    const max = timings[timings.length - 1] || 0;
    const p50 = timings[Math.floor(timings.length * 0.5)] || 0;
    const p95 = timings[Math.floor(timings.length * 0.95)] || 0;
    const tps = avg > 0 ? Math.round(1000 / avg) : 0;

    return { pattern, iterations, avg_ms: Math.round(avg * 100) / 100, min_ms: min, max_ms: max, p50_ms: p50, p95_ms: p95, tps };
  }

  private runPatternIteration(pattern: string, i: number): void {
    const tag = `bench-${pattern}-${i}`;
    switch (pattern) {
      case "blackboard": {
        host.ts.write(["bench", tag, "data", host.nowMs()]);
        host.ts.read(["bench", tag, null, null]);
        host.ts.take(["bench", tag, null, null]);
        break;
      }
      case "scatter_gather": {
        for (let s = 0; s < 3; s++) {
          host.ts.write(["sg", tag, `shard-${s}`, `result-${s}`, host.nowMs()]);
        }
        host.ts.readAll(["sg", tag, null, null, null]);
        for (let s = 0; s < 3; s++) {
          host.ts.take(["sg", tag, `shard-${s}`, null, null]);
        }
        break;
      }
      case "generator_verifier": {
        host.ts.write(["gv", tag, "draft", 0.5, host.nowMs()]);
        const draft = host.ts.read(["gv", tag, null, null, null]);
        if (draft) {
          host.ts.write(["gv", tag + "-v", "verified", 0.9, host.nowMs()]);
        }
        host.ts.take(["gv", tag, null, null, null]);
        host.ts.take(["gv", tag + "-v", null, null, null]);
        break;
      }
      case "pipeline": {
        host.ts.write(["pipe", tag, "stage1", "researched", host.nowMs()]);
        host.ts.write(["pipe", tag, "stage2", "analyzed", host.nowMs()]);
        host.ts.write(["pipe", tag, "stage3", "verified", host.nowMs()]);
        host.ts.readAll(["pipe", tag, null, null, null]);
        for (let s = 1; s <= 3; s++) {
          host.ts.take(["pipe", tag, `stage${s}`, null, null]);
        }
        break;
      }
      case "pubsub": {
        host.ts.write(["event", tag, "published", "test-data", host.nowMs()]);
        host.ts.readAll(["event", tag, null, null, null]);
        host.ts.take(["event", tag, null, null, null]);
        break;
      }
      case "voting": {
        for (let v = 0; v < 3; v++) {
          host.ts.write(["vote-bench", tag, `voter-${v}`, v % 2 === 0 ? "approve" : "reject", host.nowMs()]);
        }
        host.ts.readAll(["vote-bench", tag, null, null, null]);
        for (let v = 0; v < 3; v++) {
          host.ts.take(["vote-bench", tag, `voter-${v}`, null, null]);
        }
        break;
      }
      case "task_delegation": {
        host.ts.write(["td", tag, "pending", "benchmark task", host.nowMs()]);
        host.ts.take(["td", tag, "pending", null, null]);
        break;
      }
      case "veto": {
        host.ts.write(["veto-bench", tag, "test-reason", host.nowMs()]);
        host.ts.read(["veto-bench", tag, null, null]);
        host.ts.take(["veto-bench", tag, null, null]);
        break;
      }
      case "barrier": {
        for (let a = 0; a < 3; a++) {
          host.ts.write(["ready-bench", tag, `agent-${a}`, host.nowMs()]);
        }
        const ready = host.ts.readAll(["ready-bench", tag, null, null]);
        if (ready.length >= 3) {
          host.ts.write(["signal-bench", tag, "COMMIT", host.nowMs()]);
        }
        host.ts.take(["signal-bench", tag, null, null]);
        for (let a = 0; a < 3; a++) {
          host.ts.take(["ready-bench", tag, `agent-${a}`, null]);
        }
        break;
      }
      case "capability_discovery": {
        host.ts.write(["cap", tag, "capability-data", host.nowMs()]);
        host.ts.readAll(["cap", tag, null, null]);
        host.ts.take(["cap", tag, null, null]);
        break;
      }
    }
  }
}

// ============================================================
// AuditEventAgent (GenEvent)
// Pattern: Pub-Sub (logs all coordination events)
// ============================================================

interface AuditState {
  [key: string]: unknown;
  actorId: string;
  logCount: number;
}

class AuditEventAgent extends PlexSpacesActor<AuditState> {
  getDefaultState(): AuditState {
    return { actorId: "", logCount: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("audit", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  onCoordination_event(payload: Record<string, unknown>): Record<string, unknown> {
    this.state.logCount++;
    const entry = {
      seq: this.state.logCount,
      type: payload.type || "unknown",
      source: payload.source || "unknown",
      data: payload.data || {},
      timestamp: payload.timestamp || host.nowMs(),
    };
    try {
      host.kv.put(`audit:${this.state.logCount}`, JSON.stringify(entry));
      host.kv.put("audit:count", String(this.state.logCount));
    } catch { /* KV may not be available in all environments */ }

    return { logged: true, seq: this.state.logCount };
  }

  onGet_audit_log(payload: Record<string, unknown>): Record<string, unknown> {
    const limit = Number(payload.limit || 20);
    const entries: Record<string, unknown>[] = [];

    const countStr = host.kv.get("audit:count");
    const totalCount = countStr ? parseInt(countStr, 10) || 0 : this.state.logCount;

    const start = Math.max(1, totalCount - limit + 1);
    for (let i = start; i <= totalCount; i++) {
      const raw = host.kv.get(`audit:${i}`);
      if (raw) {
        try {
          entries.push(JSON.parse(raw) as Record<string, unknown>);
        } catch { /* skip malformed */ }
      }
    }

    return { entries, total_count: totalCount };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return { log_count: this.state.logCount };
  }
}

// ============================================================
// CoordinationFSM (GenFSM)
// Pattern: State machine tracking coordination lifecycle
// ============================================================

interface FSMState {
  [key: string]: unknown;
  actorId: string;
  currentState: string;
  transitionsCount: number;
}

const VALID_TRANSITIONS: Record<string, string[]> = {
  idle: ["decomposing", "failed"],
  decomposing: ["researching", "failed"],
  researching: ["analyzing", "failed"],
  analyzing: ["verifying", "failed"],
  verifying: ["voting", "failed"],
  voting: ["synthesizing", "failed"],
  synthesizing: ["complete", "failed"],
  complete: ["idle", "failed"],
  failed: ["idle"],
};

class CoordinationFSM extends PlexSpacesActor<FSMState> {
  getDefaultState(): FSMState {
    return { actorId: "", currentState: "idle", transitionsCount: 0 };
  }

  protected onInit(config: Record<string, unknown>): void {
    this.state.actorId = host.selfId();
  }

  onTransition(payload: Record<string, unknown>): Record<string, unknown> {
    const targetState = String(payload.target_state || "");
    const current = this.state.currentState;
    const allowed = VALID_TRANSITIONS[current] || ["failed"];

    if (allowed.includes(targetState)) {
      const previous = current;
      this.state.currentState = targetState;
      this.state.transitionsCount++;
      return { previous, current: targetState, valid: true };
    }

    return { previous: current, current, valid: false, error: `Invalid transition: ${current} -> ${targetState}` };
  }

  onGet_state(_payload: Record<string, unknown>): Record<string, unknown> {
    return { current_state: this.state.currentState, transitions_count: this.state.transitionsCount };
  }

  onReset(_payload: Record<string, unknown>): Record<string, unknown> {
    const previous = this.state.currentState;
    this.state.currentState = "idle";
    this.state.transitionsCount = 0;
    return { previous, current: "idle", reset: true };
  }
}

// ============================================================
// CoordinatorWorkflow (Workflow)
// Pattern: Pipeline, Scatter-Gather, Task Delegation, 2PC, Capability Discovery
// ============================================================

interface CoordinatorState {
  [key: string]: unknown;
  actorId: string;
  status: string;
  subtasks: number;
  findings: number;
  analyses: number;
  vetoes: number;
  votes: number;
  iterations: number;
  report: string;
}

class CoordinatorWorkflow extends WorkflowActor<CoordinatorState> {
  getDefaultState(): CoordinatorState {
    return {
      actorId: "", status: "idle", subtasks: 0, findings: 0,
      analyses: 0, vetoes: 0, votes: 0, iterations: 0, report: "",
    };
  }

  protected onInit(config: Record<string, unknown>): void {
    const selfId = host.selfId();
    this.state.actorId = selfId;
    tsRegisterService("coordinator", selfId);
    try { host.processGroups.join("coordination-events"); } catch { /* ok */ }
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const task = String(payload.task || "security audit");
    this.state.status = "running";

    const fsmTarget = siblingActorTarget("coordination_fsm");
    const researchTarget = siblingActorTarget("research");
    const analysisTarget = siblingActorTarget("analysis");
    const verifierTarget = siblingActorTarget("verifier");
    const synthesizerTarget = siblingActorTarget("synthesizer");

    // Step 1: Register self (Capability Discovery)
    tsRegisterService("coordinator", this.state.actorId);

    // Step 2: FSM → decomposing
    try { host.ask(fsmTarget, "transition", { target_state: "decomposing" }); } catch { /* ok */ }

    // Step 3: Decompose into subtasks
    const domains = ["SQL injection and input validation", "Authentication bypass and JWT handling", "Cross-site scripting in templates"];
    this.state.subtasks = domains.length;

    // Step 4: Write tasks as tuples (Dynamic Task Delegation)
    for (let i = 0; i < domains.length; i++) {
      host.ts.write(["task", `wf-task-${i}`, "pending", domains[i], i + 1]);
    }
    fireAuditEvent("tasks_posted", this.state.actorId, { count: domains.length });

    // Step 5: FSM → researching
    try { host.ask(fsmTarget, "transition", { target_state: "researching" }); } catch { /* ok */ }

    // Step 6: Research each domain (Scatter-Gather fallback to sequential)
    const researchResults: Record<string, unknown>[] = [];
    for (const domain of domains) {
      try {
        const result = host.ask(researchTarget, "research", { topic: domain, depth: 2 }) as Record<string, unknown>;
        researchResults.push(result);
        this.state.findings++;
      } catch {
        researchResults.push({ error: `research failed for ${domain}` });
      }
    }

    // Step 7: FSM → analyzing
    try { host.ask(fsmTarget, "transition", { target_state: "analyzing" }); } catch { /* ok */ }

    // Step 8: Analyze findings
    let analysisResult: Record<string, unknown> = {};
    try {
      analysisResult = host.ask(analysisTarget, "analyze", {}) as Record<string, unknown>;
      this.state.analyses++;
    } catch { /* ok */ }

    // Step 9: FSM → verifying
    try { host.ask(fsmTarget, "transition", { target_state: "verifying" }); } catch { /* ok */ }

    // Step 10: Generator-Verifier loop
    let verified = false;
    const maxIterations = 3;
    for (let i = 0; i < maxIterations; i++) {
      this.state.iterations++;
      try {
        const verifyResult = host.ask(verifierTarget, "verify", {
          analysis_id: analysisResult.analysis_id || "unknown",
          summary: analysisResult.summary || "",
          severity: analysisResult.severity || "medium",
          confidence: 0.7,
        }) as Record<string, unknown>;
        if (verifyResult.approved) {
          verified = true;
          break;
        }
        // Refine with feedback
        if (researchResults.length > 0) {
          try {
            const refined = host.ask(researchTarget, "research", {
              topic: domains[0],
              feedback: verifyResult.feedback,
              depth: 3,
            }) as Record<string, unknown>;
            this.state.findings++;
            // Re-analyze
            analysisResult = host.ask(analysisTarget, "analyze", {}) as Record<string, unknown>;
            this.state.analyses++;
          } catch { break; }
        }
      } catch { break; }
    }

    // Step 11: FSM → voting
    try { host.ask(fsmTarget, "transition", { target_state: "voting" }); } catch { /* ok */ }

    // Step 12: Consensus voting
    const proposalId = `proposal-${generateId()}`;
    const voterIds = ["v1", "v2", "v3"];
    let approvals = 0;
    for (const vid of voterIds) {
      try {
        const voteResult = host.ask(verifierTarget, "vote", {
          proposal_id: proposalId,
          voter_id: vid,
          analysis: { severity: analysisResult.severity || "high" },
        }) as Record<string, unknown>;
        this.state.votes++;
        if (voteResult.decision === "approve") approvals++;
      } catch { /* ok */ }
    }
    const consensusReached = approvals > voterIds.length / 2;

    // Step 13: FSM → synthesizing
    try { host.ask(fsmTarget, "transition", { target_state: "synthesizing" }); } catch { /* ok */ }

    // Step 14: Synthesize final report
    let synthesisResult: Record<string, unknown> = {};
    try {
      synthesisResult = host.ask(synthesizerTarget, "synthesize", {}) as Record<string, unknown>;
      this.state.vetoes = Number(synthesisResult.vetoed_count || 0);
    } catch { /* ok */ }

    this.state.report = String(synthesisResult.report || "synthesis unavailable");

    // Step 15: FSM → complete
    try { host.ask(fsmTarget, "transition", { target_state: "complete" }); } catch { /* ok */ }

    this.state.status = "completed";

    fireAuditEvent("workflow_completed", this.state.actorId, {
      subtasks: this.state.subtasks,
      findings: this.state.findings,
      consensus: consensusReached,
    });

    return {
      status: "completed",
      report: this.state.report,
      consensus_reached: consensusReached,
      metrics: {
        subtasks: this.state.subtasks,
        findings: this.state.findings,
        analyses: this.state.analyses,
        vetoes: this.state.vetoes,
        votes: this.state.votes,
        iterations: this.state.iterations,
        approvals,
      },
    };
  }

  signal(name: string, data: Record<string, unknown>): void {
    if (name === "cancel") {
      this.state.status = "cancelled";
      const fsmTarget = siblingActorTarget("coordination_fsm");
      try { host.ask(fsmTarget, "transition", { target_state: "failed" }); } catch { /* ok */ }
      fireAuditEvent("workflow_cancelled", this.state.actorId, {});
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "progress") {
      return {
        status: this.state.status,
        subtasks: this.state.subtasks,
        findings: this.state.findings,
        analyses: this.state.analyses,
        vetoes: this.state.vetoes,
        votes: this.state.votes,
        iterations: this.state.iterations,
      };
    }
    return { status: this.state.status };
  }
}

// ============================================================
// Actor routing
// ============================================================

const router = new ActorRouter({
  coordinator: () => new CoordinatorWorkflow(),
  research: () => new ResearchAgent(),
  analysis: () => new AnalysisAgent(),
  verifier: () => new VerifierAgent(),
  synthesizer: () => new SynthesizerAgent(),
  benchmark: () => new BenchmarkAgent(),
  audit: () => new AuditEventAgent(),
  coordination_fsm: () => new CoordinationFSM(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.init(configJson),
  handle: (
    from: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView
  ) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.setState(stateJson),
};
