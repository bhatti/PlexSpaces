// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for shard-group-proto-wire.ts encoder/decoder correctness.
// Verifies correct proto field numbers per the authoritative proto definitions:
//   ScatterGatherRequest:    request_id=1, group_id=2, query=3, timeout=4, aggregation=5, min_responses=6
//   CreateShardGroupRequest: request_id=1, config=2, actor_type=3, initial_state=5, metadata=6
//   ScatterGatherResponse:   request_id=1, result=2, shard_responses=3, stats=4
//   BroadcastShardGroupResponse: request_id=1, shard_responses=2, stats=3
//   ShardQueryResponse:      request_id=1, shard_id=2, shard_actor_id=3, response=4, latency=5, success=6, error=7

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  encodeScatterGatherRequest,
  encodeCreateShardGroupRequest,
  encodeBroadcastShardGroupRequest,
  encodeReduceShardGroupRequest,
  encodeBarrierShardGroupRequest,
  decodeScatterGatherResponse,
  decodeBroadcastShardGroupResponse,
  decodeBarrierShardGroupResponse,
  decodeReduceShardGroupResponse,
  decodeCreateShardGroupResponse,
} from '../dist/wire/shard-group-proto-wire.js';

// ---------------------------------------------------------------------------
// Proto wire helpers (minimal — enough to build test payloads)
// ---------------------------------------------------------------------------

function encodeVarint(n) {
  const bytes = [];
  let v = BigInt(n);
  while (v >= 0x80n) {
    bytes.push(Number(v & 0xffn) | 0x80);
    v >>= 7n;
  }
  bytes.push(Number(v));
  return new Uint8Array(bytes);
}

function encodeField(fieldNum, wireType, valueBytes) {
  const tag = (fieldNum << 3) | wireType;
  const tagBytes = encodeVarint(tag);
  const out = new Uint8Array(tagBytes.length + valueBytes.length);
  out.set(tagBytes, 0);
  out.set(valueBytes, tagBytes.length);
  return out;
}

function encodeVarintField(fieldNum, v) {
  return concat(encodeVarint((fieldNum << 3) | 0), encodeVarint(v));
}

function encodeLenField(fieldNum, data) {
  const lenBytes = encodeVarint(data.length);
  const out = new Uint8Array(1 + lenBytes.length + data.length);
  const tag = encodeVarint((fieldNum << 3) | 2);
  let off = 0;
  out.set(tag, off); off += tag.length;
  out.set(lenBytes, off); off += lenBytes.length;
  out.set(data, off);
  return out;
}

function encodeStringField(fieldNum, s) {
  const enc = new TextEncoder();
  return encodeLenField(fieldNum, enc.encode(s));
}

function concat(...arrays) {
  const total = arrays.reduce((sum, a) => sum + a.length, 0);
  const out = new Uint8Array(total);
  let off = 0;
  for (const a of arrays) { out.set(a, off); off += a.length; }
  return out;
}

// Parse top-level field numbers from proto bytes (returns Set<number>).
function parseFieldNums(data) {
  const fields = new Set();
  let pos = 0;
  while (pos < data.length) {
    let tag = 0n;
    let shift = 0n;
    while (true) {
      const b = data[pos++];
      tag |= BigInt(b & 0x7f) << shift;
      shift += 7n;
      if (!(b & 0x80)) break;
    }
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    fields.add(fn);
    // skip value
    if (wt === 0) {
      // varint
      while (data[pos++] & 0x80) {}
    } else if (wt === 2) {
      // length-delimited
      let len = 0n; let sh = 0n;
      while (true) {
        const b = data[pos++];
        len |= BigInt(b & 0x7f) << sh;
        sh += 7n;
        if (!(b & 0x80)) break;
      }
      pos += Number(len);
    } else {
      break; // unexpected, stop
    }
  }
  return fields;
}

// ---------------------------------------------------------------------------
// Encoder tests
// ---------------------------------------------------------------------------

describe('encodeScatterGatherRequest — field numbers', () => {
  it('has request_id=1, group_id=2, query=3, timeout=4, aggregation=5, min_responses=6', () => {
    const buf = encodeScatterGatherRequest({
      group_id: 'grp',
      query: { op: 'compute', value: 42 },
      timeout_ms: 5000,
      aggregation: 'concat',
      min_responses: 2,
    });
    const fields = parseFieldNums(buf);
    for (const f of [1, 2, 3, 4, 5, 6]) {
      assert.ok(fields.has(f), `expected field ${f}, got ${[...fields]}`);
    }
  });

  it('encodes group_id at field 2 (not 1)', () => {
    const buf = encodeScatterGatherRequest({ group_id: 'test-group', query: { op: 'ping' } });
    // field 1 = request_id (ulid), field 2 = group_id
    // Decode field 2 as string
    let pos = 0;
    let foundGroupId = false;
    while (pos < buf.length) {
      let tag = 0n; let sh = 0n;
      while (true) { const b = buf[pos++]; tag |= BigInt(b & 0x7f) << sh; sh += 7n; if (!(b & 0x80)) break; }
      const fn = Number(tag >> 3n); const wt = Number(tag & 7n);
      if (fn === 2 && wt === 2) {
        let len = 0n; let ls = 0n;
        while (true) { const b = buf[pos++]; len |= BigInt(b & 0x7f) << ls; ls += 7n; if (!(b & 0x80)) break; }
        const str = new TextDecoder().decode(buf.slice(pos, pos + Number(len)));
        assert.equal(str, 'test-group');
        foundGroupId = true;
        break;
      } else if (wt === 0) {
        while (buf[pos++] & 0x80) {}
      } else if (wt === 2) {
        let len = 0n; let ls = 0n;
        while (true) { const b = buf[pos++]; len |= BigInt(b & 0x7f) << ls; ls += 7n; if (!(b & 0x80)) break; }
        pos += Number(len);
      }
    }
    assert.ok(foundGroupId, 'group_id not found at field 2');
  });

  it('encodes message_type from op field inside query Message', () => {
    const buf = encodeScatterGatherRequest({ group_id: 'g', query: { op: 'train' } });
    // Field 3 = query (Message). Message field 5 = message_type.
    // We just verify the bytes contain "train" as message_type text.
    const text = new TextDecoder().decode(buf);
    assert.ok(text.includes('train'), 'message_type "train" not found in encoded bytes');
  });

  it('does not hardcode message_type as "call" when op is provided', () => {
    const buf = encodeScatterGatherRequest({ group_id: 'g', query: { op: 'custom_op' } });
    const text = new TextDecoder().decode(buf);
    assert.ok(!text.includes('\x04call'), 'hardcoded "call" message_type found but op was custom_op');
  });
});

describe('encodeCreateShardGroupRequest — field numbers', () => {
  it('has request_id=1, config=2, actor_type=3', () => {
    const buf = encodeCreateShardGroupRequest({
      group_id: 'g', actor_type: 'Worker', shard_count: 4,
    });
    const fields = parseFieldNums(buf);
    for (const f of [1, 2, 3]) {
      assert.ok(fields.has(f), `expected field ${f}, got ${[...fields]}`);
    }
  });

  it('encodes initial_state at field 5 (not 4)', () => {
    const buf = encodeCreateShardGroupRequest({
      group_id: 'g', actor_type: 'W', initial_state: { count: 0 },
    });
    const fields = parseFieldNums(buf);
    assert.ok(fields.has(5), `expected field 5 (initial_state), got ${[...fields]}`);
    assert.ok(!fields.has(4), `field 4 should not be set (shard_config is not encoded), got ${[...fields]}`);
  });

  it('encodes metadata at field 6', () => {
    const buf = encodeCreateShardGroupRequest({
      group_id: 'g', actor_type: 'W', metadata: { env: 'prod' },
    });
    const fields = parseFieldNums(buf);
    assert.ok(fields.has(6), `expected field 6 (metadata), got ${[...fields]}`);
  });
});

describe('encodeBroadcastShardGroupRequest — field numbers', () => {
  it('has request_id=1, group_id=2, message=3, timeout=4, min_acks=5', () => {
    const buf = encodeBroadcastShardGroupRequest({
      group_id: 'grp', message: { op: 'ping' }, timeout_ms: 2000, min_acks: 3,
    });
    const fields = parseFieldNums(buf);
    for (const f of [1, 2, 3, 4, 5]) {
      assert.ok(fields.has(f), `expected field ${f}, got ${[...fields]}`);
    }
  });
});

describe('encodeReduceShardGroupRequest — field numbers', () => {
  it('has request_id=1, group_id=2, map_function=3, timeout=4, min_responses=5, reduction=6, target=7', () => {
    const buf = encodeReduceShardGroupRequest({
      group_id: 'g', map_function: { op: 'sum' },
      timeout_ms: 3000, min_responses: 1, reduction: 'sum', target: 'result',
    });
    const fields = parseFieldNums(buf);
    for (const f of [1, 2, 3, 4, 5, 6, 7]) {
      assert.ok(fields.has(f), `expected field ${f}, got ${[...fields]}`);
    }
  });
});

describe('encodeBarrierShardGroupRequest — field numbers', () => {
  it('has request_id=1, group_id=2, barrier_id=3, round=4, timeout=5, min_acks=6', () => {
    const buf = encodeBarrierShardGroupRequest({
      group_id: 'g', barrier_id: 'b1', round: 2, timeout_ms: 1000, min_acks: 3,
    });
    const fields = parseFieldNums(buf);
    for (const f of [1, 2, 3, 4, 5, 6]) {
      assert.ok(fields.has(f), `expected field ${f}, got ${[...fields]}`);
    }
  });
});

// ---------------------------------------------------------------------------
// Decoder tests — build hand-encoded proto bytes and verify decoded output
// ---------------------------------------------------------------------------

// Build a minimal ShardQueryResponse proto:
// request_id=1 (skip), shard_id=2 (varint), shard_actor_id=3 (string),
// response=4 (embedded Message with payload at field 6), success=6 (varint)
function buildShardQueryResponseProto(shardId, actorId, payloadJson, success) {
  const enc2 = new TextEncoder();
  let buf = new Uint8Array(0);
  buf = concat(buf, encodeVarintField(2, shardId));
  buf = concat(buf, encodeStringField(3, actorId));
  if (payloadJson) {
    const payloadBytes = enc2.encode(payloadJson);
    let msg = new Uint8Array(0);
    msg = concat(msg, encodeLenField(6, payloadBytes));
    buf = concat(buf, encodeLenField(4, msg));
  }
  if (success) buf = concat(buf, encodeVarintField(6, 1));
  return buf;
}

// Build ScatterGatherStats proto: shards_queried=1, shards_responded=2, shards_failed=3
function buildStatsProto(queried, responded, failed) {
  return concat(
    encodeVarintField(1, queried),
    encodeVarintField(2, responded),
    encodeVarintField(3, failed),
  );
}

// Build ScatterGatherResponse: shard_responses=3, stats=4
function buildScatterGatherResponseProto(shardProtos, statsProto) {
  let buf = new Uint8Array(0);
  for (const sp of shardProtos) buf = concat(buf, encodeLenField(3, sp));
  if (statsProto) buf = concat(buf, encodeLenField(4, statsProto));
  return buf;
}

// Build BroadcastShardGroupResponse: shard_responses=2, stats=3
function buildBroadcastResponseProto(shardProtos, statsProto) {
  let buf = new Uint8Array(0);
  for (const sp of shardProtos) buf = concat(buf, encodeLenField(2, sp));
  if (statsProto) buf = concat(buf, encodeLenField(3, statsProto));
  return buf;
}

describe('decodeScatterGatherResponse — shard_responses at field 3', () => {
  it('decodes shard_responses from field 3 with stats from field 4', () => {
    const shard = buildShardQueryResponseProto(1, 'worker-1', '{"value":42}', true);
    const stats = buildStatsProto(4, 4, 0);
    const respBytes = buildScatterGatherResponseProto([shard], stats);

    const out = decodeScatterGatherResponse(respBytes);
    assert.equal(out.shard_responses.length, 1, 'expected 1 shard response');
    assert.equal(out.shard_responses[0].shard_id, 1);
    assert.equal(out.shard_responses[0].shard_actor_id, 'worker-1');
    assert.equal(out.shard_responses[0].success, true);
    assert.equal(out.stats.shards_queried, 4);
    assert.equal(out.stats.shards_responded, 4);
    assert.equal(out.stats.shards_failed, 0);
  });

  it('returns empty arrays and empty stats for empty input', () => {
    const out = decodeScatterGatherResponse(new Uint8Array(0));
    assert.equal(out.shard_responses.length, 0);
    assert.deepEqual(out.stats, {});
  });

  it('decodes multiple shard responses', () => {
    const shards = [
      buildShardQueryResponseProto(0, 'w-0', '{"v":1}', true),
      buildShardQueryResponseProto(1, 'w-1', '{"v":2}', true),
      buildShardQueryResponseProto(2, 'w-2', null, false),
    ];
    const stats = buildStatsProto(3, 2, 1);
    const out = decodeScatterGatherResponse(buildScatterGatherResponseProto(shards, stats));
    assert.equal(out.shard_responses.length, 3);
    assert.equal(out.stats.shards_queried, 3);
    assert.equal(out.stats.shards_responded, 2);
    assert.equal(out.stats.shards_failed, 1);
  });

  it('does NOT decode shard_responses if encoded at wrong field 2', () => {
    // Build a response with shard at field 2 (old wrong encoding) — decoder must ignore it
    const shard = buildShardQueryResponseProto(5, 'wrong', null, false);
    const buf = encodeLenField(2, shard); // field 2, not 3
    const out = decodeScatterGatherResponse(buf);
    assert.equal(out.shard_responses.length, 0, 'field 2 should not be decoded as shard_responses');
  });
});

describe('decodeShardQueryResponse — field numbers shard_id=2, shard_actor_id=3, success=6, error=7', () => {
  it('decodes shard_id at field 2 and shard_actor_id at field 3', () => {
    const shard = buildShardQueryResponseProto(7, 'actor-7', '{"ok":true}', true);
    const resp = buildScatterGatherResponseProto([shard], null);
    const out = decodeScatterGatherResponse(resp);
    assert.equal(out.shard_responses[0].shard_id, 7);
    assert.equal(out.shard_responses[0].shard_actor_id, 'actor-7');
  });

  it('decodes error at field 7', () => {
    const enc2 = new TextEncoder();
    let buf = encodeVarintField(2, 3); // shard_id = 3
    buf = concat(buf, encodeStringField(3, 'shard-3'));
    buf = concat(buf, encodeVarintField(6, 0)); // success = false
    buf = concat(buf, encodeStringField(7, 'timeout exceeded'));
    const resp = buildScatterGatherResponseProto([buf], null);
    const out = decodeScatterGatherResponse(resp);
    assert.equal(out.shard_responses[0].error, 'timeout exceeded');
    assert.equal(out.shard_responses[0].success, false);
  });

  it('does not put shard_id at field 1 (old wrong field number)', () => {
    // Encode shard_id at old wrong field 1 — decoder must ignore it
    let buf = encodeVarintField(1, 99); // field 1, not 2 = should be ignored as request_id
    buf = concat(buf, encodeVarintField(2, 5)); // correct shard_id = 5
    const resp = buildScatterGatherResponseProto([buf], null);
    const out = decodeScatterGatherResponse(resp);
    assert.equal(out.shard_responses[0].shard_id, 5, 'shard_id should come from field 2');
  });
});

describe('decodeBroadcastShardGroupResponse — shard_responses at field 2', () => {
  it('decodes shard_responses from field 2 and stats from field 3', () => {
    const shard = buildShardQueryResponseProto(0, 'w-0', null, true);
    const stats = buildStatsProto(2, 2, 0);
    const out = decodeBroadcastShardGroupResponse(buildBroadcastResponseProto([shard], stats));
    assert.equal(out.shard_responses.length, 1);
    assert.equal(out.stats.shards_queried, 2);
    assert.equal(out.stats.shards_responded, 2);
  });

  it('does NOT decode shard_responses if encoded at field 3 (scatter-gather layout)', () => {
    const shard = buildShardQueryResponseProto(0, 'w-0', null, true);
    const buf = encodeLenField(3, shard); // field 3 = wrong for broadcast
    const out = decodeBroadcastShardGroupResponse(buf);
    assert.equal(out.shard_responses.length, 0, 'broadcast must read field 2, not 3');
  });
});

describe('decodeBarrierShardGroupResponse — same layout as broadcast', () => {
  it('decodes shard_responses from field 2', () => {
    const shard = buildShardQueryResponseProto(1, 'barrier-shard-1', null, true);
    const out = decodeBarrierShardGroupResponse(buildBroadcastResponseProto([shard], null));
    assert.equal(out.shard_responses.length, 1);
    assert.equal(out.shard_responses[0].shard_actor_id, 'barrier-shard-1');
  });
});

describe('decodeReduceShardGroupResponse — result=2, shard_responses=3', () => {
  it('decodes result payload and shard_responses', () => {
    const enc2 = new TextEncoder();
    // Build Message with payload at field 6
    const payload = enc2.encode('{"sum":10}');
    const msgBytes = encodeLenField(6, payload);
    let buf = encodeLenField(2, msgBytes); // result = field 2
    const shard = buildShardQueryResponseProto(0, 's-0', '{"v":5}', true);
    buf = concat(buf, encodeLenField(3, shard)); // shard_responses = field 3
    const out = decodeReduceShardGroupResponse(buf);
    assert.deepEqual(out.result, { sum: 10 });
    assert.equal(out.shard_responses.length, 1);
  });
});
