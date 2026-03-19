// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for collective / parallel shard-group host operations.
// Each test injects a mock rawCall that returns canned JSON,
// then validates the wrapper round-trips correctly.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// ---------------------------------------------------------------------------
// Helpers — mirrors the pattern from host-shardgroup.test.mjs
// Each helper simulates a Host method: serialize request → rawCall → parse.
// ---------------------------------------------------------------------------

function callWrapper(rawCall, request) {
  const result = rawCall(JSON.stringify(request));
  if (typeof result === 'string' && result.startsWith('ERROR:')) {
    throw new Error(result);
  }
  return JSON.parse(result);
}

// ---------------------------------------------------------------------------
// broadcastShardGroup
// ---------------------------------------------------------------------------
describe('broadcastShardGroup', () => {
  it('returns shard_responses and stats', () => {
    const mock = () => JSON.stringify({
      shard_responses: [{ shard_id: 0, success: true }],
      stats: { shards_queried: 3, shards_responded: 3, shards_failed: 0 },
    });
    const resp = callWrapper(mock, { group_id: 'workers', message: { op: 'reset' } });
    assert.equal(resp.stats.shards_queried, 3);
    assert.equal(resp.shard_responses.length, 1);
  });

  it('throws on ERROR response', () => {
    const mock = () => 'ERROR: group not found';
    assert.throws(() => callWrapper(mock, { group_id: 'missing' }), /ERROR/);
  });
});

// ---------------------------------------------------------------------------
// reduceShardGroup
// ---------------------------------------------------------------------------
describe('reduceShardGroup', () => {
  it('returns aggregated result and stats', () => {
    const mock = () => JSON.stringify({
      result: 42.0,
      shard_responses: [],
      stats: { shards_queried: 4, shards_responded: 4, shards_failed: 0 },
    });
    const resp = callWrapper(mock, {
      group_id: 'workers', query: { action: 'get_count' },
      reduction: 1, min_responses: 1,
    });
    assert.equal(resp.result, 42.0);
    assert.equal(resp.stats.shards_responded, 4);
  });

  it('throws on ERROR response', () => {
    const mock = () => 'ERROR: no reducible values';
    assert.throws(() => callWrapper(mock, { group_id: 'g1' }), /ERROR/);
  });
});

// ---------------------------------------------------------------------------
// allReduceShardGroup
// ---------------------------------------------------------------------------
describe('allReduceShardGroup', () => {
  it('returns result and shard_responses', () => {
    const mock = () => JSON.stringify({
      result: 100.0,
      shard_responses: [{ shard_id: 0, success: true }],
      stats: { shards_queried: 2, shards_responded: 2, shards_failed: 0 },
    });
    const resp = callWrapper(mock, {
      group_id: 'workers', query: { action: 'sum' },
      reduction: 1, min_responses: 1,
    });
    assert.equal(resp.result, 100.0);
    assert.equal(resp.stats.shards_queried, 2);
  });
});

// ---------------------------------------------------------------------------
// barrierShardGroup
// ---------------------------------------------------------------------------
describe('barrierShardGroup', () => {
  it('returns stats with barrier round info', () => {
    const mock = () => JSON.stringify({
      shard_responses: [{ shard_id: 0, success: true }],
      stats: { shards_queried: 3, shards_responded: 3, shards_failed: 0 },
    });
    const resp = callWrapper(mock, {
      group_id: 'workers', barrier_id: 'round-1', round: 1, min_acks: 1,
    });
    assert.equal(resp.stats.shards_queried, 3);
    assert.equal(resp.stats.shards_failed, 0);
  });

  it('throws on ERROR response', () => {
    const mock = () => 'ERROR: group not found';
    assert.throws(() => callWrapper(mock, { group_id: 'missing' }), /ERROR/);
  });
});

// ---------------------------------------------------------------------------
// spawnActors
// ---------------------------------------------------------------------------
describe('spawnActors', () => {
  it('returns results for each spawned actor', () => {
    const mock = () => JSON.stringify({
      results: [
        { success: true, error: '', response: { actor_ref: 'c-0@node', actor_id: 'c-0' } },
        { success: true, error: '', response: { actor_ref: 'c-1@node', actor_id: 'c-1' } },
      ],
    });
    const resp = callWrapper(mock, {
      requests: [
        { actor_type: 'counter', actor_id: 'c-0' },
        { actor_type: 'counter', actor_id: 'c-1' },
      ],
    });
    assert.equal(resp.results.length, 2);
    assert.ok(resp.results.every(r => r.success));
  });

  it('handles empty request', () => {
    const mock = () => JSON.stringify({ results: [] });
    const resp = callWrapper(mock, { requests: [] });
    assert.deepEqual(resp.results, []);
  });

  it('supports instances_count field', () => {
    const mock = (reqJSON) => {
      const req = JSON.parse(reqJSON);
      assert.equal(req.requests[0].instances_count, 3);
      return JSON.stringify({ results: [{ success: true }] });
    };
    const resp = callWrapper(mock, {
      requests: [{ actor_type: 'worker', instances_count: 3 }],
    });
    assert.ok(resp.results[0].success);
  });

  it('throws on ERROR response', () => {
    const mock = () => 'ERROR: spawn failed';
    assert.throws(() => callWrapper(mock, { requests: [] }), /ERROR/);
  });
});

// ---------------------------------------------------------------------------
// bulkUpdateShardGroup
// ---------------------------------------------------------------------------
describe('bulkUpdateShardGroup', () => {
  it('returns update statistics', () => {
    const mock = () => JSON.stringify({
      updates_sent: 5, updates_succeeded: 4, updates_failed: 1, errors: ['timeout'],
    });
    const resp = callWrapper(mock, { group_id: 'workers', updates: {} });
    assert.equal(resp.updates_sent, 5);
    assert.equal(resp.updates_failed, 1);
  });
});

// ---------------------------------------------------------------------------
// mapShardGroup
// ---------------------------------------------------------------------------
describe('mapShardGroup', () => {
  it('returns results and stats', () => {
    const mock = () => JSON.stringify({
      results: [{ shard_id: 0, response: { count: 10 } }],
      stats: { succeeded: 1, failed: 0, total: 1 },
    });
    const resp = callWrapper(mock, { group_id: 'workers', query: { action: 'status' } });
    assert.equal(resp.results.length, 1);
    assert.equal(resp.stats.total, 1);
  });
});

// ---------------------------------------------------------------------------
// scatterGather
// ---------------------------------------------------------------------------
describe('scatterGather', () => {
  it('returns shard_responses and stats', () => {
    const mock = () => JSON.stringify({
      shard_responses: [{ shard_id: 0, success: true, response: {} }],
      stats: { shards_queried: 4, shards_responded: 3, shards_failed: 1 },
    });
    const resp = callWrapper(mock, { group_id: 'workers', query: { action: 'get_all' } });
    assert.equal(resp.stats.shards_queried, 4);
    assert.equal(resp.stats.shards_failed, 1);
  });
});
