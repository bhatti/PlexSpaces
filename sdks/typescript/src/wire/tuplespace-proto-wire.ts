// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Protobuf wire for plexspaces.tuplespace.v1 WriteRequest / ReadRequest / ReadResponse
// (subset for WASM host imports). Byte-compatible with:
// - sdks/go/plexspaces/tuplespace_proto_wire.go
// - crates/wasm-runtime simple_component_host (prost)

import {
  appendFixed64LE,
  appendLengthDelimited,
  appendVarint,
  concatBytes,
  readLengthDelimited,
  readVarint,
  skipField,
} from './proto-wire-common.js';

const MIN_INT64 = -9223372036854775808n;
const MAX_INT64 = 9223372036854775807n;

/** Protobuf wire bytes backed by ArrayBuffer (matches host/WASM expectations). */
type WireU8 = Uint8Array<ArrayBuffer>;

function encodeTupleField(v: unknown, allowWildcardStar: boolean): WireU8 {
  if (v === null || v === undefined) {
    return appendVarint(new Uint8Array([0x38]), 1);
  }
  if (typeof v === 'string') {
    if (allowWildcardStar && v === '*') {
      return appendVarint(new Uint8Array([0x38]), 1);
    }
    const enc = new TextEncoder();
    const bytes = new Uint8Array(enc.encode(v));
    let inner = new Uint8Array([0x1a]);
    inner = appendVarint(inner, bytes.length);
    inner = concatBytes(inner, bytes);
    return inner;
  }
  if (typeof v === 'boolean') {
    const inner = new Uint8Array([0x20]);
    return appendVarint(inner, v ? 1 : 0);
  }
  if (typeof v === 'number' && Number.isFinite(v)) {
    const t = Math.trunc(v);
    if (t === v && t >= Number(MIN_INT64) && t <= Number(MAX_INT64)) {
      let inner = new Uint8Array([0x08]);
      inner = appendVarintSigned(inner, t);
      return inner;
    }
    let inner = new Uint8Array([0x11]);
    const tmp = new Uint8Array(8);
    new DataView(tmp.buffer).setFloat64(0, v, true);
    inner = concatBytes(inner, tmp);
    return inner;
  }
  throw new Error(`unsupported tuple field type ${typeof v}`);
}

function appendVarintSigned(buf: Uint8Array, xIn: number): WireU8 {
  let x = BigInt(xIn);
  if (x < 0n) x = BigInt.asUintN(64, x);
  const parts: number[] = [];
  let n = x;
  while (n >= 0x80n) {
    parts.push(Number(n & 0xffn) | 0x80);
    n >>= 7n;
  }
  parts.push(Number(n));
  return concatBytes(buf, new Uint8Array(parts));
}

function encodeTupleFields(tuple: unknown[], allowWildcardStar: boolean): WireU8 {
  let out: WireU8 = new Uint8Array(0);
  for (const el of tuple) {
    const tf = encodeTupleField(el, allowWildcardStar);
    out = appendLengthDelimited(out, 2, tf);
  }
  return out;
}

/** Build WriteRequest protobuf bytes (field 1: tuple with repeated TupleField). */
export function encodeWriteRequest(tuple: unknown[]): Uint8Array {
  const tupleBody = encodeTupleFields(tuple, false);
  return appendLengthDelimited(new Uint8Array(0), 1, tupleBody);
}

/** Build ReadRequest protobuf bytes. */
export function encodeReadRequest(pattern: unknown[], take: boolean, maxResults: number): Uint8Array {
  const templateBody = encodeTupleFields(pattern, true);
  let out = appendLengthDelimited(new Uint8Array(0), 1, templateBody);
  if (take) {
    out = concatBytes(out, new Uint8Array([0x20, 0x01]));
  }
  out = concatBytes(out, new Uint8Array([0x28]));
  out = appendVarint(out, maxResults >>> 0);
  return out;
}

function parseTupleFieldMsg(msg: Uint8Array): unknown {
  let pos = 0;
  let last: unknown = undefined;
  while (pos < msg.length) {
    const { value: tag, n: tn } = readVarint(msg, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (wt === 0) {
      const { value: v, n: m } = readVarint(msg, pos);
      pos += m;
      if (fn === 1) last = Number(v);
      else if (fn === 4) last = v !== 0n;
      else if (fn === 6 || fn === 7) last = null;
    } else if (wt === 1) {
      if (pos + 8 > msg.length) throw new Error('double underflow');
      const view = new DataView(msg.buffer, msg.byteOffset + pos, 8);
      const d = view.getFloat64(0, true);
      pos += 8;
      if (fn === 2) last = d;
    } else if (wt === 2) {
      const { slice: chunk, nextPos } = readLengthDelimited(msg, pos);
      pos = nextPos;
      if (fn === 3 || fn === 5) {
        last = new TextDecoder('utf-8', { fatal: false }).decode(chunk);
      }
    } else {
      pos = skipField(msg, pos, wt);
    }
  }
  return last;
}

function parseTupleMsg(msg: Uint8Array): unknown[] {
  const fields: unknown[] = [];
  let pos = 0;
  while (pos < msg.length) {
    const { value: tag, n: tn } = readVarint(msg, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 2 && wt === 2) {
      const { slice: sub, nextPos } = readLengthDelimited(msg, pos);
      pos = nextPos;
      fields.push(parseTupleFieldMsg(sub));
    } else {
      pos = skipField(msg, pos, wt);
    }
  }
  return fields;
}

/** Parse ReadResponse bytes into list of tuple field arrays. */
export function parseReadResponseTuples(data: Uint8Array): unknown[][] {
  const tuples: unknown[][] = [];
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      tuples.push(parseTupleMsg(slice));
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return tuples;
}

export function decodeReadResponseFirstTuple(raw: Uint8Array): unknown[] | null {
  if (raw.length === 0) return null;
  try {
    const tuples = parseReadResponseTuples(raw);
    if (tuples.length === 0) return null;
    return tuples[0] ?? null;
  } catch {
    return null;
  }
}

export function decodeReadResponseAllTuples(raw: Uint8Array): unknown[][] {
  if (raw.length === 0) return [];
  try {
    return parseReadResponseTuples(raw);
  } catch {
    return [];
  }
}
