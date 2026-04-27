// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Regression: WIT `list<u8>` arrives as Uint8Array in jco guests; JSON must still parse.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from '../dist/wit-payload.js';

describe('decodeWitPayloadUtf8', () => {
  it('passes through strings unchanged', () => {
    const s = '{"op":"status"}';
    assert.equal(decodeWitPayloadUtf8(s), s);
  });

  it('decodes UTF-8 Uint8Array to the same text as a string body', () => {
    const json = '{"op":"increment","amount":2}';
    const bytes = new TextEncoder().encode(json);
    assert.equal(decodeWitPayloadUtf8(bytes), json);
  });

  it('returns empty string for empty Uint8Array', () => {
    assert.equal(decodeWitPayloadUtf8(new Uint8Array(0)), '');
  });

  it('decodes a subarray view (offset into shared ArrayBuffer)', () => {
    const inner = new TextEncoder().encode('{"op":"x"}');
    const buf = new ArrayBuffer(inner.byteLength + 4);
    new Uint8Array(buf).set(inner, 2);
    const view = new Uint8Array(buf, 2, inner.byteLength);
    assert.equal(decodeWitPayloadUtf8(view), '{"op":"x"}');
  });
});

describe('encodeWitPayloadUtf8', () => {
  it('round-trips JSON through decode', () => {
    const json = '{"count":2,"self_id":"cart-1//abstractions::app@node"}';
    const bytes = encodeWitPayloadUtf8(json);
    assert.ok(bytes instanceof Uint8Array);
    assert.equal(decodeWitPayloadUtf8(bytes), json);
  });
});
