// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Shared protobuf wire primitives for WASM host payloads.
// Matches sdks/go/plexspaces/tuplespace_proto_wire.go and http_fetch_proto_wire.go.

/** Append non-negative protobuf varint (field tags, lengths, enums). */
export function appendVarint(buf: Uint8Array, xIn: number): Uint8Array<ArrayBuffer> {
  if (!Number.isFinite(xIn) || xIn < 0 || xIn > Number.MAX_SAFE_INTEGER) {
    throw new Error('appendVarint expects a non-negative safe integer');
  }
  let n = BigInt(Math.floor(xIn));
  const parts: number[] = [];
  while (n >= 0x80n) {
    parts.push(Number(n & 0xffn) | 0x80);
    n >>= 7n;
  }
  parts.push(Number(n));
  return concatBytes(buf, new Uint8Array(parts));
}

export function appendFixed64LE(buf: Uint8Array, v: number): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(buf.length + 8);
  out.set(buf, 0);
  const view = new DataView(out.buffer, buf.length, 8);
  view.setFloat64(0, v, true);
  return out;
}

export function appendLengthDelimited(
  buf: Uint8Array,
  fieldNum: number,
  inner: Uint8Array,
): Uint8Array<ArrayBuffer> {
  const tag = BigInt(fieldNum << 3 | 2);
  let b = appendVarint(buf, Number(tag));
  b = appendVarint(b, inner.length);
  return concatBytes(b, inner);
}

/**
 * Concatenate byte buffers into a new ArrayBuffer-backed Uint8Array.
 * Explicit return type avoids TS 5.9+ inferring ArrayBufferLike from subarrays / TextEncoder.
 */
export function concatBytes(a: Uint8Array, b: Uint8Array): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
}

export function readVarint(data: Uint8Array, pos: number): { value: bigint; n: number } {
  let x = 0n;
  let s = 0n;
  const orig = pos;
  for (let i = 0; i < 10; i++) {
    if (pos >= data.length) throw new Error('varint buffer underflow');
    const b = data[pos]!;
    pos++;
    if (b < 0x80) {
      return { value: x | (BigInt(b) << s), n: pos - orig };
    }
    x |= BigInt(b & 0x7f) << s;
    s += 7n;
  }
  throw new Error('varint too long');
}

export function skipField(data: Uint8Array, pos: number, wireType: number): number {
  switch (wireType) {
    case 0: {
      const { n } = readVarint(data, pos);
      return pos + n;
    }
    case 1:
      if (pos + 8 > data.length) throw new Error('fixed64 underflow');
      return pos + 8;
    case 2: {
      const { value: ln, n } = readVarint(data, pos);
      return pos + n + Number(ln);
    }
    case 5:
      if (pos + 4 > data.length) throw new Error('fixed32 underflow');
      return pos + 4;
    default:
      throw new Error(`unknown wire type ${wireType}`);
  }
}

export function readLengthDelimited(data: Uint8Array, pos: number): { slice: Uint8Array; nextPos: number } {
  const { value: ln, n } = readVarint(data, pos);
  const start = pos + n;
  const end = start + Number(ln);
  if (end > data.length) throw new Error('length-delimited field truncated');
  const copy = new Uint8Array(end - start);
  copy.set(data.subarray(start, end));
  return { slice: copy, nextPos: end };
}
