// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Protobuf wire encoding/decoding for shard group host functions.
// Matches the Rust prost-encoded protos in crates/wasm-runtime/src/simple_component_host.rs.
//
// Proto field numbers from actor_runtime.proto and common.proto.

import {
  appendLengthDelimited,
  appendVarint,
  concatBytes,
  readLengthDelimited,
  readVarint,
  skipField,
} from './proto-wire-common.js';

const enc = new TextDecoder('utf-8', { fatal: false });
const encW = new TextEncoder();

// ---------------------------------------------------------------------------
// Varint helpers
// ---------------------------------------------------------------------------

function appendString(buf: Uint8Array, fieldNum: number, s: string): Uint8Array {
  if (!s) return buf;
  return appendLengthDelimited(buf, fieldNum, new Uint8Array(encW.encode(s)));
}

function appendUint32(buf: Uint8Array, fieldNum: number, v: number): Uint8Array {
  if (v === 0) return buf;
  const tag = (fieldNum << 3) | 0; // varint wire type
  let b = appendVarint(buf, tag);
  b = appendVarint(b, v >>> 0);
  return b;
}

function appendBytes(buf: Uint8Array, fieldNum: number, data: Uint8Array): Uint8Array {
  if (data.length === 0) return buf;
  return appendLengthDelimited(buf, fieldNum, data);
}

function readString(data: Uint8Array, pos: number): { value: string; nextPos: number } {
  const { slice, nextPos } = readLengthDelimited(data, pos);
  return { value: enc.decode(slice), nextPos };
}

function readUint32(data: Uint8Array, pos: number): { value: number; nextPos: number } {
  const { value, n } = readVarint(data, pos);
  return { value: Number(value) & 0xffffffff, nextPos: pos + n };
}

// ---------------------------------------------------------------------------
// Enum helpers — map JS string names to proto int values
// ---------------------------------------------------------------------------

function partitionStrategyEnum(s: string | undefined): number {
  switch ((s ?? '').toLowerCase()) {
    case 'hash': return 1;
    case 'range': return 2;
    case 'consistent_hash': return 3;
    case 'custom': return 99;
    default: return 0;
  }
}

function rebalancePolicyEnum(s: string | undefined): number {
  switch ((s ?? '').toLowerCase()) {
    case 'none': return 1;
    case 'on_scale': return 2;
    case 'load_based': return 3;
    default: return 0;
  }
}

function nodePlacementStrategyEnum(s: string | undefined): number {
  switch ((s ?? '').toLowerCase()) {
    case 'same_node': return 1;
    case 'from_registry': return 2;
    case 'node_ids': return 3;
    default: return 0;
  }
}

function aggregationStrategyEnum(s: string | undefined): number {
  switch ((s ?? '').toLowerCase()) {
    case 'concat': return 1;
    case 'merge': return 2;
    case 'first': return 3;
    case 'majority': return 4;
    default: return 0;
  }
}

// ---------------------------------------------------------------------------
// NodePlacement encoder (DataParallelConfig.placement, field 6)
//
// message NodePlacement {
//   NodePlacementStrategy strategy = 1;  (varint)
//   string cluster = 2;
//   repeated string node_ids = 3;
// }
// ---------------------------------------------------------------------------

function encodeNodePlacement(placement: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);
  const strategy = nodePlacementStrategyEnum(placement.strategy as string);
  if (strategy !== 0) {
    buf = appendUint32(buf, 1, strategy);
  }
  const cluster = (placement.cluster as string) ?? '';
  buf = appendString(buf, 2, cluster);
  const nodeIds = placement.node_ids as string[] | undefined;
  if (Array.isArray(nodeIds)) {
    for (const n of nodeIds) {
      buf = appendString(buf, 3, n);
    }
  }
  return buf;
}

// ---------------------------------------------------------------------------
// DataParallelConfig encoder (CreateShardGroupRequest.config, field 1)
//
// message DataParallelConfig {
//   string group_id = 1;
//   uint32 shard_count = 2;
//   reserved 3;
//   PartitionStrategy partition_strategy = 4;  (varint)
//   RebalancePolicy rebalance_policy = 5;      (varint)
//   NodePlacement placement = 6;               (embedded)
// }
// ---------------------------------------------------------------------------

function encodeDataParallelConfig(cfg: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);
  buf = appendString(buf, 1, (cfg.group_id as string) ?? '');
  const shardCount = Number(cfg.shard_count ?? 0) >>> 0;
  if (shardCount > 0) buf = appendUint32(buf, 2, shardCount);
  const ps = partitionStrategyEnum(cfg.partition_strategy as string);
  if (ps !== 0) buf = appendUint32(buf, 4, ps);
  const rp = rebalancePolicyEnum(cfg.rebalance_policy as string);
  if (rp !== 0) buf = appendUint32(buf, 5, rp);
  const placement = cfg.placement as Record<string, unknown> | undefined;
  if (placement && typeof placement === 'object') {
    const placementBytes = encodeNodePlacement(placement);
    if (placementBytes.length > 0) {
      buf = appendLengthDelimited(buf, 6, placementBytes);
    }
  }
  return buf;
}

// ---------------------------------------------------------------------------
// Message encoder (ScatterGatherRequest.query, field 2)
//
// message Message {
//   string id = 1;
//   string sender_id = 2;
//   string receiver_id = 3;
//   string channel = 4;
//   string message_type = 5;
//   bytes payload = 6;    ← JSON-encoded query dict
// }
// ---------------------------------------------------------------------------

function ulid(): string {
  const t = Date.now();
  const chars = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let id = '';
  let ts = t;
  for (let i = 9; i >= 0; i--) {
    id = chars[ts % 32]! + id;
    ts = Math.floor(ts / 32);
  }
  for (let i = 0; i < 16; i++) id += chars[Math.floor(Math.random() * 32)]!;
  return id;
}

function encodeMessage(query: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 5, 'call');
  const payloadBytes = new Uint8Array(encW.encode(JSON.stringify(query)));
  buf = appendBytes(buf, 6, payloadBytes);
  return buf;
}

// ---------------------------------------------------------------------------
// CreateShardGroupRequest encoder
//
// message CreateShardGroupRequest {
//   DataParallelConfig config = 1;   (embedded)
//   string actor_type = 2;
//   ActorConfig shard_config = 3;    (not used here)
//   bytes initial_state = 4;
// }
// ---------------------------------------------------------------------------

export function encodeCreateShardGroupRequest(req: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);

  // config = DataParallelConfig (fields: group_id, shard_count, partition_strategy, rebalance_policy, placement)
  const cfgFields: Record<string, unknown> = {
    group_id: req.group_id,
    shard_count: req.shard_count,
    partition_strategy: req.partition_strategy,
    rebalance_policy: req.rebalance_policy,
    placement: req.placement,
  };
  const cfgBytes = encodeDataParallelConfig(cfgFields);
  buf = appendLengthDelimited(buf, 1, cfgBytes);

  buf = appendString(buf, 2, (req.actor_type as string) ?? '');

  const initialState = req.initial_state;
  if (initialState !== undefined && initialState !== null) {
    const stateBytes = new Uint8Array(encW.encode(JSON.stringify(initialState)));
    if (stateBytes.length > 0) {
      buf = appendBytes(buf, 4, stateBytes);
    }
  }

  return buf;
}

// ---------------------------------------------------------------------------
// ScatterGatherRequest encoder
//
// message ScatterGatherRequest {
//   string group_id = 1;
//   Message query = 2;                              (embedded)
//   google.protobuf.Duration timeout = 3;           (embedded)
//   ShardGroupAggregationStrategy aggregation = 4;  (varint)
//   uint32 min_responses = 5;
// }
//
// google.protobuf.Duration { int64 seconds=1, int32 nanos=2 }
// ---------------------------------------------------------------------------

function encodeDurationMs(ms: number): Uint8Array {
  const seconds = Math.floor(ms / 1000);
  const nanos = (ms % 1000) * 1_000_000;
  let buf: Uint8Array = new Uint8Array(0);
  if (seconds > 0) {
    // int64 seconds = field 1, wire type 0
    buf = appendVarint(buf, (1 << 3) | 0);
    buf = appendVarint(buf, seconds);
  }
  if (nanos > 0) {
    // int32 nanos = field 2, wire type 0
    buf = appendVarint(buf, (2 << 3) | 0);
    buf = appendVarint(buf, nanos);
  }
  return buf;
}

export function encodeScatterGatherRequest(req: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);

  buf = appendString(buf, 1, (req.group_id as string) ?? '');

  const query = req.query as Record<string, unknown> | undefined;
  if (query && typeof query === 'object') {
    const msgBytes = encodeMessage(query);
    buf = appendLengthDelimited(buf, 2, msgBytes);
  }

  const timeoutMs = Number(req.timeout_ms ?? 30000);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0) buf = appendLengthDelimited(buf, 3, durBytes);
  }

  const agg = aggregationStrategyEnum(req.aggregation as string);
  if (agg !== 0) buf = appendUint32(buf, 4, agg);

  const minResponses = Number(req.min_responses ?? 0) >>> 0;
  if (minResponses > 0) buf = appendUint32(buf, 5, minResponses);

  return buf;
}

// ---------------------------------------------------------------------------
// CreateShardGroupResponse decoder
//
// message CreateShardGroupResponse {
//   ShardGroup group = 1;
// }
// message ShardGroup {
//   DataParallelConfig config = 1;
//   string actor_type = 2;
//   repeated string shard_actor_ids = 3;
//   ShardGroupState state = 4;
// }
// ---------------------------------------------------------------------------

function decodeShardGroup(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    config: {},
    actor_type: '',
    shard_actor_ids: [] as string[],
    state: 0,
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      result.config = decodeDataParallelConfig(slice);
    } else if (fn === 2 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.actor_type = value;
    } else if (fn === 3 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      (result.shard_actor_ids as string[]).push(enc.decode(slice));
    } else if (fn === 4 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.state = value;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
}

function decodeDataParallelConfig(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    group_id: '',
    shard_count: 0,
    partition_strategy: 0,
    rebalance_policy: 0,
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.group_id = value;
    } else if (fn === 2 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.shard_count = value;
    } else if (fn === 4 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.partition_strategy = value;
    } else if (fn === 5 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.rebalance_policy = value;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
}

export function decodeCreateShardGroupResponse(data: Uint8Array): Record<string, unknown> {
  let pos = 0;
  let group: Record<string, unknown> = { shard_actor_ids: [] };
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      group = decodeShardGroup(slice);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { group };
}

// ---------------------------------------------------------------------------
// ScatterGatherResponse decoder
//
// message ScatterGatherResponse {
//   Message result = 1;
//   repeated ShardQueryResponse shard_responses = 2;
//   ScatterGatherStats stats = 3;
// }
// message ShardQueryResponse {
//   uint32 shard_id = 1;
//   string shard_actor_id = 2;
//   Message response = 3;
//   Duration latency = 4;
//   bool success = 5;
//   string error = 6;
// }
// message Message { ... bytes payload = 6; ... }
// ---------------------------------------------------------------------------

function decodeMessagePayload(data: Uint8Array): unknown {
  let pos = 0;
  let payloadBytes: Uint8Array | null = null;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 6 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      payloadBytes = slice;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  if (!payloadBytes || payloadBytes.length === 0) return {};
  const text = enc.decode(payloadBytes);
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function decodeShardQueryResponse(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    shard_id: 0,
    shard_actor_id: '',
    payload: {},
    success: false,
    error: '',
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.shard_id = value;
    } else if (fn === 2 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.shard_actor_id = value;
    } else if (fn === 3 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      // Expose the message payload directly so normalizeWorkerPayload can find it
      result.payload = decodeMessagePayload(slice);
    } else if (fn === 5 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.success = value !== 0;
    } else if (fn === 6 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.error = value;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
}

export function decodeScatterGatherResponse(data: Uint8Array): Record<string, unknown> {
  const shardResponses: Record<string, unknown>[] = [];
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 2 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      shardResponses.push(decodeShardQueryResponse(slice));
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { shard_responses: shardResponses };
}

// ---------------------------------------------------------------------------
// GetApplicationStatusResponse decoder
//
// message GetApplicationStatusResponse {
//   optional ApplicationInfo application = 1;
//   optional ApplicationRuntimeState state = 2;
//   optional string error = 3;
//   string node_id = 4;
//   string node_address = 5;
// }
// message ApplicationInfo {
//   string application_id = 1;
//   string name = 2;
//   ...
//   optional ApplicationMetrics metrics = 8;
// }
// message ApplicationMetrics {
//   map<string, uint64> actor_counts = 1;
//   ...
//   map<string, uint64> counter_metrics = 6;
//   map<string, uint64> latency_totals_ms = 7;
//   map<string, uint64> latency_max_ms = 8;
//   map<string, uint64> latency_samples = 9;
// }
// ---------------------------------------------------------------------------

function decodeUint64MapEntry(data: Uint8Array): { key: string; value: number } {
  let pos = 0;
  let key = '';
  let value = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      key = enc.decode(slice);
    } else if (fn === 2 && wt === 0) {
      const { value: v, n: m } = readVarint(data, pos);
      pos += m;
      value = Number(v);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { key, value };
}

function decodeUint64Map(entries: Uint8Array[]): Record<string, number> {
  const result: Record<string, number> = {};
  for (const entry of entries) {
    const { key, value } = decodeUint64MapEntry(entry);
    if (key) result[key] = value;
  }
  return result;
}

export function decodeApplicationMetrics(data: Uint8Array): Record<string, unknown> {
  const actorCountEntries: Uint8Array[] = [];
  const counterMetricEntries: Uint8Array[] = [];
  const latencyTotalsEntries: Uint8Array[] = [];
  const latencyMaxEntries: Uint8Array[] = [];
  const latencySamplesEntries: Uint8Array[] = [];
  let pos = 0;
  let messageCount = 0;
  let errorCount = 0;
  let uptimeSeconds = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      actorCountEntries.push(slice);
    } else if (fn === 3 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      uptimeSeconds = value;
    } else if (fn === 4 && wt === 0) {
      const { value: v, n: m } = readVarint(data, pos);
      pos += m;
      messageCount = Number(v);
    } else if (fn === 5 && wt === 0) {
      const { value: v, n: m } = readVarint(data, pos);
      pos += m;
      errorCount = Number(v);
    } else if (fn === 6 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      counterMetricEntries.push(slice);
    } else if (fn === 7 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      latencyTotalsEntries.push(slice);
    } else if (fn === 8 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      latencyMaxEntries.push(slice);
    } else if (fn === 9 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      latencySamplesEntries.push(slice);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return {
    actor_counts: decodeUint64Map(actorCountEntries),
    uptime_seconds: uptimeSeconds,
    message_count: messageCount,
    error_count: errorCount,
    counter_metrics: decodeUint64Map(counterMetricEntries),
    latency_totals_ms: decodeUint64Map(latencyTotalsEntries),
    latency_max_ms: decodeUint64Map(latencyMaxEntries),
    latency_samples: decodeUint64Map(latencySamplesEntries),
  };
}

function decodeApplicationInfo(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    application_id: '',
    name: '',
    metrics: null,
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.application_id = value;
    } else if (fn === 2 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.name = value;
    } else if (fn === 8 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      result.metrics = decodeApplicationMetrics(slice);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
}

export function decodeGetApplicationStatusResponse(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    application: null,
    node_id: '',
    node_address: '',
    error: null,
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      result.application = decodeApplicationInfo(slice);
    } else if (fn === 3 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.error = value;
    } else if (fn === 4 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.node_id = value;
    } else if (fn === 5 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.node_address = value;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
}

// ---------------------------------------------------------------------------
// ApplicationMetricsAdd encoder
//
// The host function hostApplicationMetricsAdd takes (applicationId: string, metrics: payload)
// where metrics is a protobuf-encoded ApplicationMetrics.
// ---------------------------------------------------------------------------

function appendUint64(buf: Uint8Array, fieldNum: number, v: number): Uint8Array {
  if (v === 0) return buf;
  const tag = (fieldNum << 3) | 0;
  let b = appendVarint(buf, tag);
  b = appendVarint(b, v);
  return b;
}

function encodeUint64MapEntry(key: string, value: number): Uint8Array {
  let entry: Uint8Array = new Uint8Array(0);
  entry = appendString(entry, 1, key);
  entry = appendUint64(entry, 2, value);
  return entry;
}

// ---------------------------------------------------------------------------
// BulkUpdateShardGroupRequest encoder
//
// BulkUpdateShardGroupRequest {
//   string request_id = 1;
//   string group_id = 2;
//   map<string, Message> updates = 3;  // partition_key -> message
//   ConsistencyLevel consistency_level = 4;
//   Duration timeout = 5;
//   bool wait_for_responses = 6;
// }
// ---------------------------------------------------------------------------
export function encodeBulkUpdateShardGroupRequest(req: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);

  buf = appendString(buf, 1, (req.request_id as string) ?? '');
  buf = appendString(buf, 2, (req.group_id as string) ?? '');

  const rawUpdates = req.updates;
  let items: Array<{ key: string; payload: Record<string, unknown> }>;
  if (Array.isArray(rawUpdates)) {
    items = rawUpdates as Array<{ key: string; payload: Record<string, unknown> }>;
  } else if (rawUpdates && typeof rawUpdates === 'object') {
    items = Object.entries(rawUpdates as Record<string, unknown>).map(([k, v]) => ({
      key: k,
      payload: v as Record<string, unknown>,
    }));
  } else {
    items = [];
  }

  for (const entry of items) {
    const partitionKey = String(entry.key ?? '');
    const payload = entry.payload && typeof entry.payload === 'object' ? entry.payload : {};
    const msgBytes = encodeMessage(payload as Record<string, unknown>);
    let mapEntry: Uint8Array = new Uint8Array(0);
    mapEntry = appendString(mapEntry, 1, partitionKey);
    mapEntry = appendLengthDelimited(mapEntry, 2, msgBytes);
    buf = appendLengthDelimited(buf, 3, mapEntry);
  }

  const consistencyLevel = Number(req.consistency_level ?? 0) >>> 0;
  if (consistencyLevel !== 0) buf = appendUint32(buf, 4, consistencyLevel);

  const timeoutMs = Number(req.timeout_ms ?? 5000);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0) buf = appendLengthDelimited(buf, 5, durBytes);
  }

  const waitForResponses = req.wait_for_responses !== false; // default true
  if (waitForResponses) {
    buf = appendUint32(buf, 6, 1);
  }

  return buf;
}

// ---------------------------------------------------------------------------
// BulkUpdateShardGroupResponse decoder
//
// BulkUpdateShardGroupResponse {
//   string request_id = 1;
//   uint32 updates_sent = 2;
//   uint32 updates_succeeded = 3;
//   uint32 updates_failed = 4;
//   repeated ShardUpdateStats shard_stats = 5;
//   repeated string errors = 6;
// }
// ---------------------------------------------------------------------------
export function decodeBulkUpdateShardGroupResponse(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    request_id: '',
    updates_sent: 0,
    updates_succeeded: 0,
    updates_failed: 0,
    shard_stats: [] as Record<string, unknown>[],
    errors: [] as string[],
  };
  if (!data || data.length === 0) return result;

  let pos = 0;
  while (pos < data.length) {
    const { value: tagVal, n: tagN } = readVarint(data, pos);
    pos += tagN;
    const fieldNum = Number(tagVal >> BigInt(3));
    const wireType = Number(tagVal & BigInt(7));

    if (fieldNum === 1 && wireType === 2) {
      const { value, nextPos } = readString(data, pos);
      result.request_id = value;
      pos = nextPos;
    } else if (fieldNum === 2 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_sent = value;
      pos = nextPos;
    } else if (fieldNum === 3 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_succeeded = value;
      pos = nextPos;
    } else if (fieldNum === 4 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_failed = value;
      pos = nextPos;
    } else if (fieldNum === 5 && wireType === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      (result.shard_stats as Record<string, unknown>[]).push(decodeShardUpdateStats(slice));
      pos = nextPos;
    } else if (fieldNum === 6 && wireType === 2) {
      const { value, nextPos } = readString(data, pos);
      (result.errors as string[]).push(value);
      pos = nextPos;
    } else {
      pos = skipField(data, pos, wireType);
    }
  }
  return result;
}

function decodeShardUpdateStats(data: Uint8Array): Record<string, unknown> {
  const result: Record<string, unknown> = {
    shard_id: 0, shard_actor_id: '', updates_sent: 0, updates_succeeded: 0, updates_failed: 0,
  };
  if (!data || data.length === 0) return result;
  let pos = 0;
  while (pos < data.length) {
    const { value: tagVal, n: tagN } = readVarint(data, pos);
    pos += tagN;
    const fieldNum = Number(tagVal >> BigInt(3));
    const wireType = Number(tagVal & BigInt(7));
    if (fieldNum === 1 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos); result.shard_id = value; pos = nextPos;
    } else if (fieldNum === 2 && wireType === 2) {
      const { value, nextPos } = readString(data, pos); result.shard_actor_id = value; pos = nextPos;
    } else if (fieldNum === 3 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos); result.updates_sent = value; pos = nextPos;
    } else if (fieldNum === 4 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos); result.updates_succeeded = value; pos = nextPos;
    } else if (fieldNum === 5 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos); result.updates_failed = value; pos = nextPos;
    } else {
      pos = skipField(data, pos, wireType);
    }
  }
  return result;
}

export function encodeApplicationMetrics(metrics: Record<string, unknown>): Uint8Array {
  let buf: Uint8Array = new Uint8Array(0);

  const counterMetrics = metrics.counter_metrics as Record<string, number> | undefined;
  if (counterMetrics && typeof counterMetrics === 'object') {
    for (const [key, value] of Object.entries(counterMetrics)) {
      const entry = encodeUint64MapEntry(key, Number(value));
      buf = appendLengthDelimited(buf, 6, entry);
    }
  }

  const latencyTotals = metrics.latency_totals_ms as Record<string, number> | undefined;
  if (latencyTotals && typeof latencyTotals === 'object') {
    for (const [key, value] of Object.entries(latencyTotals)) {
      const entry = encodeUint64MapEntry(key, Number(value));
      buf = appendLengthDelimited(buf, 7, entry);
    }
  }

  const latencyMax = metrics.latency_max_ms as Record<string, number> | undefined;
  if (latencyMax && typeof latencyMax === 'object') {
    for (const [key, value] of Object.entries(latencyMax)) {
      const entry = encodeUint64MapEntry(key, Number(value));
      buf = appendLengthDelimited(buf, 8, entry);
    }
  }

  const latencySamples = metrics.latency_samples as Record<string, number> | undefined;
  if (latencySamples && typeof latencySamples === 'object') {
    for (const [key, value] of Object.entries(latencySamples)) {
      const entry = encodeUint64MapEntry(key, Number(value));
      buf = appendLengthDelimited(buf, 9, entry);
    }
  }

  return buf;
}
