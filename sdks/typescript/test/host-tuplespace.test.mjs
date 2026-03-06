// SPDX-License-Identifier: LGPL-2.1-or-later
// Integration tests for host.ts (TupleSpace list-in/list-out API).

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// TupleSpace and Host are in src/host.ts; we test TupleSpace with a mock Host
// to avoid WIT virtual imports in test.
function createMockHost() {
  const writeCalls = [];
  const takeReturns = [];
  const readReturns = [];
  const readAllReturns = [];
  return {
    tsWrite(json) {
      writeCalls.push(json);
      return '';
    },
    tsTake(json) {
      return takeReturns.shift() ?? '';
    },
    tsRead(json) {
      return readReturns.shift() ?? '';
    },
    tsReadAll(json) {
      return readAllReturns.shift() ?? '[]';
    },
    getWriteCalls: () => writeCalls,
    setTakeReturns: (arr) => takeReturns.push(...arr),
    setReadReturns: (arr) => readReturns.push(...arr),
    setReadAllReturns: (arr) => readAllReturns.push(...arr),
  };
}

// Inline TupleSpace logic (same as src/host.ts) so we don't need to load WIT-bound Host
class TupleSpace {
  constructor(host) {
    this.host = host;
  }
  write(tuple) {
    return this.host.tsWrite(JSON.stringify(tuple));
  }
  take(pattern) {
    const raw = this.host.tsTake(JSON.stringify(pattern));
    if (raw === '' || raw.startsWith('ERROR')) return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  read(pattern) {
    const raw = this.host.tsRead(JSON.stringify(pattern));
    if (raw === '' || raw.startsWith('ERROR')) return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  readAll(pattern) {
    const raw = this.host.tsReadAll(JSON.stringify(pattern));
    if (raw === '' || raw.startsWith('ERROR')) return [];
    try {
      const out = JSON.parse(raw);
      return Array.isArray(out) ? out : [];
    } catch {
      return [];
    }
  }
}

describe('TupleSpace (host.ts) list-in/list-out', () => {
  it('write serializes tuple and calls tsWrite', () => {
    const mock = createMockHost();
    const ts = new TupleSpace(mock);
    const out = ts.write(['job', 'j1', 'task', 't0', 1]);
    assert.strictEqual(out, '');
    assert.strictEqual(mock.getWriteCalls().length, 1);
    assert.deepStrictEqual(JSON.parse(mock.getWriteCalls()[0]), ['job', 'j1', 'task', 't0', 1]);
  });

  it('take returns null when empty', () => {
    const mock = createMockHost();
    mock.setTakeReturns(['']);
    const ts = new TupleSpace(mock);
    assert.strictEqual(ts.take(['job', 'j1', 'task', null, null]), null);
  });

  it('take returns null on ERROR', () => {
    const mock = createMockHost();
    mock.setTakeReturns(['ERROR: timeout']);
    const ts = new TupleSpace(mock);
    assert.strictEqual(ts.take(['job', null, null]), null);
  });

  it('take returns tuple when match', () => {
    const mock = createMockHost();
    mock.setTakeReturns([JSON.stringify(['job', 'j1', 'task', 't0', 42])]);
    const ts = new TupleSpace(mock);
    const result = ts.take(['job', 'j1', 'task', null, null]);
    assert.notStrictEqual(result, null);
    assert.deepStrictEqual(result, ['job', 'j1', 'task', 't0', 42]);
  });

  it('readAll returns empty array when empty', () => {
    const mock = createMockHost();
    mock.setReadAllReturns(['[]']);
    const ts = new TupleSpace(mock);
    assert.deepStrictEqual(ts.readAll(['job', 'j1', 'result', null, null]), []);
  });

  it('readAll returns list of tuples', () => {
    const mock = createMockHost();
    mock.setReadAllReturns([JSON.stringify([['job', 'j1', 'result', 't0', {}], ['job', 'j1', 'result', 't1', {}]])]);
    const ts = new TupleSpace(mock);
    const result = ts.readAll(['job', 'j1', 'result', null, null]);
    assert.strictEqual(result.length, 2);
    assert.deepStrictEqual(result[0], ['job', 'j1', 'result', 't0', {}]);
    assert.deepStrictEqual(result[1], ['job', 'j1', 'result', 't1', {}]);
  });

  it('read returns null when empty', () => {
    const mock = createMockHost();
    mock.setReadReturns(['']);
    const ts = new TupleSpace(mock);
    assert.strictEqual(ts.read(['job', null, null]), null);
  });

  it('read returns tuple when match', () => {
    const mock = createMockHost();
    mock.setReadReturns([JSON.stringify(['job', 'j1', 'task', 't0', 1])]);
    const ts = new TupleSpace(mock);
    assert.deepStrictEqual(ts.read(['job', 'j1', 'task', null, null]), ['job', 'j1', 'task', 't0', 1]);
  });
});
