// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// LLM Workflow Orchestrator - TypeScript WASM Actor
//
// Demonstrates five agentic LLM patterns using PlexSpaces actors:
//   1. Routing        — RouterActor classifies inputs and dispatches to specialist pipelines
//   2. Prompt Chaining — ChainActor executes multi-step sequential transforms
//   3. LLM-as-Judge   — JudgeActor scores outputs against criteria heuristics
//   4. Reflection     — OrchestratorWorkflow iteratively refines until score threshold met
//   5. Evol-Instruct  — ChainActor.onEvolve_instruction mutates prompts for dataset augmentation
//
// Message types (GenServer): op="route" | "execute_chain" | "evaluate" | "evolve_instruction" | "get_stats"
// Message types (Workflow):  workflow_run | workflow_signal:feedback | workflow_signal:reset
//                            workflow_query:progress | workflow_query:history

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

// TupleSpace-based service registry: write-once registration so only the first
// (supervisor-spawned) instance claims the slot. Subsequent instances (e.g.
// virtual_actor activations during re-instantiation) find the entry and skip.
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
  // TupleSpace discovery: supervisor-spawned actors register on Init.
  const discovered = tsDiscoverService(role);
  if (discovered) return discovered;
  // Fallback: role-based routing via actor ID construction
  const selfId = host.selfId();
  try {
    return ActorID.parse(selfId).withTypeAndName(role, role).toString();
  } catch {
    return role;
  }
}

// ============================================================
// RouterActor
// ============================================================

interface RouterState {
  [key: string]: unknown;
  actorId: string;
  routingDecisions: number;
  lastRoute: string;
  routes: Record<string, number>;
}

class RouterActor extends PlexSpacesActor<RouterState> {
  getDefaultState(): RouterState {
    return {
      actorId: "",
      routingDecisions: 0,
      lastRoute: "",
      routes: {},
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = String(config.actor_id ?? "");
    tsRegisterService("router", this.state.actorId);
  }

  onRoute(payload: Record<string, unknown>): Record<string, unknown> {
    const content = String(payload.content ?? "");
    const lower = content.toLowerCase();

    let route: string;
    if (lower.includes("summarize") || content.length < 100) {
      route = "summarize";
    } else if (lower.includes("extract") || lower.includes("entities")) {
      route = "extract";
    } else if (lower.includes("analyze") || lower.includes("compare")) {
      route = "analyze";
    } else {
      route = "generate";
    }

    this.state.routingDecisions += 1;
    this.state.lastRoute = route;
    this.state.routes[route] = (this.state.routes[route] ?? 0) + 1;

    return {
      route,
      task_type: route,
      content,
      routing_id: host.nowMs(),
    };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      routing_decisions: this.state.routingDecisions,
      last_route: this.state.lastRoute,
      routes: { ...this.state.routes },
    };
  }
}

// ============================================================
// ChainActor
// ============================================================

interface ChainState {
  [key: string]: unknown;
  actorId: string;
  stepsCompleted: number;
  currentChain: string;
  chainResults: string[];
}

class ChainActor extends PlexSpacesActor<ChainState> {
  getDefaultState(): ChainState {
    return {
      actorId: "",
      stepsCompleted: 0,
      currentChain: "",
      chainResults: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = String(config.actor_id ?? "");
    tsRegisterService("chain", this.state.actorId);
  }

  onExecute_chain(payload: Record<string, unknown>): Record<string, unknown> {
    const content = String(payload.content ?? "");
    const steps = Array.isArray(payload.steps)
      ? (payload.steps as string[])
      : ["summarize", "extract_keywords", "format_output"];

    const t0 = host.nowMs();
    const stepResults: Record<string, unknown>[] = [];
    let currentContent = content;

    for (const step of steps) {
      const stepStart = host.nowMs();
      let transformed = currentContent;

      if (step === "summarize") {
        transformed =
          currentContent.length > 200
            ? currentContent.slice(0, 200) + "... [summarized]"
            : currentContent;
      } else if (step === "extract_keywords") {
        const words = currentContent
          .replace(/[^a-zA-Z\s]/g, "")
          .split(/\s+/)
          .filter((w) => w.length > 5);
        const unique = [...new Set(words)].slice(0, 5);
        transformed = unique.join(", ");
      } else if (step === "format_output") {
        transformed = JSON.stringify({
          step_count: stepResults.length + 1,
          content: currentContent,
          processed: true,
        });
      }

      stepResults.push({
        step,
        input_length: currentContent.length,
        output_length: transformed.length,
        latency_ms: host.nowMs() - stepStart,
      });
      currentContent = transformed;
    }

    const totalTime = host.nowMs() - t0;
    this.state.stepsCompleted += steps.length;
    this.state.currentChain = steps.join("→");
    this.state.chainResults.push(currentContent);

    return {
      chain_id: host.nowMs(),
      steps_completed: steps.length,
      results: stepResults,
      final_output: currentContent,
      latency_ms: totalTime,
    };
  }

  onEvolve_instruction(payload: Record<string, unknown>): Record<string, unknown> {
    const instruction = String(payload.instruction ?? "");
    const mutations = Number(payload.mutations ?? 2);

    const synonyms: Record<string, string> = {
      good: "excellent",
      bad: "poor",
      big: "substantial",
      small: "minimal",
      fast: "efficient",
      slow: "gradual",
      use: "utilize",
      make: "construct",
      get: "retrieve",
      show: "demonstrate",
    };

    let evolved = instruction;
    let count = 0;

    if (mutations >= 1) {
      evolved = "Please explain in detail: " + evolved;
      count += 1;
    }
    if (mutations >= 2) {
      evolved = evolved + " Provide examples.";
      count += 1;
    }
    if (mutations >= 3) {
      for (const [word, syn] of Object.entries(synonyms)) {
        const re = new RegExp(`\\b${word}\\b`, "gi");
        evolved = evolved.replace(re, syn);
      }
      count += 1;
    }

    return {
      original: instruction,
      evolved,
      mutations_applied: count,
    };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      steps_completed: this.state.stepsCompleted,
      current_chain: this.state.currentChain,
      chains_run: this.state.chainResults.length,
    };
  }
}

// ============================================================
// JudgeActor
// ============================================================

interface JudgeState {
  [key: string]: unknown;
  actorId: string;
  evaluationsRun: number;
  avgScore: number;
  scoreHistory: number[];
}

class JudgeActor extends PlexSpacesActor<JudgeState> {
  getDefaultState(): JudgeState {
    return {
      actorId: "",
      evaluationsRun: 0,
      avgScore: 0,
      scoreHistory: [],
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = String(config.actor_id ?? "");
    tsRegisterService("judge", this.state.actorId);
  }

  onEvaluate(payload: Record<string, unknown>): Record<string, unknown> {
    const content = String(payload.content ?? "");
    const originalQuery = String(payload.original_query ?? "");
    const criteria = Array.isArray(payload.criteria)
      ? (payload.criteria as string[])
      : ["relevance", "completeness", "clarity"];

    // Score relevance: shared word count between content and query (0–10)
    const contentWords = new Set(
      content
        .toLowerCase()
        .replace(/[^a-z\s]/g, "")
        .split(/\s+/)
        .filter(Boolean),
    );
    const queryWords = originalQuery
      .toLowerCase()
      .replace(/[^a-z\s]/g, "")
      .split(/\s+/)
      .filter(Boolean);
    const sharedCount = queryWords.filter((w) => contentWords.has(w)).length;
    const relevance = Math.min(10, queryWords.length > 0 ? (sharedCount / queryWords.length) * 10 : 5);

    // Score completeness: length-based
    let completeness: number;
    if (content.length > 200) {
      completeness = 9;
    } else if (content.length > 50) {
      completeness = 7;
    } else {
      completeness = 4;
    }

    // Score clarity: penalise repeated words
    const allWords = content.toLowerCase().replace(/[^a-z\s]/g, "").split(/\s+/).filter(Boolean);
    const uniqueRatio = allWords.length > 0 ? new Set(allWords).size / allWords.length : 1;
    const clarity = Math.round(uniqueRatio * 10);

    const criteriaScores: Record<string, number> = {};
    if (criteria.includes("relevance")) criteriaScores["relevance"] = Math.round(relevance * 10) / 10;
    if (criteria.includes("completeness")) criteriaScores["completeness"] = completeness;
    if (criteria.includes("clarity")) criteriaScores["clarity"] = clarity;

    const scoreValues = Object.values(criteriaScores);
    const compositeScore =
      scoreValues.length > 0
        ? Math.round((scoreValues.reduce((a, b) => a + b, 0) / scoreValues.length) * 10) / 10
        : 0;

    this.state.scoreHistory.push(compositeScore);
    this.state.evaluationsRun += 1;
    this.state.avgScore =
      Math.round(
        (this.state.scoreHistory.reduce((a, b) => a + b, 0) / this.state.scoreHistory.length) * 10,
      ) / 10;

    return {
      score: compositeScore,
      criteria_scores: criteriaScores,
      passed: compositeScore >= 6.0,
      feedback: `Score: ${compositeScore}/10`,
    };
  }

  onGet_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      evaluations_run: this.state.evaluationsRun,
      avg_score: this.state.avgScore,
      score_history: [...this.state.scoreHistory],
    };
  }
}

// ============================================================
// PipelineAuditActor
// ============================================================

interface AuditState {
  [key: string]: unknown;
  actorId: string;
  eventsReceived: number;
  lastEvent: Record<string, unknown>;
}

class PipelineAuditActor extends PlexSpacesActor<AuditState> {
  getDefaultState(): AuditState {
    return { actorId: "", eventsReceived: 0, lastEvent: {} };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = String(config.actor_id ?? "");
  }

  // Fire-and-forget: cast handler for pipeline step completion events
  onPipeline_step_completed(payload: Record<string, unknown>): void {
    this.state.eventsReceived++;
    this.state.lastEvent = payload;
    try {
      host.applicationMetricsAdd(this.state.actorId || "llm-orchestrator", {
        message_count: 1,
        counter_metrics: { pipeline_events: 1 },
      });
    } catch (_e) {
      // metrics optional
    }
  }

  onGet_audit_stats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      events_received: this.state.eventsReceived,
      last_event: { ...this.state.lastEvent },
    };
  }
}

// ============================================================
// QualityFSMActor
// ============================================================

interface QualityFSMState {
  [key: string]: unknown;
  actorId: string;
  fsmState: string; // 'pending' | 'evaluating' | 'approved' | 'rejected' | 'escalated'
  attempts: number;
  lastScore: number;
}

class QualityFSMActor extends PlexSpacesActor<QualityFSMState> {
  // FSM metadata — mirrors @fsm_actor(states=[...], initial="pending") in Python/Rust
  static readonly FSM_STATES = ["pending", "evaluating", "approved", "rejected", "escalated"] as const;
  static readonly FSM_INITIAL = "pending";

  getDefaultState(): QualityFSMState {
    return { actorId: "", fsmState: QualityFSMActor.FSM_INITIAL, attempts: 0, lastScore: 0 };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.actorId = String(config.actor_id ?? "");
  }

  onEvaluate(payload: Record<string, unknown>): Record<string, unknown> {
    const score = Number(payload.score ?? 0);
    this.state.attempts++;
    this.state.lastScore = score;
    if (score >= 8) {
      this.state.fsmState = "approved";
    } else if (score >= 6) {
      this.state.fsmState = this.state.attempts >= 3 ? "escalated" : "evaluating";
    } else {
      this.state.fsmState = this.state.attempts >= 3 ? "rejected" : "evaluating";
    }
    try {
      host.applicationMetricsAdd(this.state.actorId || "llm-orchestrator", {
        message_count: 1,
        counter_metrics: { quality_evaluations: 1 },
      });
    } catch (_e) {
      // metrics optional
    }
    return { state: this.state.fsmState, score, attempts: this.state.attempts };
  }

  onReset(_payload: Record<string, unknown>): Record<string, unknown> {
    this.state.fsmState = "pending";
    this.state.attempts = 0;
    this.state.lastScore = 0;
    return { state: this.state.fsmState };
  }

  onGet_state(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      state: this.state.fsmState,
      attempts: this.state.attempts,
      last_score: this.state.lastScore,
    };
  }
}

// ============================================================
// OrchestratorWorkflow
// ============================================================

interface OrchestratorState {
  [key: string]: unknown;
  status: string;
  taskId: string;
  currentStep: string;
  iterationCount: number;
  finalScore: number;
  result: string;
  signals: string[];
  // Sibling actor targets resolved during onInit
  routerTarget: string;
  chainTarget: string;
  judgeTarget: string;
}

class OrchestratorWorkflow extends WorkflowActor<OrchestratorState> {
  getDefaultState(): OrchestratorState {
    return {
      status: "",
      taskId: "",
      currentStep: "",
      iterationCount: 0,
      finalScore: 0,
      result: "",
      signals: [],
      routerTarget: "",
      chainTarget: "",
      judgeTarget: "",
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    // Resolve sibling actor targets once at init time
    this.state.routerTarget = siblingActorTarget("router");
    this.state.chainTarget = siblingActorTarget("chain");
    this.state.judgeTarget = siblingActorTarget("judge");
    // Preserve any persisted IDs if already set (durable reactivation)
    if (config.actor_id) {
      // actor_id available — targets already set above
    }
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const content = String(payload.content ?? "");
    const maxIterations = Number(payload.max_iterations ?? 3);
    const scoreThreshold = Number(payload.score_threshold ?? 6.0);

    this.state.taskId = String(host.nowMs());
    this.state.status = "running";
    this.state.iterationCount = 0;
    this.state.currentStep = "route";

    // Step 1: Route
    let routeDecision = "generate";
    try {
      const routeRes = host.ask(
        this.state.routerTarget,
        "route",
        { content },
        10000,
      ) as Record<string, unknown>;
      routeDecision = String(routeRes.route ?? "generate");
    } catch (_e) {
      // Router unavailable — continue with default route
    }

    // Step 2: Chain (initial pass)
    this.state.currentStep = "chain";
    let chainOutput = content;
    try {
      const chainRes = host.ask(
        this.state.chainTarget,
        "execute_chain",
        { content },
        15000,
      ) as Record<string, unknown>;
      chainOutput = String(chainRes.final_output ?? content);
    } catch (_e) {
      // Chain unavailable — use raw content
    }

    // Step 3: Reflection loop with judge scoring
    this.state.currentStep = "judge";
    let currentContent = chainOutput;
    let finalScore = 0;
    let finalResult = currentContent;

    for (let iter = 0; iter <= maxIterations; iter++) {
      let score = 0;
      try {
        const judgeRes = host.ask(
          this.state.judgeTarget,
          "evaluate",
          { content: currentContent, original_query: content },
          10000,
        ) as Record<string, unknown>;
        score = Number(judgeRes.score ?? 0);
      } catch (_e) {
        // Judge unavailable — assume passing score
        score = scoreThreshold;
      }

      finalScore = score;
      finalResult = currentContent;

      if (score >= scoreThreshold || iter >= maxIterations) {
        break;
      }

      // Refine: prepend iteration note for next round
      this.state.iterationCount += 1;
      currentContent = `Refined attempt ${this.state.iterationCount}: ${content}`;
      try {
        const refinedChain = host.ask(
          this.state.chainTarget,
          "execute_chain",
          { content: currentContent },
          15000,
        ) as Record<string, unknown>;
        currentContent = String(refinedChain.final_output ?? currentContent);
      } catch (_e) {
        // use currentContent as-is
      }
    }

    this.state.status = "completed";
    this.state.currentStep = "done";
    this.state.finalScore = finalScore;
    this.state.result = finalResult;

    // Store result in TupleSpace for cross-actor access
    try {
      host.ts.write(["orchestrator", "result", this.state.taskId, this.state.finalScore, host.nowMs()]);
    } catch (_e) {
      // TupleSpace write optional
    }

    // Report metrics
    try {
      host.applicationMetricsAdd("llm-orchestrator", {
        message_count: 1,
        counter_metrics: {
          orchestrator_runs_total: 1,
          [`route_${routeDecision}`]: 1,
        },
        latency_totals_ms: { orchestrator_iterations: this.state.iterationCount },
        latency_max_ms: { orchestrator_final_score: Math.round(finalScore * 10) },
        latency_samples: { orchestrator: 1 },
      });
    } catch (_e) {
      // metrics optional
    }

    return {
      task_id: this.state.taskId,
      status: "completed",
      iterations: this.state.iterationCount,
      final_score: finalScore,
      result: finalResult,
      route: routeDecision,
    };
  }

  signal(name: string, payload: Record<string, unknown>): void {
    if (name === "feedback") {
      const fb = String(payload.content ?? payload.feedback ?? "");
      this.state.signals.push(fb);
      if (fb) {
        this.state.result = fb;
      }
    } else if (name === "reset") {
      this.state.iterationCount = 0;
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "progress") {
      return {
        task_id: this.state.taskId,
        status: this.state.status,
        current_step: this.state.currentStep,
        iteration_count: this.state.iterationCount,
        final_score: this.state.finalScore,
      };
    }
    if (name === "history") {
      return {
        signals: [...this.state.signals],
        iteration_count: this.state.iterationCount,
      };
    }
    return { error: `unknown_query: ${name}` };
  }
}

// ============================================================
// Actor routing
// ============================================================

const router = new ActorRouter({
  router: () => new RouterActor(),
  chain: () => new ChainActor(),
  judge: () => new JudgeActor(),
  orchestrator: () => new OrchestratorWorkflow(),
  pipeline_audit: () => new PipelineAuditActor(),
  quality_fsm: () => new QualityFSMActor(),
});

export const actor = {
  init: (configJson: string) => router.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string) => router.setState(stateJson),
};
