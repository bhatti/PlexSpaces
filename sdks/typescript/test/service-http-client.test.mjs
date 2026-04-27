// SPDX-License-Identifier: AGPL-3.0-or-later
// Tests for ServiceHttpClient (outbound HTTP service-link client).
// Uses a mock httpFetch to avoid WIT virtual imports.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// Inline the logic from src/host.ts to avoid WIT virtual imports in test.
class MockHost {
  constructor() {
    this._calls = [];
    this._response = JSON.stringify({ status: 200, headers: {}, body: '' });
  }
  setResponse(resp) {
    this._response = JSON.stringify(resp);
  }
  httpFetch(linkName, method, pathAndQuery, headers, body) {
    this._calls.push({ linkName, method, pathAndQuery, headers, body });
    const result = this._response;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  getCalls() { return this._calls; }
}

class ServiceHttpClient {
  constructor(mockHost, linkName) {
    this._host = mockHost;
    this._linkName = linkName;
  }
  get(pathAndQuery, headers) {
    return this._host.httpFetch(this._linkName, 'GET', pathAndQuery, headers ?? {}, '');
  }
  post(pathAndQuery, body, headers) {
    const bodyStr = body !== undefined ? JSON.stringify(body) : '';
    return this._host.httpFetch(this._linkName, 'POST', pathAndQuery, headers ?? {}, bodyStr);
  }
  put(pathAndQuery, body, headers) {
    const bodyStr = body !== undefined ? JSON.stringify(body) : '';
    return this._host.httpFetch(this._linkName, 'PUT', pathAndQuery, headers ?? {}, bodyStr);
  }
  delete(pathAndQuery, headers) {
    return this._host.httpFetch(this._linkName, 'DELETE', pathAndQuery, headers ?? {}, '');
  }
}

describe('ServiceHttpClient', () => {
  it('get sends GET request to correct link and path', () => {
    const mockHost = new MockHost();
    const client = new ServiceHttpClient(mockHost, 'payments-api');
    const resp = client.get('/v1/balance?account=123');
    assert.equal(resp.status, 200);
    const calls = mockHost.getCalls();
    assert.equal(calls.length, 1);
    assert.equal(calls[0].linkName, 'payments-api');
    assert.equal(calls[0].method, 'GET');
    assert.equal(calls[0].pathAndQuery, '/v1/balance?account=123');
  });

  it('post serializes body as JSON', () => {
    const mockHost = new MockHost();
    mockHost.setResponse({ status: 201, headers: {}, body: '{"id":"abc"}' });
    const client = new ServiceHttpClient(mockHost, 'payments-api');
    const resp = client.post('/v1/transfer', { amount: 100 });
    assert.equal(resp.status, 201);
    const calls = mockHost.getCalls();
    assert.equal(calls[0].method, 'POST');
    assert.equal(calls[0].body, '{"amount":100}');
  });

  it('put serializes body as JSON', () => {
    const mockHost = new MockHost();
    const client = new ServiceHttpClient(mockHost, 'inventory-api');
    client.put('/v1/items/1', { name: 'updated' });
    const calls = mockHost.getCalls();
    assert.equal(calls[0].method, 'PUT');
    assert.equal(calls[0].body, '{"name":"updated"}');
  });

  it('delete sends DELETE with no body', () => {
    const mockHost = new MockHost();
    const client = new ServiceHttpClient(mockHost, 'inventory-api');
    client.delete('/v1/items/1');
    const calls = mockHost.getCalls();
    assert.equal(calls[0].method, 'DELETE');
    assert.equal(calls[0].body, '');
  });

  it('throws on ERROR response', () => {
    const mockHost = new MockHost();
    mockHost.setResponse('ERROR: circuit open');
    const client = new ServiceHttpClient(mockHost, 'flaky-api');
    // Override to return error string
    mockHost.httpFetch = () => { throw new Error('ERROR: circuit open'); };
    assert.throws(() => client.get('/v1/test'), /circuit open/);
  });
});
