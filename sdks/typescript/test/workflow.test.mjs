// SPDX-License-Identifier: AGPL-3.0-or-later
// Unit tests for workflow fault-tolerance: withRetry (Step Functions / Durable Functions style).

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
// Import workflow module only (avoids loading SDK modules that use WIT virtual imports)
import { defaultRetryConfig, withRetry } from '../dist/workflow.js';

describe('defaultRetryConfig', () => {
  it('returns single-attempt config (max_attempts 1)', () => {
    const c = defaultRetryConfig();
    assert.strictEqual(c.max_attempts, 1);
    assert.strictEqual(c.initial_interval_ms, 100);
    assert.strictEqual(c.backoff_rate, 2);
    assert.strictEqual(c.max_interval_ms, 30000);
  });
});

describe('withRetry', () => {
  it('returns result on first success', () => {
    const result = withRetry(() => 42);
    assert.strictEqual(result, 42);
  });

  it('succeeds after one retry', () => {
    let calls = 0;
    const result = withRetry(
      () => {
        calls += 1;
        if (calls < 2) throw new Error('transient');
        return 100;
      },
      { max_attempts: 3 }
    );
    assert.strictEqual(result, 100);
    assert.strictEqual(calls, 2);
  });

  it('succeeds after two retries', () => {
    let calls = 0;
    const result = withRetry(
      () => {
        calls += 1;
        if (calls < 3) throw new Error('transient');
        return 200;
      },
      { max_attempts: 5 }
    );
    assert.strictEqual(result, 200);
    assert.strictEqual(calls, 3);
  });

  it('throws after max_attempts exhausted', () => {
    let calls = 0;
    assert.throws(
      () =>
        withRetry(
          () => {
            calls += 1;
            throw new Error('permanent');
          },
          { max_attempts: 3 }
        ),
      { message: 'permanent' }
    );
    assert.strictEqual(calls, 3);
  });

  it('uses default 3 attempts when config omitted', () => {
    let calls = 0;
    assert.throws(
      () =>
        withRetry(() => {
          calls += 1;
          throw new Error('fail');
        }),
      { message: 'fail' }
    );
    assert.strictEqual(calls, 3);
  });

  it('max_attempts 1 throws on first failure', () => {
    let calls = 0;
    assert.throws(
      () =>
        withRetry(
          () => {
            calls += 1;
            throw new Error('once');
          },
          { max_attempts: 1 }
        ),
      { message: 'once' }
    );
    assert.strictEqual(calls, 1);
  });
});
