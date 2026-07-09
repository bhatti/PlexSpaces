// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { AgentLoop, defaultAgentConfig, agentActor } from '../dist/agent.js';

// ── AgentLoop: OODA steps ────────────────────────────────────────────────────

describe('AgentLoop OODA steps', () => {
  it('records four OODA steps in order', () => {
    const loop = new AgentLoop('test-actor', defaultAgentConfig());
    const obs = loop.observe({ task: 'summarise' });
    const plan = loop.orient(obs);
    const action = loop.decide(plan);
    loop.act(action, { inputTokens: 5, outputTokens: 8, model: 'claude-3' });

    const traj = loop.getTrajectory();
    assert.equal(traj.steps.length, 4);
    assert.deepEqual(
      traj.steps.map((s) => s.kind),
      ['observe', 'orient', 'decide', 'act'],
    );
  });

  it('observe returns input unchanged', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const inp = { data: 42 };
    assert.deepEqual(loop.observe(inp), inp);
  });

  it('orient returns input unchanged', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const inp = { plan: 'do-x' };
    assert.deepEqual(loop.orient(inp), inp);
  });

  it('decide returns input unchanged', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const inp = { action: 'call-tool' };
    assert.deepEqual(loop.decide(inp), inp);
  });

  it('act accumulates token totals', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    loop.act({}, { inputTokens: 10, outputTokens: 20 });
    const traj = loop.getTrajectory();
    assert.equal(traj.totalInputTokens, 10);
    assert.equal(traj.totalOutputTokens, 20);
  });
});

// ── AgentLoop: tool_call ─────────────────────────────────────────────────────

describe('AgentLoop toolCall', () => {
  it('records tool_call step with correct kind and toolName', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const result = loop.toolCall('web_search', { query: 'actors' }, { hits: 5 });

    assert.deepEqual(result, { hits: 5 });
    const traj = loop.getTrajectory();
    assert.equal(traj.steps.length, 1);
    assert.equal(traj.steps[0].kind, 'tool_call');
    assert.equal(traj.steps[0].toolName, 'web_search');
  });

  it('toolCall accumulates tokens', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    loop.toolCall('calc', {}, {}, { inputTokens: 7, outputTokens: 3 });
    const traj = loop.getTrajectory();
    assert.equal(traj.totalInputTokens, 7);
    assert.equal(traj.totalOutputTokens, 3);
  });
});

// ── AgentLoop: token budget ───────────────────────────────────────────────────

describe('AgentLoop budget enforcement', () => {
  it('returns false when below budget', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), tokenBudget: 100 });
    loop.act({}, { inputTokens: 40, outputTokens: 40 });
    assert.equal(loop.budgetExceeded(), false);
  });

  it('returns true at exact budget', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), tokenBudget: 100 });
    loop.act({}, { inputTokens: 50, outputTokens: 50 });
    assert.equal(loop.budgetExceeded(), true);
  });

  it('returns false when tokenBudget is 0 (unlimited)', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), tokenBudget: 0 });
    loop.act({}, { inputTokens: 99999, outputTokens: 99999 });
    assert.equal(loop.budgetExceeded(), false);
  });
});

// ── AgentLoop: iteration limit ───────────────────────────────────────────────

describe('AgentLoop iteration limit', () => {
  it('returns false below max', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), maxIterations: 3 });
    loop.incrementIteration();
    loop.incrementIteration();
    assert.equal(loop.iterationLimitReached(), false);
  });

  it('returns true at max', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), maxIterations: 2 });
    loop.incrementIteration();
    loop.incrementIteration();
    assert.equal(loop.iterationLimitReached(), true);
  });

  it('returns false when maxIterations is 0 (unlimited)', () => {
    const loop = new AgentLoop('unknown', { ...defaultAgentConfig(), maxIterations: 0 });
    for (let i = 0; i < 1000; i++) loop.incrementIteration();
    assert.equal(loop.iterationLimitReached(), false);
  });
});

// ── AgentLoop: suspend ───────────────────────────────────────────────────────

describe('AgentLoop suspend', () => {
  it('isSuspended is false by default', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    assert.equal(loop.isSuspended, false);
  });

  it('suspend sets isSuspended and records a step', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    loop.suspend('awaiting_approval');
    assert.equal(loop.isSuspended, true);
    const traj = loop.getTrajectory();
    assert.equal(traj.steps.length, 1);
    assert.equal(traj.steps[0].kind, 'suspend');
    assert.equal(traj.steps[0].input, 'awaiting_approval');
  });
});

// ── AgentLoop: finalizeTrajectory ─────────────────────────────────────────────

describe('AgentLoop finalizeTrajectory', () => {
  it('sets outcome and detail', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    loop.act({}, { inputTokens: 5, outputTokens: 10 });
    const traj = loop.finalizeTrajectory('success', 'done');
    assert.equal(traj.outcome, 'success');
    assert.equal(traj.outcomeDetail, 'done');
  });

  it('trajectory_id is non-empty', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const traj = loop.finalizeTrajectory('success');
    assert.ok(traj.trajectoryId.length > 0);
  });

  it('token totals are correct', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    loop.act({}, { inputTokens: 20, outputTokens: 30 });
    loop.act({}, { inputTokens: 10, outputTokens: 5 });
    const traj = loop.finalizeTrajectory('success');
    assert.equal(traj.totalInputTokens, 30);
    assert.equal(traj.totalOutputTokens, 35);
  });

  it('agentActorId is set from constructor param', () => {
    const loop = new AgentLoop('my-actor-007', defaultAgentConfig());
    const traj = loop.finalizeTrajectory('success');
    assert.equal(traj.agentActorId, 'my-actor-007');
  });

  it('completedAtMs and durationMs are set', () => {
    const loop = new AgentLoop('unknown', defaultAgentConfig());
    const traj = loop.finalizeTrajectory('success');
    assert.ok(traj.completedAtMs > 0);
    assert.ok(traj.durationMs >= 0);
  });
});

// ── @agentActor decorator ─────────────────────────────────────────────────────

describe('agentActor decorator', () => {
  it('injects agentLoop property into actor instances', () => {
    const decorator = agentActor({ maxIterations: 5, tokenBudget: 500 });

    class MyActor {}
    decorator(MyActor);

    const instance = new MyActor();
    const loop = instance.agentLoop;
    assert.ok(loop instanceof AgentLoop);
  });

  it('agentLoop is not enumerable on instance', () => {
    const decorator = agentActor({ maxIterations: 5 });
    class MyActor {}
    decorator(MyActor);
    const instance = new MyActor();
    // Touch agentLoop to trigger lazy init
    void instance.agentLoop;
    const keys = Object.keys(instance);
    assert.ok(!keys.includes('agentLoop'));
    assert.ok(!keys.includes('_agentLoop'));
  });

  it('each instance gets its own AgentLoop', () => {
    const decorator = agentActor({ maxIterations: 3 });
    class MyActor {}
    decorator(MyActor);

    const a = new MyActor();
    const b = new MyActor();
    a.agentLoop.observe('a');
    assert.equal(a.agentLoop.getTrajectory().steps.length, 1);
    assert.equal(b.agentLoop.getTrajectory().steps.length, 0);
  });
});
