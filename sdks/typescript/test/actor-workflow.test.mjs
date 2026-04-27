// SPDX-License-Identifier: AGPL-3.0-or-later
// Contract tests for Workflow routing (workflow_run, workflow_signal:name, workflow_query:name).
// The real implementation is in src/actor.ts handle(); this test documents and verifies the
// routing contract without loading the SDK (which has WIT virtual imports).

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

/**
 * Inline copy of the workflow routing contract from PlexSpacesActor.handle().
 * When msgType is workflow_run / workflow_signal:name / workflow_query:name,
 * dispatch to run / signal / query. Used to verify behavior without WASM imports.
 */
function routeWorkflowMessage(actor, msgType, payloadJson) {
  const payload = payloadJson && payloadJson.trim()
    ? JSON.parse(payloadJson)
    : {};
  if (msgType === 'workflow_run') {
    if (typeof actor.run === 'function') {
      const result = actor.run(payload);
      return JSON.stringify(result ?? {});
    }
  }
  if (msgType.startsWith('workflow_signal:')) {
    const name = msgType.slice('workflow_signal:'.length).trim();
    if (typeof actor.signal === 'function') {
      actor.signal(name, payload);
      return '{}';
    }
  }
  if (msgType.startsWith('workflow_query:')) {
    const name = msgType.slice('workflow_query:'.length).trim();
    if (typeof actor.query === 'function') {
      const result = actor.query(name, payload);
      return JSON.stringify(result ?? {});
    }
  }
  return null; // fallthrough to op-based dispatch
}

describe('Workflow routing contract', () => {
  it('workflow_run should call run(payload) and return JSON', () => {
    const received = [];
    const actor = {
      run(payload) {
        received.push(payload);
        return { status: 'ok', order_id: payload.order_id };
      },
      signal() {},
      query() {},
    };
    const out = routeWorkflowMessage(actor, 'workflow_run', '{"order_id":"o1","customer_id":"c1"}');
    assert.strictEqual(out, '{"status":"ok","order_id":"o1"}');
    assert.deepStrictEqual(received, [{ order_id: 'o1', customer_id: 'c1' }]);
  });

  it('workflow_signal:name should call signal(name, payload) and return {}', () => {
    const received = [];
    const actor = {
      run() { return {}; },
      signal(name, data) {
        received.push([name, data]);
      },
      query() { return {}; },
    };
    const out = routeWorkflowMessage(actor, 'workflow_signal:cancel', '{"reason":"user"}');
    assert.strictEqual(out, '{}');
    assert.deepStrictEqual(received, [['cancel', { reason: 'user' }]]);
  });

  it('workflow_query:name should call query(name, params) and return JSON', () => {
    const received = [];
    const actor = {
      run() { return {}; },
      signal() {},
      query(name, params) {
        received.push([name, params]);
        return { query: name, order_id: params.order_id };
      },
    };
    const out = routeWorkflowMessage(actor, 'workflow_query:status', '{"order_id":"o1"}');
    assert.strictEqual(out, '{"query":"status","order_id":"o1"}');
    assert.deepStrictEqual(received, [['status', { order_id: 'o1' }]]);
  });

  it('unknown msgType should return null (fallthrough)', () => {
    const actor = { run() {}, signal() {}, query() {} };
    const out = routeWorkflowMessage(actor, 'get_status', '{}');
    assert.strictEqual(out, null);
  });
});
