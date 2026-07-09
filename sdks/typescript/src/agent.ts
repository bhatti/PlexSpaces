// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// AgentLoop standalone utility — OODA-loop agent harness for PlexSpaces TypeScript SDK.
// TypeScript SDK parity with Python sdks/python/plexspaces/agent.py.
//
// Provides:
// - Structured step recording (each observe/orient/decide/act journaled in AgentTrajectory)
// - Token budget enforcement (budgetExceeded() halts loop when cumulative tokens >= budget)
// - Iteration limit enforcement (iterationLimitReached() halts loop after N iterations)
// - Human-in-the-loop suspend (suspend() sets isSuspended to true)
// - Trajectory capture (finalizeTrajectory / getTrajectory)
//
// AgentLoop is a standalone plain class — it is NOT a decorator and does NOT wrap
// other types. Use agentActor() as a convenience decorator to inject an AgentLoop
// into actor instances.
//
// Usage:
//   import { agentActor, AgentLoop } from '@plexspaces/sdk';
//
//   @agentActor({ maxIterations: 10, tokenBudget: 4096 })
//   class ResearchAgent extends PlexSpacesActor<ResearchState> {
//     getDefaultState() { return { result: '' }; }
//     run(payload: Record<string, unknown>) {
//       const obs = this.agentLoop.observe(payload);
//       const plan = this.agentLoop.orient(obs);
//       const action = this.agentLoop.decide(plan);
//       const result = this.agentLoop.act(action);
//       return this.agentLoop.finalizeTrajectory('success');
//     }
//   }

/** Step kind in an OODA agent trajectory. */
export type AgentStepKind =
  | 'observe'
  | 'orient'
  | 'decide'
  | 'act'
  | 'tool_call'
  | 'suspend';

/**
 * A single recorded step in an agent trajectory.
 *
 * Mirrors Python `AgentStep` dataclass. All timing fields are milliseconds
 * since epoch (Date.now()).
 */
export interface AgentStep {
  /** Unique ULID-style step identifier. */
  stepId: string;
  /** OODA phase or special step kind. */
  kind: AgentStepKind;
  /** Method or tool name that produced this step. */
  method: string;
  /** Input data passed into the step. */
  input: unknown;
  /** Output produced by the step (undefined until completed). */
  output: unknown;
  /** Epoch-millisecond timestamp when the step started. */
  startedAtMs: number;
  /** Epoch-millisecond timestamp when the step completed (0 if pending). */
  completedAtMs: number;
  /** Wall-clock duration in milliseconds. */
  durationMs: number;
  /** Whether the step succeeded. */
  success: boolean;
  /** Error message if the step failed; undefined on success. */
  error?: string;
  /** Tool name for tool_call steps; undefined otherwise. */
  toolName?: string;
  /** LLM input tokens consumed in this step. */
  inputTokens: number;
  /** LLM output tokens produced in this step. */
  outputTokens: number;
  /** Model identifier used in this step (empty string if N/A). */
  model: string;
  /** Arbitrary key/value metadata attached to this step. */
  metadata: Record<string, unknown>;
}

/**
 * Complete agent execution trajectory.
 *
 * Mirrors Python `AgentTrajectory` dataclass. Captured from the first
 * observe/orient/decide/act call through finalizeTrajectory().
 */
export interface AgentTrajectory {
  /** Unique ULID-style trajectory identifier. */
  trajectoryId: string;
  /** Actor ID of the agent that produced this trajectory. */
  agentActorId: string;
  /** Eval harness run ID (for offline evaluation). */
  evalRunId: string;
  /** Scenario ID (for offline evaluation). */
  scenarioId: string;
  /** Ordered list of recorded steps. */
  steps: AgentStep[];
  /** Lifecycle outcome: 'running' | 'success' | 'failed' | 'suspended' | ... */
  outcome: string;
  /** Human-readable outcome detail. */
  outcomeDetail: string;
  /** Sum of inputTokens across all steps. */
  totalInputTokens: number;
  /** Sum of outputTokens across all steps. */
  totalOutputTokens: number;
  /** Epoch-millisecond timestamp when the trajectory started. */
  startedAtMs: number;
  /** Epoch-millisecond timestamp when the trajectory completed (0 while running). */
  completedAtMs: number;
  /** Total wall-clock duration in milliseconds (0 while running). */
  durationMs: number;
  /** Optional evaluation score attached post-run. */
  score: number;
  /** Arbitrary key/value metadata attached to this trajectory. */
  metadata: Record<string, unknown>;
}

/**
 * Configuration for an agent actor's loop.
 *
 * Mirrors the keyword arguments of Python `@agent_actor(...)`.
 */
export interface AgentConfig {
  /** Maximum OODA iterations before forced stop. 0 = unlimited. */
  maxIterations: number;
  /** Maximum cumulative tokens before forced stop. 0 = unlimited. */
  tokenBudget: number;
  /** Eval run ID for trajectory tagging (empty = not tracked). */
  evalRunId: string;
  /** Scenario ID for trajectory tagging (empty = not tagged). */
  scenarioId: string;
}

/**
 * Returns a sensible default AgentConfig.
 *
 * @returns `{ maxIterations: 10, tokenBudget: 0, evalRunId: '', scenarioId: '' }`
 */
export function defaultAgentConfig(): AgentConfig {
  return {
    maxIterations: 10,
    tokenBudget: 0,
    evalRunId: '',
    scenarioId: '',
  };
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/** Returns epoch milliseconds. Uses Date.now() which is safe in all environments. */
function nowMs(): number {
  return Date.now();
}

/** Generate a lightweight unique ID. Uses Math.random for WASM portability. */
function newStepId(): string {
  const ts = nowMs().toString(36);
  const rnd = Math.random().toString(36).slice(2, 9);
  return `step-${ts}-${rnd}`;
}

/** Generate a trajectory ID with different prefix. */
function newTrajectoryId(): string {
  const ts = nowMs().toString(36);
  const rnd = Math.random().toString(36).slice(2, 9);
  return `traj-${ts}-${rnd}`;
}

// ─── AgentLoop ────────────────────────────────────────────────────────────────

/**
 * Stateful OODA-loop harness for agent actors.
 *
 * Tracks steps, tokens, suspension state, and iteration count. The class is
 * injected into actor instances by `agentActor()`. Call `getTrajectory()` for
 * a live snapshot and `finalizeTrajectory(outcome)` to close the trajectory.
 *
 * Example:
 * ```ts
 * const loop = new AgentLoop('my-actor-id', { maxIterations: 5, tokenBudget: 1000, evalRunId: '', scenarioId: '' });
 * const obs = loop.observe({ query: 'hello' });
 * const plan = loop.orient(obs);
 * loop.act(plan, { inputTokens: 50, outputTokens: 20, model: 'claude-3' });
 * const traj = loop.finalizeTrajectory('success');
 * ```
 */
export class AgentLoop {
  private readonly config: AgentConfig;
  private readonly agentActorId: string;
  private readonly trajectory: AgentTrajectory;
  private _isSuspended = false;
  private _iterationCount = 0;

  /**
   * @param agentActorId - Actor ID embedded in trajectory metadata.
   * @param config - Agent loop configuration (maxIterations, tokenBudget, etc.)
   */
  constructor(agentActorId: string, config: AgentConfig) {
    this.config = { ...config };
    this.agentActorId = agentActorId;
    this.trajectory = {
      trajectoryId: newTrajectoryId(),
      agentActorId,
      evalRunId: config.evalRunId,
      scenarioId: config.scenarioId,
      steps: [],
      outcome: 'running',
      outcomeDetail: '',
      totalInputTokens: 0,
      totalOutputTokens: 0,
      startedAtMs: nowMs(),
      completedAtMs: 0,
      durationMs: 0,
      score: 0,
      metadata: {},
    };
  }

  // ─── Public step methods ───────────────────────────────────────────────────

  /**
   * Record an OBSERVE step: gather information from environment, memory, or context.
   *
   * @param input - Raw observation data.
   * @returns The same input (pass-through).
   */
  observe(input: unknown): unknown {
    return this.recordStep('observe', 'observe', input, input);
  }

  /**
   * Record an ORIENT step: process observations into a plan or understanding.
   *
   * @param obs - Observation data from the observe step.
   * @returns The same obs (pass-through).
   */
  orient(obs: unknown): unknown {
    return this.recordStep('orient', 'orient', obs, obs);
  }

  /**
   * Record a DECIDE step: select the next action from available options.
   *
   * @param plan - Planning data from the orient step.
   * @returns The same plan (pass-through).
   */
  decide(plan: unknown): unknown {
    return this.recordStep('decide', 'decide', plan, plan);
  }

  /**
   * Record an ACT step: execute the chosen action.
   *
   * @param action - Action data to execute.
   * @param opts - Optional token usage and model metadata.
   * @returns The same action (pass-through).
   */
  act(
    action: unknown,
    opts?: { inputTokens?: number; outputTokens?: number; model?: string },
  ): unknown {
    return this.recordStep('act', 'act', action, action, opts);
  }

  /**
   * Record a TOOL_CALL step: validated tool invocation with arguments and result.
   *
   * @param toolName - Name of the tool invoked.
   * @param args - Arguments passed to the tool.
   * @param result - Result returned by the tool.
   * @param opts - Optional token usage and model metadata.
   * @returns The result value (pass-through).
   */
  toolCall(
    toolName: string,
    args: unknown,
    result: unknown,
    opts?: { inputTokens?: number; outputTokens?: number; model?: string },
  ): unknown {
    const started = nowMs();
    const step: AgentStep = {
      stepId: newStepId(),
      kind: 'tool_call',
      method: `tool:${toolName}`,
      input: { name: toolName, arguments: args },
      output: result,
      startedAtMs: started,
      completedAtMs: nowMs(),
      durationMs: 0,
      success: true,
      toolName,
      inputTokens: opts?.inputTokens ?? 0,
      outputTokens: opts?.outputTokens ?? 0,
      model: opts?.model ?? '',
      metadata: {},
    };
    step.durationMs = step.completedAtMs - step.startedAtMs;
    this.addStep(step);
    return result;
  }

  /**
   * Suspend the agent loop, waiting for an external signal (human approval, etc.).
   *
   * After calling this, check `isSuspended` in your run loop and return early.
   *
   * @param reason - Human-readable reason for suspension.
   */
  suspend(reason: string): void {
    this._isSuspended = true;
    this.recordStep('suspend', 'suspend', reason, undefined);
  }

  /** Whether the agent loop has been suspended via `suspend()`. */
  get isSuspended(): boolean {
    return this._isSuspended;
  }

  /**
   * Returns true if cumulative token usage meets or exceeds the configured budget.
   * Always returns false when `tokenBudget` is 0 (unlimited).
   */
  budgetExceeded(): boolean {
    if (this.config.tokenBudget <= 0) return false;
    const used = this.trajectory.totalInputTokens + this.trajectory.totalOutputTokens;
    return used >= this.config.tokenBudget;
  }

  /**
   * Returns true if the iteration count meets or exceeds `maxIterations`.
   * Always returns false when `maxIterations` is 0 (unlimited).
   */
  iterationLimitReached(): boolean {
    if (this.config.maxIterations <= 0) return false;
    return this._iterationCount >= this.config.maxIterations;
  }

  /** Increment the iteration counter (call once per OODA loop pass). */
  incrementIteration(): void {
    this._iterationCount++;
  }

  /**
   * Close the trajectory and return the final snapshot.
   *
   * @param outcome - Outcome label (e.g. `'success'`, `'failed'`, `'suspended'`).
   * @param detail - Optional human-readable outcome detail.
   * @returns Completed AgentTrajectory snapshot.
   */
  finalizeTrajectory(outcome: string, detail = ''): AgentTrajectory {
    const now = nowMs();
    this.trajectory.outcome = outcome;
    this.trajectory.outcomeDetail = detail;
    this.trajectory.completedAtMs = now;
    this.trajectory.durationMs = now - this.trajectory.startedAtMs;
    return { ...this.trajectory, steps: [...this.trajectory.steps] };
  }

  /**
   * Return a live snapshot of the current trajectory (trajectory is still open).
   *
   * @returns Current AgentTrajectory (shallow copy of steps array).
   */
  getTrajectory(): AgentTrajectory {
    return { ...this.trajectory, steps: [...this.trajectory.steps] };
  }

  // ─── Private helpers ───────────────────────────────────────────────────────

  private recordStep(
    kind: AgentStepKind,
    method: string,
    input: unknown,
    output: unknown,
    opts?: { inputTokens?: number; outputTokens?: number; model?: string },
  ): unknown {
    const started = nowMs();
    const step: AgentStep = {
      stepId: newStepId(),
      kind,
      method,
      input,
      output,
      startedAtMs: started,
      completedAtMs: nowMs(),
      durationMs: 0,
      success: true,
      inputTokens: opts?.inputTokens ?? 0,
      outputTokens: opts?.outputTokens ?? 0,
      model: opts?.model ?? '',
      metadata: {},
    };
    step.durationMs = step.completedAtMs - step.startedAtMs;
    this.addStep(step);
    return output;
  }

  private addStep(step: AgentStep): void {
    this.trajectory.steps.push(step);
    this.trajectory.totalInputTokens += step.inputTokens;
    this.trajectory.totalOutputTokens += step.outputTokens;
  }
}

// ─── @agentActor decorator ────────────────────────────────────────────────────

/**
 * Class decorator that injects an `AgentLoop` into actor instances.
 *
 * Mirrors Python `@agent_actor(max_iterations=10, token_budget=4096, ...)`.
 * Purely additive — does not alter existing methods or WIT entry points.
 *
 * The decorator adds:
 * - `_agentLoop: AgentLoop` — private backing field (initialized on first access)
 * - `get agentLoop(): AgentLoop` — public accessor
 *
 * Example:
 * ```ts
 * import { agentActor, PlexSpacesActor } from '@plexspaces/sdk';
 *
 * @agentActor({ maxIterations: 5, tokenBudget: 2048 })
 * class SummaryAgent extends PlexSpacesActor<{ result: string }> {
 *   getDefaultState() { return { result: '' }; }
 *   run(payload: Record<string, unknown>) {
 *     const obs = this.agentLoop.observe(payload);
 *     // ...
 *     return this.agentLoop.finalizeTrajectory('success');
 *   }
 * }
 * ```
 *
 * @param config - Partial AgentConfig; unset fields use defaultAgentConfig() values.
 * @returns ClassDecorator that injects AgentLoop.
 */
export function agentActor(config?: Partial<AgentConfig>): ClassDecorator {
  const resolved: AgentConfig = { ...defaultAgentConfig(), ...config };

  return function <TFunction extends Function>(target: TFunction): TFunction {
    const original = target.prototype;

    // Define _agentLoop lazily on each instance so the constructor is not touched.
    // enumerable: false prevents the loop from appearing in JSON.stringify or for..in.
    Object.defineProperty(original, '_agentLoop', {
      get(this: Record<string, unknown>) {
        const key = '__agentLoopInstance__';
        if (!this[key]) {
          // Use actor id if available (set by framework after construction)
          const actorId =
            typeof this['actorId'] === 'string'
              ? (this['actorId'] as string)
              : 'unknown';
          this[key] = new AgentLoop(actorId, resolved);
        }
        return this[key];
      },
      enumerable: false,
      configurable: true,
    });

    // Public accessor — forwards to _agentLoop; not enumerable.
    Object.defineProperty(original, 'agentLoop', {
      get(this: Record<string, unknown>) {
        return (this as Record<string, AgentLoop>)['_agentLoop'];
      },
      enumerable: false,
      configurable: true,
    });

    return target;
  };
}
