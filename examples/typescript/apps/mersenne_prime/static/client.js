"use strict";
(() => {
  // wit-stub:plexspaces:actor/host-logging@0.1.0
  var _noop = () => void 0;
  var log = _noop;
  var nowMs = _noop;

  // ../../../../sdks/typescript/dist/decorators.js
  var ACTOR_METADATA = Symbol.for("plexspaces.actor.metadata");

  // ../../../../sdks/typescript/dist/wit-payload.js
  function decodeWitPayloadUtf8(input) {
    if (typeof input === "string") {
      return input;
    }
    if (input instanceof ArrayBuffer) {
      return new TextDecoder("utf-8", { fatal: false }).decode(new Uint8Array(input));
    }
    if (ArrayBuffer.isView(input)) {
      const v = input;
      return new TextDecoder("utf-8", { fatal: false }).decode(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
    }
    return "";
  }
  function encodeWitPayloadUtf8(text) {
    return new TextEncoder().encode(text);
  }

  // ../../../../sdks/typescript/dist/actor.js
  var CHAR_QUOTE = 34;
  var CHAR_BACKSLASH = 92;
  var CHAR_NEWLINE = 10;
  var CHAR_CR = 13;
  var CHAR_TAB = 9;
  var CHAR_SPACE = 32;
  var ESCAPE_TABLE = [];
  for (let i = 0; i < 128; i++) {
    if (i === CHAR_QUOTE)
      ESCAPE_TABLE[i] = '\\"';
    else if (i === CHAR_BACKSLASH)
      ESCAPE_TABLE[i] = "\\\\";
    else if (i === CHAR_NEWLINE)
      ESCAPE_TABLE[i] = "\\n";
    else if (i === CHAR_CR)
      ESCAPE_TABLE[i] = "\\r";
    else if (i === CHAR_TAB)
      ESCAPE_TABLE[i] = "\\t";
    else if (i < CHAR_SPACE) {
      const h1 = i >> 4 & 15;
      const h0 = i & 15;
      ESCAPE_TABLE[i] = "\\u00" + String.fromCharCode(h1 < 10 ? 48 + h1 : 87 + h1) + String.fromCharCode(h0 < 10 ? 48 + h0 : 87 + h0);
    } else {
      ESCAPE_TABLE[i] = "";
    }
  }

  // ../../../../sdks/typescript/dist/wire/proto-wire-common.js
  function appendVarint(buf, xIn) {
    if (!Number.isFinite(xIn) || xIn < 0 || xIn > Number.MAX_SAFE_INTEGER) {
      throw new Error("appendVarint expects a non-negative safe integer");
    }
    let n = BigInt(Math.floor(xIn));
    const parts = [];
    while (n >= 0x80n) {
      parts.push(Number(n & 0xffn) | 128);
      n >>= 7n;
    }
    parts.push(Number(n));
    return concatBytes(buf, new Uint8Array(parts));
  }
  function appendLengthDelimited(buf, fieldNum, inner) {
    const tag = BigInt(fieldNum << 3 | 2);
    let b = appendVarint(buf, Number(tag));
    b = appendVarint(b, inner.length);
    return concatBytes(b, inner);
  }
  function concatBytes(a, b) {
    const out = new Uint8Array(a.length + b.length);
    out.set(a, 0);
    out.set(b, a.length);
    return out;
  }
  function readVarint(data, pos) {
    let x = 0n;
    let s = 0n;
    const orig = pos;
    for (let i = 0; i < 10; i++) {
      if (pos >= data.length)
        throw new Error("varint buffer underflow");
      const b = data[pos];
      pos++;
      if (b < 128) {
        return { value: x | BigInt(b) << s, n: pos - orig };
      }
      x |= BigInt(b & 127) << s;
      s += 7n;
    }
    throw new Error("varint too long");
  }
  function skipField(data, pos, wireType) {
    switch (wireType) {
      case 0: {
        const { n } = readVarint(data, pos);
        return pos + n;
      }
      case 1:
        if (pos + 8 > data.length)
          throw new Error("fixed64 underflow");
        return pos + 8;
      case 2: {
        const { value: ln, n } = readVarint(data, pos);
        return pos + n + Number(ln);
      }
      case 5:
        if (pos + 4 > data.length)
          throw new Error("fixed32 underflow");
        return pos + 4;
      default:
        throw new Error(`unknown wire type ${wireType}`);
    }
  }
  function readLengthDelimited(data, pos) {
    const { value: ln, n } = readVarint(data, pos);
    const start = pos + n;
    const end = start + Number(ln);
    if (end > data.length)
      throw new Error("length-delimited field truncated");
    const copy = new Uint8Array(end - start);
    copy.set(data.subarray(start, end));
    return { slice: copy, nextPos: end };
  }

  // ../../../../sdks/typescript/dist/wire/tuplespace-proto-wire.js
  var MIN_INT64 = -9223372036854775808n;
  var MAX_INT64 = 9223372036854775807n;
  function encodeTupleField(v, allowWildcardStar) {
    if (v === null || v === void 0) {
      return appendVarint(new Uint8Array([56]), 1);
    }
    if (typeof v === "string") {
      if (allowWildcardStar && v === "*") {
        return appendVarint(new Uint8Array([56]), 1);
      }
      const enc3 = new TextEncoder();
      const bytes2 = new Uint8Array(enc3.encode(v));
      let inner = new Uint8Array([26]);
      inner = appendVarint(inner, bytes2.length);
      inner = concatBytes(inner, bytes2);
      return inner;
    }
    if (typeof v === "boolean") {
      const inner = new Uint8Array([32]);
      return appendVarint(inner, v ? 1 : 0);
    }
    if (typeof v === "number" && Number.isFinite(v)) {
      const t = Math.trunc(v);
      if (t === v && t >= Number(MIN_INT64) && t <= Number(MAX_INT64)) {
        let inner2 = new Uint8Array([8]);
        inner2 = appendVarintSigned(inner2, t);
        return inner2;
      }
      let inner = new Uint8Array([17]);
      const tmp = new Uint8Array(8);
      new DataView(tmp.buffer).setFloat64(0, v, true);
      inner = concatBytes(inner, tmp);
      return inner;
    }
    throw new Error(`unsupported tuple field type ${typeof v}`);
  }
  function appendVarintSigned(buf, xIn) {
    let x = BigInt(xIn);
    if (x < 0n)
      x = BigInt.asUintN(64, x);
    const parts = [];
    let n = x;
    while (n >= 0x80n) {
      parts.push(Number(n & 0xffn) | 128);
      n >>= 7n;
    }
    parts.push(Number(n));
    return concatBytes(buf, new Uint8Array(parts));
  }
  function encodeTupleFields(tuple, allowWildcardStar) {
    let out = new Uint8Array(0);
    for (const el of tuple) {
      const tf = encodeTupleField(el, allowWildcardStar);
      out = appendLengthDelimited(out, 2, tf);
    }
    return out;
  }
  function encodeWriteRequest(tuple) {
    const tupleBody = encodeTupleFields(tuple, false);
    return appendLengthDelimited(new Uint8Array(0), 1, tupleBody);
  }
  function encodeReadRequest(pattern, take, maxResults) {
    const templateBody = encodeTupleFields(pattern, true);
    let out = appendLengthDelimited(new Uint8Array(0), 1, templateBody);
    if (take) {
      out = concatBytes(out, new Uint8Array([32, 1]));
    }
    out = concatBytes(out, new Uint8Array([40]));
    out = appendVarint(out, maxResults >>> 0);
    return out;
  }
  function parseTupleFieldMsg(msg) {
    let pos = 0;
    let last = void 0;
    while (pos < msg.length) {
      const { value: tag, n: tn } = readVarint(msg, pos);
      pos += tn;
      const fn = Number(tag >> 3n);
      const wt = Number(tag & 7n);
      if (wt === 0) {
        const { value: v, n: m } = readVarint(msg, pos);
        pos += m;
        if (fn === 1)
          last = Number(v);
        else if (fn === 4)
          last = v !== 0n;
        else if (fn === 6 || fn === 7)
          last = null;
      } else if (wt === 1) {
        if (pos + 8 > msg.length)
          throw new Error("double underflow");
        const view = new DataView(msg.buffer, msg.byteOffset + pos, 8);
        const d = view.getFloat64(0, true);
        pos += 8;
        if (fn === 2)
          last = d;
      } else if (wt === 2) {
        const { slice: chunk, nextPos } = readLengthDelimited(msg, pos);
        pos = nextPos;
        if (fn === 3 || fn === 5) {
          last = new TextDecoder("utf-8", { fatal: false }).decode(chunk);
        }
      } else {
        pos = skipField(msg, pos, wt);
      }
    }
    return last;
  }
  function parseTupleMsg(msg) {
    const fields = [];
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
  function parseReadResponseTuples(data) {
    const tuples = [];
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
  function decodeReadResponseFirstTuple(raw) {
    if (raw.length === 0)
      return null;
    try {
      const tuples = parseReadResponseTuples(raw);
      if (tuples.length === 0)
        return null;
      return tuples[0] ?? null;
    } catch {
      return null;
    }
  }
  function decodeReadResponseAllTuples(raw) {
    if (raw.length === 0)
      return [];
    try {
      return parseReadResponseTuples(raw);
    } catch {
      return [];
    }
  }

  // ../../../../sdks/typescript/dist/wire/http-fetch-proto-wire.js
  function utf8Valid(bytes2) {
    try {
      new TextDecoder("utf-8", { fatal: true }).decode(bytes2);
      return true;
    } catch {
      return false;
    }
  }
  function bytesToBase64Sync(bytes2) {
    let bin = "";
    for (let i = 0; i < bytes2.length; i++)
      bin += String.fromCharCode(bytes2[i]);
    if (typeof btoa !== "undefined")
      return btoa(bin);
    const Buf = globalThis.Buffer;
    if (Buf)
      return Buf.from(bytes2).toString("base64");
    throw new Error("base64 encode unavailable");
  }
  function encodeHttpFetchRequestWire(headers, body) {
    let buf = new Uint8Array(0);
    const enc3 = new TextEncoder();
    for (const [k, v] of Object.entries(headers)) {
      const kb = new Uint8Array(enc3.encode(k));
      const vb = new Uint8Array(enc3.encode(v));
      let entry = appendLengthDelimited(new Uint8Array(0), 1, kb);
      entry = appendLengthDelimited(entry, 2, vb);
      buf = appendLengthDelimited(buf, 1, entry);
    }
    const bodyUse = body && body.length > 0 ? new Uint8Array(body) : new Uint8Array(0);
    buf = appendLengthDelimited(buf, 2, bodyUse);
    return buf;
  }
  function parseStringStringMapEntry(entry) {
    let pos = 0;
    let key = "";
    let val = "";
    while (pos < entry.length) {
      const { value: tag, n: tn } = readVarint(entry, pos);
      pos += tn;
      const fn = Number(tag >> 3n);
      const wt = Number(tag & 7n);
      if (fn === 1 && wt === 2) {
        const { slice, nextPos } = readLengthDelimited(entry, pos);
        pos = nextPos;
        key = new TextDecoder("utf-8", { fatal: false }).decode(slice);
      } else if (fn === 2 && wt === 2) {
        const { slice, nextPos } = readLengthDelimited(entry, pos);
        pos = nextPos;
        val = new TextDecoder("utf-8", { fatal: false }).decode(slice);
      } else {
        pos = skipField(entry, pos, wt);
      }
    }
    return { key, val };
  }
  function decodeHttpFetchResponseWire(data) {
    const out = {
      status: 0,
      headers: {},
      body: ""
    };
    let pos = 0;
    while (pos < data.length) {
      const { value: tag, n: tn } = readVarint(data, pos);
      pos += tn;
      const fn = Number(tag >> 3n);
      const wt = Number(tag & 7n);
      if (fn === 1 && wt === 0) {
        const { value: v, n: m } = readVarint(data, pos);
        pos += m;
        out.status = Number(v);
      } else if (fn === 2 && wt === 2) {
        const { slice: sl, nextPos } = readLengthDelimited(data, pos);
        pos = nextPos;
        const { key, val } = parseStringStringMapEntry(sl);
        if (key)
          out.headers[key] = val;
      } else if (fn === 3 && wt === 2) {
        const { slice: sl, nextPos } = readLengthDelimited(data, pos);
        pos = nextPos;
        out.body = utf8Valid(sl) ? new TextDecoder("utf-8", { fatal: false }).decode(sl) : bytesToBase64Sync(sl);
      } else {
        pos = skipField(data, pos, wt);
      }
    }
    return out;
  }

  // ../../../../sdks/typescript/dist/wire/shard-group-proto-wire.js
  var enc = new TextDecoder("utf-8", { fatal: false });
  var encW = new TextEncoder();
  function appendString(buf, fieldNum, s) {
    if (!s)
      return buf;
    return appendLengthDelimited(buf, fieldNum, new Uint8Array(encW.encode(s)));
  }
  function appendUint32(buf, fieldNum, v) {
    if (v === 0)
      return buf;
    const tag = fieldNum << 3 | 0;
    let b = appendVarint(buf, tag);
    b = appendVarint(b, v >>> 0);
    return b;
  }
  function appendBytes(buf, fieldNum, data) {
    if (data.length === 0)
      return buf;
    return appendLengthDelimited(buf, fieldNum, data);
  }
  function readString(data, pos) {
    const { slice, nextPos } = readLengthDelimited(data, pos);
    return { value: enc.decode(slice), nextPos };
  }
  function readUint32(data, pos) {
    const { value, n } = readVarint(data, pos);
    return { value: Number(value) & 4294967295, nextPos: pos + n };
  }
  function partitionStrategyEnum(s) {
    switch ((s ?? "").toLowerCase()) {
      case "hash":
        return 1;
      case "range":
        return 2;
      case "consistent_hash":
        return 3;
      case "custom":
        return 99;
      default:
        return 0;
    }
  }
  function rebalancePolicyEnum(s) {
    switch ((s ?? "").toLowerCase()) {
      case "none":
        return 1;
      case "on_scale":
        return 2;
      case "load_based":
        return 3;
      default:
        return 0;
    }
  }
  function nodePlacementStrategyEnum(s) {
    switch ((s ?? "").toLowerCase()) {
      case "same_node":
        return 1;
      case "from_registry":
        return 2;
      case "node_ids":
        return 3;
      default:
        return 0;
    }
  }
  function aggregationStrategyEnum(s) {
    switch ((s ?? "").toLowerCase()) {
      case "concat":
        return 1;
      case "merge":
        return 2;
      case "first":
        return 3;
      case "majority":
        return 4;
      default:
        return 0;
    }
  }
  function encodeNodePlacement(placement) {
    let buf = new Uint8Array(0);
    const strategy = nodePlacementStrategyEnum(placement.strategy);
    if (strategy !== 0) {
      buf = appendUint32(buf, 1, strategy);
    }
    const cluster = placement.cluster ?? "";
    buf = appendString(buf, 2, cluster);
    const nodeIds = placement.node_ids;
    if (Array.isArray(nodeIds)) {
      for (const n of nodeIds) {
        buf = appendString(buf, 3, n);
      }
    }
    return buf;
  }
  function encodeDataParallelConfig(cfg) {
    let buf = new Uint8Array(0);
    buf = appendString(buf, 1, cfg.group_id ?? "");
    const shardCount = Number(cfg.shard_count ?? 0) >>> 0;
    if (shardCount > 0)
      buf = appendUint32(buf, 2, shardCount);
    const ps = partitionStrategyEnum(cfg.partition_strategy);
    if (ps !== 0)
      buf = appendUint32(buf, 4, ps);
    const rp = rebalancePolicyEnum(cfg.rebalance_policy);
    if (rp !== 0)
      buf = appendUint32(buf, 5, rp);
    const placement = cfg.placement;
    if (placement && typeof placement === "object") {
      const placementBytes = encodeNodePlacement(placement);
      if (placementBytes.length > 0) {
        buf = appendLengthDelimited(buf, 6, placementBytes);
      }
    }
    return buf;
  }
  function ulid() {
    const t = Date.now();
    const chars = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let id = "";
    let ts = t;
    for (let i = 9; i >= 0; i--) {
      id = chars[ts % 32] + id;
      ts = Math.floor(ts / 32);
    }
    for (let i = 0; i < 16; i++)
      id += chars[Math.floor(Math.random() * 32)];
    return id;
  }
  function encodeMessage(query) {
    let buf = new Uint8Array(0);
    buf = appendString(buf, 1, ulid());
    buf = appendString(buf, 5, "call");
    const payloadBytes = new Uint8Array(encW.encode(JSON.stringify(query)));
    buf = appendBytes(buf, 6, payloadBytes);
    return buf;
  }
  function encodeCreateShardGroupRequest(req) {
    let buf = new Uint8Array(0);
    const cfgFields = {
      group_id: req.group_id,
      shard_count: req.shard_count,
      partition_strategy: req.partition_strategy,
      rebalance_policy: req.rebalance_policy,
      placement: req.placement
    };
    const cfgBytes = encodeDataParallelConfig(cfgFields);
    buf = appendLengthDelimited(buf, 1, cfgBytes);
    buf = appendString(buf, 2, req.actor_type ?? "");
    const initialState = req.initial_state;
    if (initialState !== void 0 && initialState !== null) {
      const stateBytes = new Uint8Array(encW.encode(JSON.stringify(initialState)));
      if (stateBytes.length > 0) {
        buf = appendBytes(buf, 4, stateBytes);
      }
    }
    return buf;
  }
  function encodeDurationMs(ms) {
    const seconds = Math.floor(ms / 1e3);
    const nanos = ms % 1e3 * 1e6;
    let buf = new Uint8Array(0);
    if (seconds > 0) {
      buf = appendVarint(buf, 1 << 3 | 0);
      buf = appendVarint(buf, seconds);
    }
    if (nanos > 0) {
      buf = appendVarint(buf, 2 << 3 | 0);
      buf = appendVarint(buf, nanos);
    }
    return buf;
  }
  function encodeScatterGatherRequest(req) {
    let buf = new Uint8Array(0);
    buf = appendString(buf, 1, req.group_id ?? "");
    const query = req.query;
    if (query && typeof query === "object") {
      const msgBytes = encodeMessage(query);
      buf = appendLengthDelimited(buf, 2, msgBytes);
    }
    const timeoutMs = Number(req.timeout_ms ?? 3e4);
    if (timeoutMs > 0) {
      const durBytes = encodeDurationMs(timeoutMs);
      if (durBytes.length > 0)
        buf = appendLengthDelimited(buf, 3, durBytes);
    }
    const agg = aggregationStrategyEnum(req.aggregation);
    if (agg !== 0)
      buf = appendUint32(buf, 4, agg);
    const minResponses = Number(req.min_responses ?? 0) >>> 0;
    if (minResponses > 0)
      buf = appendUint32(buf, 5, minResponses);
    return buf;
  }
  function decodeShardGroup(data) {
    const result = {
      config: {},
      actor_type: "",
      shard_actor_ids: [],
      state: 0
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
        result.shard_actor_ids.push(enc.decode(slice));
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
  function decodeDataParallelConfig(data) {
    const result = {
      group_id: "",
      shard_count: 0,
      partition_strategy: 0,
      rebalance_policy: 0
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
  function decodeCreateShardGroupResponse(data) {
    let pos = 0;
    let group = { shard_actor_ids: [] };
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
  function decodeMessagePayload(data) {
    let pos = 0;
    let payloadBytes = null;
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
    if (!payloadBytes || payloadBytes.length === 0)
      return {};
    const text = enc.decode(payloadBytes);
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  }
  function decodeShardQueryResponse(data) {
    const result = {
      shard_id: 0,
      shard_actor_id: "",
      payload: {},
      success: false,
      error: ""
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
  function decodeScatterGatherResponse(data) {
    const shardResponses = [];
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
  function decodeUint64MapEntry(data) {
    let pos = 0;
    let key = "";
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
  function decodeUint64Map(entries) {
    const result = {};
    for (const entry of entries) {
      const { key, value } = decodeUint64MapEntry(entry);
      if (key)
        result[key] = value;
    }
    return result;
  }
  function decodeApplicationMetrics(data) {
    const actorCountEntries = [];
    const counterMetricEntries = [];
    const latencyTotalsEntries = [];
    const latencyMaxEntries = [];
    const latencySamplesEntries = [];
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
      latency_samples: decodeUint64Map(latencySamplesEntries)
    };
  }
  function decodeApplicationInfo(data) {
    const result = {
      application_id: "",
      name: "",
      metrics: null
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
  function decodeGetApplicationStatusResponse(data) {
    const result = {
      application: null,
      node_id: "",
      node_address: "",
      error: null
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
  function appendUint64(buf, fieldNum, v) {
    if (v === 0)
      return buf;
    const tag = fieldNum << 3 | 0;
    let b = appendVarint(buf, tag);
    b = appendVarint(b, v);
    return b;
  }
  function encodeUint64MapEntry(key, value) {
    let entry = new Uint8Array(0);
    entry = appendString(entry, 1, key);
    entry = appendUint64(entry, 2, value);
    return entry;
  }
  function encodeApplicationMetrics(metrics) {
    let buf = new Uint8Array(0);
    const counterMetrics = metrics.counter_metrics;
    if (counterMetrics && typeof counterMetrics === "object") {
      for (const [key, value] of Object.entries(counterMetrics)) {
        const entry = encodeUint64MapEntry(key, Number(value));
        buf = appendLengthDelimited(buf, 6, entry);
      }
    }
    const latencyTotals = metrics.latency_totals_ms;
    if (latencyTotals && typeof latencyTotals === "object") {
      for (const [key, value] of Object.entries(latencyTotals)) {
        const entry = encodeUint64MapEntry(key, Number(value));
        buf = appendLengthDelimited(buf, 7, entry);
      }
    }
    const latencyMax = metrics.latency_max_ms;
    if (latencyMax && typeof latencyMax === "object") {
      for (const [key, value] of Object.entries(latencyMax)) {
        const entry = encodeUint64MapEntry(key, Number(value));
        buf = appendLengthDelimited(buf, 8, entry);
      }
    }
    const latencySamples = metrics.latency_samples;
    if (latencySamples && typeof latencySamples === "object") {
      for (const [key, value] of Object.entries(latencySamples)) {
        const entry = encodeUint64MapEntry(key, Number(value));
        buf = appendLengthDelimited(buf, 9, entry);
      }
    }
    return buf;
  }

  // ../../../../sdks/typescript/dist/wire/registry-proto-wire.js
  var enc2 = new TextEncoder();
  var dec = new TextDecoder();
  function appendStringField(buf, fieldNum, s) {
    if (!s)
      return buf;
    const encoded = enc2.encode(s);
    const bytes2 = new Uint8Array(encoded.length);
    bytes2.set(encoded);
    const tag = fieldNum << 3 | 2;
    let b = appendVarint(buf, tag);
    b = appendVarint(b, bytes2.length);
    return concatBytes(b, bytes2);
  }
  function appendVarintField(buf, fieldNum, v) {
    const tag = fieldNum << 3;
    let b = appendVarint(buf, tag);
    return appendVarint(b, v);
  }
  function objectTypeToString(n) {
    switch (n) {
      case 1:
        return "actor";
      case 2:
        return "tuplespace";
      case 3:
        return "service";
      case 4:
        return "vm";
      case 5:
        return "application";
      case 6:
        return "workflow";
      case 7:
        return "node";
      case 8:
        return "process_group";
      default:
        return "";
    }
  }
  function objectTypeFromString(s) {
    switch (s) {
      case "actor":
        return 1;
      case "tuplespace":
        return 2;
      case "service":
        return 3;
      case "vm":
        return 4;
      case "application":
        return 5;
      case "workflow":
        return 6;
      case "node":
        return 7;
      case "process_group":
        return 8;
      default:
        return 0;
    }
  }
  function encodeObjectRegistration(reg) {
    let b = new Uint8Array(0);
    b = appendStringField(b, 1, reg.objectId);
    const ot = objectTypeFromString(reg.objectType);
    if (ot !== 0)
      b = appendVarintField(b, 3, ot);
    if (reg.grpcAddress)
      b = appendStringField(b, 8, reg.grpcAddress);
    if (reg.objectCategory)
      b = appendStringField(b, 9, reg.objectCategory);
    if (reg.tenantId)
      b = appendStringField(b, 5, reg.tenantId);
    if (reg.namespace)
      b = appendStringField(b, 6, reg.namespace);
    for (const cap of reg.capabilities ?? [])
      b = appendStringField(b, 10, cap);
    for (const lbl of reg.labels ?? [])
      b = appendStringField(b, 13, lbl);
    if (reg.alias)
      b = appendStringField(b, 18, reg.alias);
    return b;
  }
  function encodeRegisterRequest(reg) {
    const inner = encodeObjectRegistration(reg);
    return appendLengthDelimited(new Uint8Array(0), 1, inner);
  }
  function encodeUnregisterRequest(objectId, objectType, tenantId, namespace) {
    let b = new Uint8Array(0);
    b = appendStringField(b, 1, objectId);
    if (objectType !== 0)
      b = appendVarintField(b, 2, objectType);
    if (tenantId)
      b = appendStringField(b, 3, tenantId);
    if (namespace)
      b = appendStringField(b, 4, namespace);
    return b;
  }
  function encodeLookupRequest(objectId, objectType, tenantId, namespace, alias) {
    let b = new Uint8Array(0);
    if (objectId)
      b = appendStringField(b, 1, objectId);
    if (objectType !== 0)
      b = appendVarintField(b, 2, objectType);
    if (tenantId)
      b = appendStringField(b, 3, tenantId);
    if (namespace)
      b = appendStringField(b, 4, namespace);
    if (alias)
      b = appendStringField(b, 5, alias);
    return b;
  }
  function encodeDiscoverRequest(opts) {
    let b = new Uint8Array(0);
    if (opts.objectType)
      b = appendVarintField(b, 1, opts.objectType);
    if (opts.objectCategory)
      b = appendStringField(b, 2, opts.objectCategory);
    if (opts.tenantId)
      b = appendStringField(b, 4, opts.tenantId);
    if (opts.namespace)
      b = appendStringField(b, 5, opts.namespace);
    for (const cap of opts.capabilities ?? [])
      b = appendStringField(b, 6, cap);
    for (const lbl of opts.labels ?? [])
      b = appendStringField(b, 7, lbl);
    if (opts.pageSize && opts.pageSize > 0)
      b = appendVarintField(b, 10, opts.pageSize);
    return b;
  }
  function encodeHeartbeatRequest(objectId, objectType, tenantId, namespace) {
    let b = new Uint8Array(0);
    b = appendStringField(b, 1, objectId);
    if (objectType !== 0)
      b = appendVarintField(b, 2, objectType);
    if (tenantId)
      b = appendStringField(b, 3, tenantId);
    if (namespace)
      b = appendStringField(b, 4, namespace);
    return b;
  }
  function decodeObjectRegistration(data) {
    const reg = { objectId: "", objectType: "" };
    let pos = 0;
    while (pos < data.length) {
      const { value: tagVal, n } = readVarint(data, pos);
      pos += n;
      const fn_ = Number(tagVal >> 3n);
      const wt = Number(tagVal & 7n);
      if (wt === 2) {
        const { value: ln, n: m } = readVarint(data, pos);
        pos += m;
        const end = pos + Number(ln);
        const chunk = data.slice(pos, end);
        pos = end;
        const str2 = dec.decode(chunk);
        switch (fn_) {
          case 1:
            reg.objectId = str2;
            break;
          case 5:
            reg.tenantId = str2;
            break;
          case 6:
            reg.namespace = str2;
            break;
          case 8:
            reg.grpcAddress = str2;
            break;
          case 9:
            reg.objectCategory = str2;
            break;
          case 10:
            (reg.capabilities ?? (reg.capabilities = [])).push(str2);
            break;
          case 13:
            (reg.labels ?? (reg.labels = [])).push(str2);
            break;
          case 18:
            reg.alias = str2;
            break;
        }
      } else if (wt === 0) {
        const { value: v, n: m } = readVarint(data, pos);
        pos += m;
        if (fn_ === 3)
          reg.objectType = objectTypeToString(Number(v));
      } else {
        pos = skipField(data, pos, wt);
      }
    }
    return reg;
  }
  function decodeLookupResponse(data) {
    let pos = 0;
    let regBytes = null;
    let found = false;
    while (pos < data.length) {
      const { value: tagVal, n } = readVarint(data, pos);
      pos += n;
      const fn_ = Number(tagVal >> 3n);
      const wt = Number(tagVal & 7n);
      if (fn_ === 1 && wt === 2) {
        const { value: ln, n: m } = readVarint(data, pos);
        pos += m;
        regBytes = data.slice(pos, pos + Number(ln));
        pos += Number(ln);
      } else if (fn_ === 2 && wt === 0) {
        const { value: v, n: m } = readVarint(data, pos);
        pos += m;
        found = v !== 0n;
      } else {
        pos = skipField(data, pos, wt);
      }
    }
    if (!found || !regBytes)
      return null;
    return decodeObjectRegistration(regBytes);
  }
  function decodeDiscoverResponse(data) {
    const results = [];
    let pos = 0;
    while (pos < data.length) {
      const { value: tagVal, n } = readVarint(data, pos);
      pos += n;
      const fn_ = Number(tagVal >> 3n);
      const wt = Number(tagVal & 7n);
      if (fn_ === 1 && wt === 2) {
        const { value: ln, n: m } = readVarint(data, pos);
        pos += m;
        const regBytes = data.slice(pos, pos + Number(ln));
        pos += Number(ln);
        results.push(decodeObjectRegistration(regBytes));
      } else {
        pos = skipField(data, pos, wt);
      }
    }
    return results;
  }

  // ../../../../sdks/typescript/dist/process_groups.js
  function firstGroupMember(members) {
    return members.length > 0 ? members[0] : null;
  }
  function firstGroupMemberOrThrow(group, members) {
    const first = firstGroupMember(members);
    if (first === null) {
      throw new Error(`no members in process group '${group}'`);
    }
    return first;
  }

  // wit-stub:plexspaces:actor/host-actor@0.1.0
  var _noop2 = () => void 0;
  var send = _noop2;
  var ask = _noop2;
  var selfId = _noop2;
  var spawn = _noop2;
  var stop = _noop2;
  var link = _noop2;
  var unlink = _noop2;
  var monitor = _noop2;
  var demonitor = _noop2;
  var sendAfter = _noop2;
  var pgJoin = _noop2;
  var pgLeave = _noop2;
  var pgMembers = _noop2;
  var pgBroadcast = _noop2;

  // wit-stub:plexspaces:actor/host-kv@0.1.0
  var _noop3 = () => void 0;
  var kvGet = _noop3;
  var kvPut = _noop3;
  var kvDelete = _noop3;
  var kvList = _noop3;
  var kvPutWithTtl = _noop3;
  var kvGetTtl = _noop3;
  var kvCas = _noop3;
  var kvIncrement = _noop3;
  var kvMultiGet = _noop3;
  var kvMultiPut = _noop3;
  var alarmSet = _noop3;
  var alarmGet = _noop3;
  var alarmDelete = _noop3;

  // wit-stub:plexspaces:actor/host-ts@0.1.0
  var _noop4 = () => void 0;
  var tsWrite = _noop4;
  var tsRead = _noop4;
  var tsTake = _noop4;
  var tsReadAll = _noop4;

  // wit-stub:plexspaces:actor/host-locks@0.1.0
  var _noop5 = () => void 0;
  var lockAcquire = _noop5;
  var lockRelease = _noop5;
  var lockRenew = _noop5;

  // wit-stub:plexspaces:actor/host-blob@0.1.0
  var _noop6 = () => void 0;
  var blobUpload = _noop6;
  var blobDownload = _noop6;
  var blobDelete = _noop6;
  var blobList = _noop6;

  // wit-stub:plexspaces:actor/host-pool@0.1.0
  var _noop7 = () => void 0;
  var poolCheckout = _noop7;
  var poolCheckin = _noop7;
  var poolGetMetrics = _noop7;

  // wit-stub:plexspaces:actor/host-shard@0.1.0
  var _noop8 = () => void 0;
  var createShardGroup = _noop8;
  var bulkUpdateShardGroup = _noop8;
  var mapShardGroup = _noop8;
  var scatterGather = _noop8;
  var broadcastShardGroup = _noop8;
  var reduceShardGroup = _noop8;
  var allReduceShardGroup = _noop8;
  var barrierShardGroup = _noop8;
  var spawnActors = _noop8;
  var applicationMetricsAdd = _noop8;
  var applicationGetMetrics = _noop8;
  var applicationGetStatus = _noop8;

  // wit-stub:plexspaces:actor/host-http@0.1.0
  var _noop9 = () => void 0;
  var httpFetch = _noop9;

  // wit-stub:plexspaces:actor/registry@0.1.0
  var _noop10 = () => void 0;
  var register = _noop10;
  var unregister = _noop10;
  var lookup = _noop10;
  var lookupByAlias = _noop10;
  var discover = _noop10;
  var heartbeat = _noop10;

  // ../../../../sdks/typescript/dist/host.js
  function safeCall(fn, ...args) {
    if (typeof fn === "function") {
      return fn(...args);
    }
    return "";
  }
  function hostPayloadToBytes(result) {
    if (result instanceof Uint8Array)
      return result;
    if (ArrayBuffer.isView(result)) {
      const v = result;
      return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
    }
    if (result instanceof ArrayBuffer) {
      return new Uint8Array(result);
    }
    if (typeof result === "string") {
      const out = new Uint8Array(result.length);
      for (let i = 0; i < result.length; i++)
        out[i] = result.charCodeAt(i) & 255;
      return out;
    }
    return new Uint8Array(0);
  }
  function hostErrorPrefixBytes(raw) {
    const prefix = "ERROR:";
    if (raw.length < prefix.length)
      return false;
    for (let i = 0; i < prefix.length; i++) {
      if (raw[i] !== prefix.charCodeAt(i))
        return false;
    }
    return true;
  }
  function bytesToBase64(bytes2) {
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let result = "";
    const len = bytes2.length;
    for (let i = 0; i < len; i += 3) {
      const b0 = bytes2[i];
      const b1 = i + 1 < len ? bytes2[i + 1] : 0;
      const b2 = i + 2 < len ? bytes2[i + 2] : 0;
      result += chars[b0 >> 2] + chars[(b0 & 3) << 4 | b1 >> 4] + (i + 1 < len ? chars[(b1 & 15) << 2 | b2 >> 6] : "=") + (i + 2 < len ? chars[b2 & 63] : "=");
    }
    return result;
  }
  function base64ToBytes(b64) {
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const clean = b64.replace(/=+$/, "");
    const len = clean.length;
    const bytes2 = new Uint8Array(Math.floor(len * 3 / 4));
    let pos = 0;
    for (let i = 0; i < len; i += 4) {
      const c0 = chars.indexOf(clean[i]);
      const c1 = chars.indexOf(clean[i + 1]);
      const c2 = i + 2 < len ? chars.indexOf(clean[i + 2]) : 0;
      const c3 = i + 3 < len ? chars.indexOf(clean[i + 3]) : 0;
      bytes2[pos++] = c0 << 2 | c1 >> 4;
      if (i + 2 < len)
        bytes2[pos++] = (c1 & 15) << 4 | c2 >> 2;
      if (i + 3 < len)
        bytes2[pos++] = (c2 & 3) << 6 | c3;
    }
    return bytes2.subarray(0, pos);
  }
  var TupleSpace = class {
    constructor(host2) {
      this.host = host2;
    }
    /**
     * Write a tuple. Values are encoded as plexspaces.tuplespace.v1 WriteRequest protobuf wire
     * (same as Go `TupleSpace.Write` / Rust simple_component_host).
     */
    write(tuple) {
      try {
        const wire = encodeWriteRequest(tuple);
        return this.host.tsWritePayload(wire);
      } catch (e) {
        return `ERROR: ${e instanceof Error ? e.message : String(e)}`;
      }
    }
    /** Take one matching tuple (destructive). */
    take(pattern) {
      try {
        const wire = encodeReadRequest(pattern, true, 1);
        const raw = this.host.tsTakePayload(wire);
        if (raw.length === 0 || hostErrorPrefixBytes(raw))
          return null;
        return decodeReadResponseFirstTuple(raw);
      } catch {
        return null;
      }
    }
    /** Read one matching tuple (non-destructive). */
    read(pattern) {
      try {
        const wire = encodeReadRequest(pattern, false, 1);
        const raw = this.host.tsReadPayload(wire);
        if (raw.length === 0 || hostErrorPrefixBytes(raw))
          return null;
        return decodeReadResponseFirstTuple(raw);
      } catch {
        return null;
      }
    }
    /** Read all matching tuples (non-destructive). */
    readAll(pattern) {
      try {
        const wire = encodeReadRequest(pattern, false, 1024);
        const raw = this.host.tsReadAllPayload(wire);
        if (raw.length === 0 || hostErrorPrefixBytes(raw))
          return [];
        return decodeReadResponseAllTuples(raw);
      } catch {
        return [];
      }
    }
  };
  var ProcessGroups = class {
    /** Join a named process group */
    join(group) {
      const result = safeCall(pgJoin, group);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /** Leave a named process group */
    leave(group) {
      const result = safeCall(pgLeave, group);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /** Get members of a process group */
    members(group) {
      const raw = safeCall(pgMembers, group);
      const result = decodeWitPayloadUtf8(raw);
      if (result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      try {
        return JSON.parse(result);
      } catch {
        return [];
      }
    }
    /** Broadcast to all group members. msgType is used for routing so payload can be data-only. */
    broadcast(group, msgType, payload) {
      const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "{}");
      const result = safeCall(pgBroadcast, group, msgType, payloadBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /** Return the first member of a process group, or null if empty. */
    first(group) {
      return firstGroupMember(this.members(group));
    }
    /** Return the first member of a process group, throwing if empty. */
    firstOrThrow(group) {
      return firstGroupMemberOrThrow(group, this.members(group));
    }
  };
  var Registry = class {
    /**
     * Register an object in the registry.
     */
    register(reg) {
      const reqBytes = encodeRegisterRequest(reg);
      const result = safeCall(register, reqBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /**
     * Unregister an object from the registry.
     */
    unregister(objectId, objectType, tenantId, namespace) {
      const reqBytes = encodeUnregisterRequest(objectId, objectType, tenantId, namespace);
      const result = safeCall(unregister, reqBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /**
     * Look up an object by ID. Returns null if not found, throws on storage errors.
     */
    lookup(objectId, objectType = 0, tenantId, namespace) {
      const reqBytes = encodeLookupRequest(objectId, objectType, tenantId, namespace);
      const raw = safeCall(lookup, reqBytes);
      if (typeof raw === "string" && raw.startsWith("ERROR:")) {
        throw new Error(raw);
      }
      if (!raw)
        return null;
      const bytes2 = raw instanceof Uint8Array ? raw : new Uint8Array(0);
      if (bytes2.length === 0)
        return null;
      return decodeLookupResponse(bytes2);
    }
    /**
     * Look up an object by alias (Orleans grain directory pattern).
     * Alias format: "{actor_type}:{name}:{namespace}:{tenant_id}"
     * Returns null if not found, throws on storage errors.
     */
    lookupByAlias(alias) {
      const raw = safeCall(lookupByAlias, alias);
      if (typeof raw === "string" && raw.startsWith("ERROR:")) {
        throw new Error(raw);
      }
      if (!raw)
        return null;
      const bytes2 = raw instanceof Uint8Array ? raw : new Uint8Array(0);
      if (bytes2.length === 0)
        return null;
      return decodeLookupResponse(bytes2);
    }
    /**
     * Discover objects with optional filtering.
     */
    discover(options = {}) {
      const reqBytes = encodeDiscoverRequest(options);
      const raw = safeCall(discover, reqBytes);
      if (!raw)
        return [];
      if (typeof raw === "string" && raw.startsWith("ERROR:")) {
        throw new Error(raw);
      }
      const bytes2 = raw instanceof Uint8Array ? raw : new Uint8Array(0);
      if (bytes2.length === 0)
        return [];
      return decodeDiscoverResponse(bytes2);
    }
    /**
     * Update the heartbeat for a registered object.
     */
    heartbeat(objectId, objectType = 0, tenantId, namespace) {
      const reqBytes = encodeHeartbeatRequest(objectId, objectType, tenantId, namespace);
      const result = safeCall(heartbeat, reqBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
  };
  var KVStore = class {
    get(key) {
      if (typeof kvGet !== "function")
        return null;
      try {
        const v = decodeWitPayloadUtf8(kvGet(key));
        return v || null;
      } catch {
        return null;
      }
    }
    put(key, value) {
      if (typeof kvPut !== "function")
        return;
      try {
        kvPut(key, encodeWitPayloadUtf8(value));
      } catch {
      }
    }
    delete(key) {
      if (typeof kvDelete !== "function")
        return;
      try {
        kvDelete(key);
      } catch {
      }
    }
    list(prefix) {
      if (typeof kvList !== "function")
        return [];
      try {
        return kvList(prefix);
      } catch {
        return [];
      }
    }
    putWithTtl(key, value, ttlSeconds) {
      if (typeof kvPutWithTtl !== "function")
        return;
      kvPutWithTtl(key, encodeWitPayloadUtf8(value), BigInt(ttlSeconds));
    }
    getTtl(key) {
      if (typeof kvGetTtl !== "function")
        return 0;
      try {
        return Number(kvGetTtl(key));
      } catch {
        return 0;
      }
    }
    cas(key, expected, newValue) {
      if (typeof kvCas !== "function")
        return false;
      const expectedBytes = expected !== null ? encodeWitPayloadUtf8(expected) : new Uint8Array(0);
      return kvCas(key, expectedBytes, encodeWitPayloadUtf8(newValue));
    }
    increment(key, delta) {
      if (typeof kvIncrement !== "function")
        return 0;
      try {
        return Number(kvIncrement(key, BigInt(delta)));
      } catch {
        return 0;
      }
    }
    multiGet(keys) {
      if (typeof kvMultiGet !== "function")
        return keys.map(() => null);
      try {
        const keysJson = encodeWitPayloadUtf8(JSON.stringify(keys));
        const resultBytes = kvMultiGet(keysJson);
        const resultJson = decodeWitPayloadUtf8(resultBytes);
        const items = JSON.parse(resultJson);
        return items.map((v) => {
          if (v === null)
            return null;
          const b = base64ToBytes(v);
          return new TextDecoder().decode(b);
        });
      } catch {
        return keys.map(() => null);
      }
    }
    multiPut(entries) {
      if (typeof kvMultiPut !== "function")
        return;
      const encoded = {};
      for (const [k, v] of Object.entries(entries)) {
        encoded[k] = bytesToBase64(new TextEncoder().encode(v));
      }
      const entriesJson = encodeWitPayloadUtf8(JSON.stringify(encoded));
      kvMultiPut(entriesJson);
    }
    getJson(key) {
      const raw = this.get(key);
      if (!raw || raw.startsWith("ERROR:"))
        return null;
      try {
        return JSON.parse(raw);
      } catch {
        return null;
      }
    }
    putJson(key, value) {
      const serialized = JSON.stringify(value);
      this.put(key, serialized);
    }
  };
  var AlarmClient = class {
    set(timestampMs) {
      if (typeof alarmSet !== "function")
        return;
      alarmSet(BigInt(timestampMs));
    }
    setIn(delayMs) {
      if (typeof nowMs !== "function")
        return;
      const now = Number(nowMs());
      this.set(now + delayMs);
    }
    get() {
      if (typeof alarmGet !== "function")
        return 0;
      try {
        return Number(alarmGet());
      } catch {
        return 0;
      }
    }
    delete() {
      if (typeof alarmDelete !== "function")
        return;
      alarmDelete();
    }
  };
  var LockClient = class {
    acquire(holderId, lockName, leaseDurationSecs, timeoutMs) {
      const result = lockAcquire(holderId, lockName, leaseDurationSecs, timeoutMs);
      if (typeof result === "string")
        throw new Error(result);
      return result;
    }
    release(lockId, holderId, lockVersion) {
      const result = lockRelease(lockId, holderId, lockVersion);
      if (typeof result === "string")
        throw new Error(result);
    }
    renew(lockId, holderId, lockVersion, leaseDurationSecs) {
      const result = lockRenew(lockId, holderId, lockVersion, leaseDurationSecs);
      if (typeof result === "string")
        throw new Error(result);
      return result;
    }
  };
  var BlobClient = class {
    upload(name, data, contentType) {
      const result = blobUpload(name, data, contentType);
      if (typeof result !== "string" || result.startsWith("ERROR:"))
        throw new Error(String(result));
      return result;
    }
    download(blobId) {
      const result = blobDownload(blobId);
      if (typeof result === "string")
        throw new Error(result);
      return result;
    }
    delete(blobId) {
      const result = blobDelete(blobId);
      if (typeof result === "string")
        throw new Error(result);
    }
    list(prefix) {
      const result = blobList(prefix);
      if (Array.isArray(result))
        return result;
      return [];
    }
  };
  var Host = class {
    constructor() {
      this.processGroups = new ProcessGroups();
      this.ts = new TupleSpace(this);
      this.registry = new Registry();
      this.kv = new KVStore();
      this.alarm = new AlarmClient();
      this.locks = new LockClient();
      this.blob = new BlobClient();
    }
    /**
     * Create an ergonomic HTTP client for a named service link.
     *
     * The link must be pre-configured in RuntimeConfig.service_links.
     * The host handles retries, circuit breaking, and auth injection.
     *
     * @param linkName - Service link name (e.g. "payments-api")
     * @returns A {@link ServiceHttpClient} bound to that link
     *
     * @example
     * ```typescript
     * const http = host.httpClient("payments-api");
     * const balance = http.get("/v1/balance?account=123");
     * const result = http.post("/v1/transfer", { amount: 100 });
     * ```
     */
    httpClient(linkName) {
      return new ServiceHttpClient(linkName);
    }
    // ========================================================================
    // Messaging
    // ========================================================================
    /** Send message to another actor (fire-and-forget) */
    send(to, msgType, payload) {
      const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "");
      const raw = safeCall(send, to, msgType, payloadBytes);
      if (typeof raw !== "string") {
        return "";
      }
      return raw;
    }
    /** Send request and wait for response (request-reply) */
    ask(to, msgType, payload, timeoutMs = 5e3) {
      const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "");
      const raw = safeCall(ask, to, msgType, payloadBytes, BigInt(timeoutMs));
      const result = decodeWitPayloadUtf8(raw);
      if (result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      try {
        return JSON.parse(result);
      } catch {
        return result;
      }
    }
    // ========================================================================
    // Actor Identity
    // ========================================================================
    /** Get own actor ID */
    selfId() {
      return safeCall(selfId);
    }
    // ========================================================================
    // Actor Lifecycle
    // ========================================================================
    /**
     * Spawn a new actor through the framework-owned actor spawn path exposed by the host.
     * Returns the canonical actor ID assigned by the framework — use this ID (not actorName)
     * for all subsequent ask/send/stop calls.
     *
     * @param moduleRef - Actor type/module reference (must be deployed)
     * @param actorName - Requested name for the new actor. The framework forms the full canonical
     *                    ID from this name, moduleRef, namespace and node. Pass empty string to
     *                    let the framework auto-generate a ULID name.
     * @param role - Disambiguation key used ONLY when multiple actors in the same supervisor share
     *               the same actor_type (moduleRef). Pass empty string when moduleRef is unique.
     * @param args - Key-value init arguments forwarded to the new actor's init()
     * @returns Canonical actor ID string assigned by the framework
     */
    spawn(moduleRef, actorName = "", role = "", args = {}) {
      const argsJson = Object.keys(args).length > 0 ? JSON.stringify(args) : "{}";
      const result = safeCall(spawn, moduleRef, actorName, role, argsJson);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return result;
    }
    /** Stop an actor gracefully */
    stop(actorId) {
      const result = safeCall(stop, actorId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    // ========================================================================
    // Actor Linking & Monitoring
    // ========================================================================
    /** Bidirectional link */
    link(actorId) {
      const result = safeCall(link, actorId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /** Remove bidirectional link */
    unlink(actorId) {
      const result = safeCall(unlink, actorId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /** Monitor an actor (returns monitor reference) */
    monitor(actorId) {
      const result = safeCall(monitor, actorId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return result;
    }
    /** Cancel a monitor */
    demonitor(monitorRef) {
      const result = safeCall(demonitor, monitorRef);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    // ========================================================================
    // Timers
    // ========================================================================
    /**
     * Send message to self after delay (returns timer ID for tracking).
     * Timer cancellation is managed by the framework's TimerFacet/ReminderFacet.
     * Stop the actor to cancel pending timers.
     *
     * WIT `payload` is opaque bytes; pass UTF-8 JSON bytes so the host matches Go/Rust guest JSON.
     */
    sendAfter(delayMs, msgType, payload) {
      const text = payload !== void 0 ? JSON.stringify(payload) : "{}";
      const payloadBytes = new TextEncoder().encode(text);
      const raw = safeCall(sendAfter, BigInt(delayMs), msgType, payloadBytes);
      if (typeof raw === "string") {
        return raw;
      }
      if (raw && typeof raw === "object") {
        const o = raw;
        if (o.tag === "ok" || o.tag === 0) {
          return typeof o.val === "string" ? o.val : "";
        }
        if (o.tag === "err" || o.tag === 1) {
          return `ERROR:${String(o.val ?? "send-after failed")}`;
        }
      }
      return "";
    }
    // ========================================================================
    // Logging & Time
    // ========================================================================
    /** Log a message */
    log(level, message) {
      safeCall(log, level, message);
    }
    debug(message) {
      this.log("debug", message);
    }
    info(message) {
      this.log("info", message);
    }
    warn(message) {
      this.log("warn", message);
    }
    error(message) {
      this.log("error", message);
    }
    /** Get current timestamp in milliseconds */
    nowMs() {
      const result = safeCall(nowMs);
      return typeof result === "bigint" ? Number(result) : typeof result === "number" ? result : 0;
    }
    /** Increment a single named application metric counter by 1. Errors are swallowed. */
    incrCounter(applicationId, name) {
      this.incrCounters(applicationId, { [name]: 1 });
    }
    /** Increment one or more named application metric counters. Errors are swallowed. */
    incrCounters(applicationId, counters) {
      try {
        this.applicationMetricsAdd(applicationId, {
          message_count: Object.keys(counters).length,
          counter_metrics: counters
        });
      } catch (e) {
        this.warn(`incrCounters: metrics update failed: ${e}`);
      }
    }
    // ========================================================================
    // TupleSpace (protobuf WriteRequest / ReadRequest / ReadResponse wire bytes)
    // ========================================================================
    /** @internal TupleSpace — plexspaces.tuplespace.v1 wire bytes. */
    tsWritePayload(data) {
      const r = safeCall(tsWrite, data);
      return typeof r === "string" ? r : "";
    }
    /** @internal */
    tsReadPayload(data) {
      return hostPayloadToBytes(safeCall(tsRead, data));
    }
    /** @internal */
    tsTakePayload(data) {
      return hostPayloadToBytes(safeCall(tsTake, data));
    }
    /** @internal */
    tsReadAllPayload(data) {
      return hostPayloadToBytes(safeCall(tsReadAll, data));
    }
    // ========================================================================
    // Elastic pool (checkout/checkin)
    // ========================================================================
    /**
     * Checkout an actor from a named pool. Returns handle { actor_id, pool_name, checkout_id } or null on failure.
     */
    poolCheckout(poolName, timeoutMs = 5e3) {
      const result = safeCall(poolCheckout, poolName, BigInt(timeoutMs));
      if (typeof result !== "string" || result === "" || result.startsWith("ERROR:"))
        return null;
      try {
        return JSON.parse(result);
      } catch {
        return null;
      }
    }
    /**
     * Checkin an actor to the pool. Pass actor_id and checkout_id from the handle returned by poolCheckout.
     */
    poolCheckin(poolName, actorId, checkoutId, healthy) {
      const result = safeCall(poolCheckin, poolName, actorId, checkoutId, healthy);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
    }
    /**
     * Get pool metrics (total_actors, available_actors, busy_actors, current_load, etc.). Returns null if not available.
     */
    poolGetMetrics(poolName) {
      const result = safeCall(poolGetMetrics, poolName);
      if (typeof result !== "string" || result === "" || result.startsWith("ERROR:"))
        return null;
      try {
        return JSON.parse(result);
      } catch {
        return null;
      }
    }
    createShardGroup(request) {
      const reqBytes = encodeCreateShardGroupRequest(request);
      const result = safeCall(createShardGroup, reqBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0)
        return { shard_actor_ids: [] };
      const decoded = decodeCreateShardGroupResponse(bytes2);
      const group = decoded.group ?? {};
      return { ...group, ...decoded };
    }
    bulkUpdateShardGroup(request) {
      const result = safeCall(bulkUpdateShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    mapShardGroup(request) {
      const result = safeCall(mapShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    scatterGather(request) {
      const reqBytes = encodeScatterGatherRequest(request);
      const result = safeCall(scatterGather, reqBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0)
        return { shard_responses: [] };
      return decodeScatterGatherResponse(bytes2);
    }
    broadcastShardGroup(request) {
      const result = safeCall(broadcastShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    reduceShardGroup(request) {
      const result = safeCall(reduceShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    allReduceShardGroup(request) {
      const result = safeCall(allReduceShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    barrierShardGroup(request) {
      const result = safeCall(barrierShardGroup, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    spawnActors(request) {
      const result = safeCall(spawnActors, JSON.stringify(request));
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      return JSON.parse(result);
    }
    applicationMetricsAdd(applicationId, metrics) {
      const metricsBytes = encodeApplicationMetrics(metrics);
      const result = safeCall(applicationMetricsAdd, applicationId, metricsBytes);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0)
        return {};
      try {
        return JSON.parse(new TextDecoder().decode(bytes2));
      } catch {
        return {};
      }
    }
    applicationGetMetrics(applicationId, nodeId) {
      const result = safeCall(applicationGetMetrics, applicationId, nodeId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0)
        return {};
      return decodeApplicationMetrics(bytes2);
    }
    applicationGetStatus(applicationId, nodeId) {
      const result = safeCall(applicationGetStatus, applicationId, nodeId);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0)
        return { node_id: nodeId, node_address: "", application: null };
      return decodeGetApplicationStatusResponse(bytes2);
    }
    /**
     * Execute an outbound HTTP request via a named service link.
     *
     * The link must be pre-configured in RuntimeConfig.service_links.
     * The host handles retries, circuit breaking, and auth injection.
     *
     * @param linkName  Service link name (e.g. "payments-api")
     * @param method    HTTP method ("GET", "POST", "PUT", "DELETE", "PATCH")
     * @param pathAndQuery  Path and optional query string (e.g. "/v1/users?limit=10")
     * @param headers   Optional extra headers object
     * @param body      Optional request body string (JSON or base64-encoded bytes)
     * @returns Response object with status, headers, body
     */
    httpFetch(linkName, method, pathAndQuery, headers, body) {
      const bodyBytes = body !== void 0 && body.length > 0 ? new TextEncoder().encode(body) : new Uint8Array(0);
      const reqWire = encodeHttpFetchRequestWire(headers ?? {}, bodyBytes);
      const result = safeCall(httpFetch, linkName, method, pathAndQuery, reqWire);
      if (typeof result === "string" && result.startsWith("ERROR:")) {
        throw new Error(result);
      }
      const bytes2 = hostPayloadToBytes(result);
      if (bytes2.length === 0) {
        return { status: 0, headers: {}, body: "" };
      }
      if (hostErrorPrefixBytes(bytes2)) {
        throw new Error(new TextDecoder("utf-8", { fatal: false }).decode(bytes2));
      }
      const asText = new TextDecoder("utf-8", { fatal: false }).decode(bytes2);
      try {
        return JSON.parse(asText);
      } catch {
        return decodeHttpFetchResponseWire(bytes2);
      }
    }
  };
  var ServiceHttpClient = class {
    constructor(linkName) {
      this.linkName = linkName;
    }
    /** GET request. Returns response object with status, headers, body. */
    get(pathAndQuery, headers) {
      return host.httpFetch(this.linkName, "GET", pathAndQuery, headers);
    }
    /** POST JSON request. body is serialized to JSON. */
    post(pathAndQuery, body, headers) {
      const bodyStr = body !== void 0 ? JSON.stringify(body) : "";
      return host.httpFetch(this.linkName, "POST", pathAndQuery, headers, bodyStr);
    }
    /** PUT JSON request. */
    put(pathAndQuery, body, headers) {
      const bodyStr = body !== void 0 ? JSON.stringify(body) : "";
      return host.httpFetch(this.linkName, "PUT", pathAndQuery, headers, bodyStr);
    }
    /** DELETE request. */
    delete(pathAndQuery, headers) {
      return host.httpFetch(this.linkName, "DELETE", pathAndQuery, headers);
    }
  };
  var host = new Host();

  // ../../../../sdks/typescript/dist/actor_id.js
  var ActorID = class _ActorID {
    constructor(name, actorType, namespace, nodeId) {
      this.name = name;
      this.actorType = actorType;
      this.namespace = namespace;
      this.nodeId = nodeId;
    }
    /**
     * Parse a canonical actor ID string into an ActorID.
     *
     * Expected format: {name}//{actor_type}::{namespace}@{node_id}
     * Throws if the string does not contain the expected separators.
     */
    static parse(id) {
      const slashIdx = id.indexOf("//");
      if (slashIdx < 0) {
        throw new Error(`parseActorID: missing '//' in ${JSON.stringify(id)}`);
      }
      const name = id.slice(0, slashIdx);
      const rest = id.slice(slashIdx + 2);
      const atIdx = rest.indexOf("@");
      const nodeId = atIdx >= 0 ? rest.slice(atIdx + 1) : "";
      const typeNs = atIdx >= 0 ? rest.slice(0, atIdx) : rest;
      const colonIdx = typeNs.indexOf("::");
      const actorType = colonIdx >= 0 ? typeNs.slice(0, colonIdx) : typeNs;
      const namespace = colonIdx >= 0 ? typeNs.slice(colonIdx + 2) : "";
      return new _ActorID(name, actorType, namespace, nodeId);
    }
    /** Return the canonical actor ID string: {name}//{actor_type}::{namespace}@{node_id}. */
    toString() {
      if (this.nodeId) {
        return `${this.name}//${this.actorType}::${this.namespace}@${this.nodeId}`;
      }
      return `${this.name}//${this.actorType}::${this.namespace}`;
    }
    /**
     * Return a copy with an explicit actor type and name.
     *
     * Use this to build a canonical ID for a peer actor with the given type and name,
     * keeping the same namespace and node.
     *
     * For supervisor-spawned actors with stable role names (name == type == role):
     * ```ts
     * const peer = self.withTypeAndName("budget_manager", "budget_manager");
     * ```
     *
     * For actors where name and type differ (e.g. ULID-named workers of a shared type):
     * ```ts
     * const peer = self.withTypeAndName("inference_worker", ulid);
     * ```
     */
    withTypeAndName(actorType, name) {
      return new _ActorID(name, actorType, this.namespace, this.nodeId);
    }
    /** Return a copy with a different name. */
    withName(name) {
      return new _ActorID(name, this.actorType, this.namespace, this.nodeId);
    }
    /** Return a copy with a different name and actor_type. */
    withType(name, actorType) {
      return new _ActorID(name, actorType, this.namespace, this.nodeId);
    }
  };

  // ../../../../sdks/typescript/dist/wire/ws-frame-wire.js
  var textEnc = new TextEncoder();
  var textDec = new TextDecoder("utf-8", { fatal: false });
  function str(buf, fieldNum, s) {
    if (!s)
      return buf;
    return appendLengthDelimited(buf, fieldNum, new Uint8Array(textEnc.encode(s)));
  }
  function bytes(buf, fieldNum, data) {
    if (data.length === 0)
      return buf;
    return appendLengthDelimited(buf, fieldNum, data);
  }
  function uint32(buf, fieldNum, v) {
    if (v === 0)
      return buf;
    let b = appendVarint(buf, fieldNum << 3 | 0);
    return appendVarint(b, v >>> 0);
  }
  function uint64(buf, fieldNum, v) {
    if (v === 0)
      return buf;
    let b = appendVarint(buf, fieldNum << 3 | 0);
    return appendVarint(b, v);
  }
  function float32(buf, fieldNum, v) {
    if (v === 0)
      return buf;
    const tag = appendVarint(buf, fieldNum << 3 | 5);
    const out = new Uint8Array(tag.length + 4);
    out.set(tag, 0);
    new DataView(out.buffer, tag.length, 4).setFloat32(0, v, true);
    return out;
  }
  function mapStringString(buf, fieldNum, m) {
    let out = buf;
    for (const [k, v] of Object.entries(m)) {
      let entry = new Uint8Array(0);
      entry = str(entry, 1, k);
      entry = str(entry, 2, v);
      out = appendLengthDelimited(out, fieldNum, entry);
    }
    return out;
  }
  function encodeWsFrameTell(requestId, actorId, msgType, payloadBytes) {
    const parsed = ActorID.parse(actorId);
    let inner = new Uint8Array(0);
    inner = str(inner, 2, parsed.namespace);
    inner = str(inner, 3, actorId);
    if (msgType)
      inner = str(inner, 11, msgType);
    inner = bytes(inner, 5, payloadBytes);
    let frame = new Uint8Array(0);
    frame = str(frame, 1, requestId);
    frame = appendLengthDelimited(frame, 10, inner);
    return frame;
  }
  function encodeWsFrameAsk(requestId, actorId, msgType, payloadBytes, timeoutMs) {
    const parsed = ActorID.parse(actorId);
    let inner = new Uint8Array(0);
    inner = str(inner, 1, requestId);
    inner = str(inner, 2, parsed.namespace);
    inner = str(inner, 3, actorId);
    inner = str(inner, 4, "POST");
    if (msgType)
      inner = str(inner, 11, msgType);
    inner = bytes(inner, 5, payloadBytes);
    if (timeoutMs > 0) {
      const secs = Math.floor(timeoutMs / 1e3);
      const nanos = timeoutMs % 1e3 * 1e6;
      let dur = new Uint8Array(0);
      if (secs > 0)
        dur = uint64(dur, 1, secs);
      if (nanos > 0)
        dur = uint32(dur, 2, nanos);
      inner = appendLengthDelimited(inner, 15, dur);
    }
    let frame = new Uint8Array(0);
    frame = str(frame, 1, requestId);
    frame = appendLengthDelimited(frame, 12, inner);
    return frame;
  }
  function encodeWsFrameNodeRegister(requestId, nodeId, nodeAddress, capabilities, resourceHints) {
    let inner = new Uint8Array(0);
    inner = str(inner, 1, nodeId);
    inner = str(inner, 2, nodeAddress);
    inner = mapStringString(inner, 3, capabilities);
    inner = uint32(inner, 10, 2);
    if (resourceHints) {
      let hints = new Uint8Array(0);
      if (resourceHints.cpuPercent)
        hints = float32(hints, 1, resourceHints.cpuPercent);
      if (resourceHints.memoryAvailableMb)
        hints = uint64(hints, 2, resourceHints.memoryAvailableMb);
      if (resourceHints.availableCores)
        hints = uint32(hints, 3, resourceHints.availableCores);
      if (hints.length > 0)
        inner = appendLengthDelimited(inner, 11, hints);
    }
    let frame = new Uint8Array(0);
    frame = str(frame, 1, requestId);
    frame = appendLengthDelimited(frame, 20, inner);
    return frame;
  }
  function encodeWsFrameHeartbeat(requestId, nodeId) {
    let inner = new Uint8Array(0);
    inner = str(inner, 1, requestId);
    inner = str(inner, 2, nodeId);
    let frame = new Uint8Array(0);
    frame = str(frame, 1, requestId);
    frame = appendLengthDelimited(frame, 24, inner);
    return frame;
  }
  function encodeWsFrameNodePing(requestId, sourceNodeId, sequenceNumber) {
    let inner = new Uint8Array(0);
    inner = str(inner, 1, requestId);
    inner = str(inner, 2, sourceNodeId);
    if (sequenceNumber)
      inner = uint64(inner, 3, sequenceNumber);
    let frame = new Uint8Array(0);
    frame = str(frame, 1, requestId);
    frame = appendLengthDelimited(frame, 22, inner);
    return frame;
  }
  function parseMessage(data) {
    const fields = /* @__PURE__ */ new Map();
    const push = (fn, v) => {
      const arr = fields.get(fn) ?? [];
      arr.push(v);
      fields.set(fn, arr);
    };
    let pos = 0;
    while (pos < data.length) {
      const { value: tag, n: tn } = readVarint(data, pos);
      pos += tn;
      const fn = Number(tag >> 3n);
      const wt = Number(tag & 7n);
      if (wt === 0) {
        const { value, n } = readVarint(data, pos);
        pos += n;
        push(fn, value);
      } else if (wt === 2) {
        const { slice, nextPos } = readLengthDelimited(data, pos);
        pos = nextPos;
        push(fn, slice);
      } else if (wt === 5) {
        if (pos + 4 > data.length)
          throw new Error("fixed32 underflow");
        const v = new DataView(data.buffer, data.byteOffset + pos, 4).getFloat32(0, true);
        pos += 4;
        push(fn, v);
      } else {
        pos = skipField(data, pos, wt);
      }
    }
    return fields;
  }
  function getStr(fields, fn) {
    const arr = fields.get(fn);
    if (!arr || arr.length === 0)
      return "";
    const v = arr[0];
    if (v instanceof Uint8Array)
      return textDec.decode(v);
    return "";
  }
  function getU64(fields, fn) {
    const arr = fields.get(fn);
    if (!arr || arr.length === 0)
      return 0;
    const v = arr[0];
    if (typeof v === "bigint")
      return Number(v);
    return 0;
  }
  function getBool(fields, fn) {
    const arr = fields.get(fn);
    if (!arr || arr.length === 0)
      return false;
    const v = arr[0];
    if (typeof v === "bigint")
      return v !== 0n;
    return false;
  }
  function getFloat(fields, fn) {
    const arr = fields.get(fn);
    if (!arr || arr.length === 0)
      return 0;
    const v = arr[0];
    if (typeof v === "number")
      return v;
    return 0;
  }
  function getBytes(fields, fn) {
    const arr = fields.get(fn);
    if (!arr || arr.length === 0)
      return new Uint8Array(0);
    const v = arr[0];
    if (v instanceof Uint8Array)
      return v;
    return new Uint8Array(0);
  }
  function parseJson(raw) {
    if (raw.length === 0)
      return null;
    try {
      return JSON.parse(textDec.decode(raw));
    } catch {
      return textDec.decode(raw);
    }
  }
  function parseAskResponse(data) {
    const f = parseMessage(data);
    return {
      requestId: getStr(f, 1),
      success: getBool(f, 2),
      payloadJson: parseJson(getBytes(f, 3)),
      errorMessage: getStr(f, 6)
    };
  }
  function parseTellResponse(data) {
    const f = parseMessage(data);
    return { requestId: getStr(f, 1), success: getBool(f, 2), errorMessage: getStr(f, 5) };
  }
  function parseRegisterAck(data) {
    const f = parseMessage(data);
    return { success: getBool(f, 1), assignedNodeId: getStr(f, 2), errorMessage: getStr(f, 3) };
  }
  function parseNodeResourceHints(data) {
    const f = parseMessage(data);
    return {
      cpuPercent: getFloat(f, 1),
      memoryAvailableMb: getU64(f, 2),
      availableCores: getU64(f, 3)
    };
  }
  function parsePingResponse(data) {
    const f = parseMessage(data);
    const resourcesBytes = getBytes(f, 9);
    const hints = resourcesBytes.length > 0 ? parseNodeResourceHints(resourcesBytes) : { cpuPercent: 0, memoryAvailableMb: 0, availableCores: 0 };
    return {
      requestId: getStr(f, 1),
      nodeId: getStr(f, 2),
      cpuPercent: hints.cpuPercent,
      memoryAvailableMb: hints.memoryAvailableMb,
      availableCores: hints.availableCores
    };
  }
  function parseIncomingTell(requestId, data) {
    const f = parseMessage(data);
    return {
      actorId: getStr(f, 3),
      msgType: getStr(f, 11),
      payloadJson: parseJson(getBytes(f, 5))
    };
  }
  function parseError(data) {
    const f = parseMessage(data);
    return { requestId: getStr(f, 1), code: getU64(f, 2), message: getStr(f, 3) };
  }
  function decodeWsFrame(bytes2) {
    try {
      const top = parseMessage(bytes2);
      const requestId = getStr(top, 1);
      for (const [fn, arr] of top.entries()) {
        if (!(arr[0] instanceof Uint8Array))
          continue;
        const payload = arr[0];
        switch (fn) {
          case 10: {
            const t = parseIncomingTell(requestId, payload);
            return { type: "incoming_tell", requestId, actorId: t.actorId, msgType: t.msgType, payloadJson: t.payloadJson };
          }
          case 11: {
            const t = parseTellResponse(payload);
            return { type: "tell_response", requestId: t.requestId || requestId, success: t.success, errorMessage: t.errorMessage };
          }
          case 13: {
            const a = parseAskResponse(payload);
            return { type: "ask_response", requestId: a.requestId || requestId, success: a.success, payloadJson: a.payloadJson, errorMessage: a.errorMessage };
          }
          case 21: {
            const r = parseRegisterAck(payload);
            return { type: "node_register_ack", requestId, success: r.success, assignedNodeId: r.assignedNodeId, errorMessage: r.errorMessage };
          }
          case 23: {
            const p = parsePingResponse(payload);
            return { type: "node_ping_response", requestId: p.requestId || requestId, nodeId: p.nodeId, cpuPercent: p.cpuPercent, memoryAvailableMb: p.memoryAvailableMb, availableCores: p.availableCores };
          }
          case 25:
            return { type: "heartbeat_ack", requestId };
          case 30: {
            const e = parseError(payload);
            return { type: "error", requestId: e.requestId || requestId, code: e.code, message: e.message };
          }
        }
      }
      return { type: "unknown" };
    } catch {
      return { type: "unknown" };
    }
  }

  // ../../../../sdks/typescript/dist/ws_thin_client.js
  var CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  function newUlid() {
    const now = Date.now();
    let t = now;
    let ts = "";
    for (let i = 9; i >= 0; i--) {
      ts = CROCKFORD[t % 32] + ts;
      t = Math.floor(t / 32);
    }
    const rb = new Uint8Array(10);
    if (typeof crypto !== "undefined" && crypto.getRandomValues) {
      crypto.getRandomValues(rb);
    } else {
      for (let i = 0; i < 10; i++)
        rb[i] = Math.floor(Math.random() * 256);
    }
    let rand = "";
    let acc = 0, bits = 0;
    for (let i = 0; i < 10; i++) {
      acc = acc << 8 | rb[i];
      bits += 8;
      while (bits >= 5) {
        bits -= 5;
        rand += CROCKFORD[acc >>> bits & 31];
      }
    }
    return ts + rand;
  }
  var WsThinClient = class {
    constructor(opts) {
      this.opts = opts;
      this.ws = null;
      this.assignedNodeId = "";
      this.pendingAsks = /* @__PURE__ */ new Map();
      this.pendingPings = /* @__PURE__ */ new Map();
      this.pendingReg = null;
      this.messageHandler = null;
      this.heartbeatTimer = null;
      this.HEARTBEAT_INTERVAL_MS = 25e3;
      this.DEFAULT_ASK_TIMEOUT_MS = 5e3;
    }
    /**
     * Open the WebSocket, complete the NodeRegistration handshake, and start the
     * heartbeat loop.  Returns the server-assigned node_id.
     */
    async connect() {
      return new Promise((resolve, reject) => {
        let url = this.opts.wsUrl;
        if (this.opts.jwtToken) {
          const sep = url.includes("?") ? "&" : "?";
          url = `${url}${sep}token=${encodeURIComponent(this.opts.jwtToken)}`;
        }
        this.ws = new WebSocket(url);
        this.ws.binaryType = "arraybuffer";
        this.ws.onopen = () => {
          const nodeId = this.opts.nodeId ?? newUlid();
          const capabilities = {};
          if (this.opts.tenant)
            capabilities["tenant"] = this.opts.tenant;
          if (this.opts.namespace)
            capabilities["namespace"] = this.opts.namespace;
          if (typeof navigator !== "undefined") {
            capabilities["cpu_cores"] = String(navigator.hardwareConcurrency ?? 1);
          }
          const resourceHints = {};
          if (typeof navigator !== "undefined") {
            resourceHints.availableCores = navigator.hardwareConcurrency ?? 1;
          }
          const requestId = newUlid();
          this.pendingReg = { resolve, reject };
          const frame = encodeWsFrameNodeRegister(requestId, nodeId, "", capabilities, resourceHints);
          this.ws.send(frame);
        };
        this.ws.onmessage = (ev) => {
          const buf = ev.data instanceof ArrayBuffer ? new Uint8Array(ev.data) : new Uint8Array(ev.data);
          this.handleFrame(buf);
        };
        this.ws.onerror = () => {
          const err = new Error("WebSocket error");
          this.rejectAllPending(err);
          reject(err);
        };
        this.ws.onclose = (ev) => {
          const err = new Error(`WebSocket closed: ${ev.code} ${ev.reason}`);
          this.rejectAllPending(err);
          if (this.pendingReg) {
            this.pendingReg.reject(err);
            this.pendingReg = null;
          }
          this.stopHeartbeat();
        };
      });
    }
    /**
     * Fire-and-forget. actorId must be the canonical form:
     *   {name}//{type}::{namespace}@{nodeId}
     */
    async tell(actorId, msgType, payload) {
      const frame = encodeWsFrameTell(newUlid(), actorId, msgType, new TextEncoder().encode(JSON.stringify(payload)));
      this.send(frame);
    }
    /**
     * Request-reply. Returns the response payload (parsed JSON).
     * Rejects with a timeout error if no response arrives within timeoutMs.
     */
    async ask(actorId, msgType, payload, timeoutMs = this.DEFAULT_ASK_TIMEOUT_MS) {
      const requestId = newUlid();
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          this.pendingAsks.delete(requestId);
          reject(new Error(`ask timeout after ${timeoutMs}ms for ${actorId}`));
        }, timeoutMs);
        this.pendingAsks.set(requestId, { resolve, reject, timer });
        const frame = encodeWsFrameAsk(requestId, actorId, msgType, new TextEncoder().encode(JSON.stringify(payload)), timeoutMs);
        this.send(frame);
      });
    }
    /**
     * Register a handler for incoming tell frames addressed to this thin node.
     * Called whenever the server routes a tell to one of this node's actor IDs.
     */
    onMessage(handler2) {
      this.messageHandler = handler2;
    }
    /**
     * Send a SWIM-compatible ping to a target node and return its resource hints.
     * The target nodeId must be known to the server (registered in SWIM membership).
     */
    async pingNode(targetNodeId, timeoutMs = 5e3) {
      const requestId = newUlid();
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          this.pendingPings.delete(requestId);
          reject(new Error(`ping timeout for ${targetNodeId}`));
        }, timeoutMs);
        this.pendingPings.set(requestId, { resolve, reject, timer });
        const frame = encodeWsFrameNodePing(requestId, this.assignedNodeId, Date.now());
        this.send(frame);
      });
    }
    /** Send a heartbeat frame to keep the WS session alive. */
    async heartbeat() {
      const frame = encodeWsFrameHeartbeat(newUlid(), this.assignedNodeId);
      this.send(frame);
    }
    /**
     * Build a canonical actor ID on this thin node.
     * Format: {name}//{type}::{namespace}@{assignedNodeId}
     */
    localActorId(name, type, namespace) {
      const ns = namespace ?? this.opts.namespace ?? "default";
      return `${name}//${type}::${ns}@${this.assignedNodeId}`;
    }
    /** The server-assigned node_id (available after connect() resolves). */
    get nodeId() {
      return this.assignedNodeId;
    }
    /** Generate a new ULID. Exposed so examples can use it without a dep. */
    static newUlid() {
      return newUlid();
    }
    /** Disconnect and clean up. */
    async disconnect() {
      this.stopHeartbeat();
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        this.ws.close(1e3, "client disconnect");
      }
      this.ws = null;
    }
    // ─── Private ──────────────────────────────────────────────────────────────
    send(data) {
      if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
        throw new Error("WsThinClient: not connected");
      }
      this.ws.send(data);
    }
    handleFrame(buf) {
      const frame = decodeWsFrame(buf);
      switch (frame.type) {
        case "node_register_ack": {
          if (this.pendingReg) {
            const reg = this.pendingReg;
            this.pendingReg = null;
            if (frame.success) {
              this.assignedNodeId = frame.assignedNodeId;
              this.startHeartbeat();
              reg.resolve(frame.assignedNodeId);
            } else {
              reg.reject(new Error(`registration failed: ${frame.errorMessage}`));
            }
          }
          break;
        }
        case "ask_response": {
          const pending = this.pendingAsks.get(frame.requestId);
          if (pending) {
            clearTimeout(pending.timer);
            this.pendingAsks.delete(frame.requestId);
            if (frame.success) {
              pending.resolve(frame.payloadJson);
            } else {
              pending.reject(new Error(frame.errorMessage || "ask failed"));
            }
          }
          break;
        }
        case "tell_response":
          break;
        case "heartbeat_ack":
          break;
        case "node_ping_response": {
          const pending = this.pendingPings.get(frame.requestId);
          if (pending) {
            clearTimeout(pending.timer);
            this.pendingPings.delete(frame.requestId);
            pending.resolve({
              success: true,
              nodeId: frame.nodeId,
              cpuPercent: frame.cpuPercent,
              memoryAvailableMb: frame.memoryAvailableMb,
              availableCores: frame.availableCores
            });
          }
          break;
        }
        case "incoming_tell": {
          if (this.messageHandler) {
            this.messageHandler(frame.actorId, frame.msgType, frame.payloadJson);
          }
          break;
        }
        case "error": {
          const pending = this.pendingAsks.get(frame.requestId);
          if (pending) {
            clearTimeout(pending.timer);
            this.pendingAsks.delete(frame.requestId);
            pending.reject(new Error(`server error ${frame.code}: ${frame.message}`));
          }
          break;
        }
      }
    }
    startHeartbeat() {
      this.heartbeatTimer = setInterval(() => {
        this.heartbeat().catch(() => {
        });
      }, this.HEARTBEAT_INTERVAL_MS);
    }
    stopHeartbeat() {
      if (this.heartbeatTimer !== null) {
        clearInterval(this.heartbeatTimer);
        this.heartbeatTimer = null;
      }
    }
    rejectAllPending(err) {
      for (const [, p] of this.pendingAsks) {
        clearTimeout(p.timer);
        p.reject(err);
      }
      this.pendingAsks.clear();
      for (const [, p] of this.pendingPings) {
        clearTimeout(p.timer);
        p.reject(err);
      }
      this.pendingPings.clear();
    }
  };

  // static/client.ts
  var wsUrlInput = document.getElementById("ws-url");
  var jwtInput = document.getElementById("jwt-token");
  var leaderNodeInput = document.getElementById("leader-node-id");
  var candidateCountInput = document.getElementById("candidate-count");
  var connectBtn = document.getElementById("connect-btn");
  var disconnectBtn = document.getElementById("disconnect-btn");
  var startRunBtn = document.getElementById("start-run-btn");
  var mStatus = document.getElementById("m-status");
  var mProgress = document.getElementById("m-progress");
  var mPct = document.getElementById("m-pct");
  var mPrimes = document.getElementById("m-primes");
  var mWorkers = document.getElementById("m-workers");
  var mCores = document.getElementById("m-cores");
  var mThroughput = document.getElementById("m-throughput");
  var mElapsed = document.getElementById("m-elapsed");
  var mSpeedup = document.getElementById("m-speedup");
  var mNode = document.getElementById("m-node");
  var mMyCores = document.getElementById("m-my-cores");
  var progDetail = document.getElementById("prog-detail");
  var progressBarInner = document.getElementById("progress-bar-inner");
  var candidatesDiv = document.getElementById("candidates");
  var workersList = document.getElementById("workers-list");
  var logDiv = document.getElementById("log");
  var client = null;
  var worker = null;
  var pollTimer = null;
  var elapsedTimer = null;
  var coordinatorId = "";
  var myActorId = "";
  var startedAt = 0;
  var isRunning = false;
  var APP_NS = "ts-mersenne-prime";
  var CANDIDATES = [
    2,
    3,
    5,
    7,
    13,
    17,
    19,
    31,
    61,
    89,
    107,
    127,
    521,
    607,
    1279,
    2203,
    2281,
    3217,
    4253,
    4423,
    9689,
    9941
  ];
  function log2(text, cls = "info") {
    const div = document.createElement("div");
    div.className = `log-entry ${cls}`;
    div.textContent = `${(/* @__PURE__ */ new Date()).toLocaleTimeString()} ${text}`;
    logDiv.insertBefore(div, logDiv.firstChild);
    while (logDiv.children.length > 150)
      logDiv.removeChild(logDiv.lastChild);
  }
  function shortId(actorId) {
    const idx = actorId.indexOf("//");
    return idx >= 0 ? actorId.slice(0, idx) : actorId.slice(0, 12);
  }
  function fmtDuration(ms) {
    if (ms < 1e3)
      return `${ms}ms`;
    if (ms < 6e4)
      return `${(ms / 1e3).toFixed(1)}s`;
    return `${Math.floor(ms / 6e4)}m${Math.floor(ms % 6e4 / 1e3)}s`;
  }
  function setButtonState(connected, running) {
    connectBtn.disabled = connected;
    disconnectBtn.disabled = !connected;
    startRunBtn.disabled = !connected || running;
    startRunBtn.textContent = running ? "Running\u2026" : "Start New Run";
  }
  function startElapsedTimer() {
    if (elapsedTimer)
      clearInterval(elapsedTimer);
    elapsedTimer = setInterval(() => {
      if (startedAt > 0 && isRunning) {
        const s = Math.floor((Date.now() - startedAt) / 1e3);
        mElapsed.textContent = s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${s % 60}s`;
      }
    }, 500);
  }
  function renderCandidates(details, total) {
    const byP = new Map(details.map((d) => [d.p, d]));
    candidatesDiv.innerHTML = "";
    for (const p of CANDIDATES.slice(0, total)) {
      const d = byP.get(p);
      const div = document.createElement("div");
      if (!d || d.status === "pending") {
        div.className = "candidate pending";
        div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>\u22121</div><div class="cand-dur">pending</div>`;
      } else if (d.status === "assigned") {
        div.className = "candidate assigned";
        div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>\u22121</div><div class="cand-dur">\u2699 computing\u2026</div><div class="cand-worker">${d.worker ? shortId(d.worker) : ""}</div>`;
      } else if (d.is_prime) {
        div.className = "candidate done-prime";
        div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>\u22121 \u2713</div><div class="cand-dur">${d.duration_ms != null ? fmtDuration(d.duration_ms) : ""}</div><div class="cand-worker">${d.worker ? shortId(d.worker) : ""}</div>`;
      } else {
        div.className = "candidate done-composite";
        div.innerHTML = `<div class="cand-p">2<sup>${p}</sup>\u22121 \u2717</div><div class="cand-dur">${d.duration_ms != null ? fmtDuration(d.duration_ms) : ""}</div>`;
      }
      candidatesDiv.appendChild(div);
    }
  }
  function renderWorkers(workers, totalWorkers) {
    if (!workers.length) {
      workersList.innerHTML = `<div id="workers-empty">No workers yet \u2014 open a tab and connect</div>`;
      return;
    }
    workersList.innerHTML = "";
    const totalCores = workers.reduce((s, w) => s + w.cpu_cores, 0);
    const totalDone = workers.reduce((s, w) => s + w.items_done, 0);
    const summary = document.createElement("div");
    summary.className = "worker-summary";
    summary.innerHTML = `<span>${workers.length} worker${workers.length !== 1 ? "s" : ""} connected</span><span>${totalCores} total cores \xB7 ${totalDone} items processed</span>`;
    workersList.appendChild(summary);
    for (const w of workers) {
      const isMe = w.actor_id === myActorId;
      const busy = w.current_p != null;
      const throughputPerS = w.total_duration_ms > 0 ? (w.items_done / w.total_duration_ms * 1e3).toFixed(1) : "\u2014";
      const card = document.createElement("div");
      card.className = `worker-card${isMe ? " mine" : ""}`;
      card.innerHTML = `
      <div class="worker-name">
        <span>${isMe ? "\u2605 " : ""}${shortId(w.actor_id)}</span>
        <span class="badge ${busy ? "" : "idle"}">${busy ? `\u2699 2^${w.current_p}` : "idle"}</span>
      </div>
      <div class="worker-stats">
        <div class="wstat">Cores <span>${w.cpu_cores}</span></div>
        <div class="wstat">Done <span>${w.items_done}</span></div>
        <div class="wstat">Avg <span>${w.avg_duration_ms > 0 ? fmtDuration(w.avg_duration_ms) : "\u2014"}</span></div>
        <div class="wstat">Throughput <span>${throughputPerS}/s</span></div>
        <div class="wstat">Status <span>${w.idle_ms < 5e3 ? "active" : "idle " + fmtDuration(w.idle_ms)}</span></div>
      </div>`;
      workersList.appendChild(card);
    }
  }
  function applyStatus(status) {
    const total = status.total ?? 0;
    const completed = status.completed ?? 0;
    const assigned = status.assigned ?? 0;
    const pending = status.pending ?? 0;
    const found = status.found_primes ?? [];
    const pct = total > 0 ? Math.round(completed / total * 100) : 0;
    const workers = status.worker_details ?? [];
    const throughput = status.throughput_per_s ?? 0;
    progressBarInner.style.width = `${pct}%`;
    mProgress.textContent = `${completed}/${total}`;
    mPct.textContent = `${pct}%`;
    mPrimes.textContent = String(found.length);
    mWorkers.textContent = String(status.workers_active ?? workers.length);
    const totalCores = workers.reduce((s, w) => s + w.cpu_cores, 0);
    mCores.textContent = totalCores > 0 ? `${totalCores} total cores` : "\u2014";
    mThroughput.textContent = throughput > 0 ? throughput.toFixed(2) : "\u2014";
    if (status.speedup != null) {
      const eff = status.efficiency_pct != null ? ` (${status.efficiency_pct}% eff)` : "";
      mSpeedup.textContent = `${status.speedup.toFixed(2)}x${eff}`;
    } else {
      mSpeedup.textContent = "\u2014";
    }
    progDetail.textContent = `${completed} done \xB7 ${assigned} active \xB7 ${pending} pending`;
    if (status.started_at && status.started_at > 0)
      startedAt = status.started_at;
    const runComplete = total > 0 && completed >= total && assigned === 0;
    if (runComplete && isRunning) {
      isRunning = false;
      mStatus.textContent = "Complete \u2713";
      if (pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
      setButtonState(true, false);
      log2(`Done! Found ${found.length} primes in ${fmtDuration(status.elapsed_ms ?? 0)}.`, "prime");
    }
    if (status.candidates_detail)
      renderCandidates(status.candidates_detail, total);
    renderWorkers(workers, status.workers_active ?? workers.length);
  }
  function spawnWorker(code) {
    const blob = new Blob([code], { type: "application/javascript" });
    const url = URL.createObjectURL(blob);
    const w = new Worker(url, { type: "classic" });
    URL.revokeObjectURL(url);
    return w;
  }
  connectBtn.addEventListener("click", async () => {
    const wsUrl = wsUrlInput.value.trim();
    const jwtToken = jwtInput.value.trim() || void 0;
    const leaderNodeId = leaderNodeInput.value.trim() || "test-node-8091";
    if (!wsUrl) {
      log2("Please enter a WebSocket URL", "error");
      return;
    }
    connectBtn.disabled = true;
    mStatus.textContent = "Connecting\u2026";
    try {
      client = new WsThinClient({ wsUrl, jwtToken, nodeId: WsThinClient.newUlid(), namespace: APP_NS });
      const nodeId = await client.connect();
      const myCores = navigator.hardwareConcurrency ?? 1;
      mNode.textContent = nodeId.slice(-10);
      mMyCores.textContent = `${myCores} cores`;
      log2(`Connected as \u2026${nodeId.slice(-8)} (${myCores} cores)`);
      myActorId = client.localActorId(nodeId.slice(-8), "WorkerNode", APP_NS);
      coordinatorId = new ActorID("mersenne", "CoordinatorActor", APP_NS, leaderNodeId).toString();
      const codeServerId = new ActorID("compute-js", "CodeServerActor", APP_NS, leaderNodeId).toString();
      const codeResp = await client.ask(codeServerId, "get_worker_js", {}, 1e4);
      if (!codeResp?.code)
        throw new Error("CodeServerActor returned empty worker JS");
      log2("Lucas-Lehmer worker JS received");
      worker = spawnWorker(codeResp.code);
      worker.onmessage = (e) => {
        const { p, is_prime, duration_ms } = e.data;
        log2(
          is_prime ? `2^${p}\u22121 PRIME \u2713 (${fmtDuration(duration_ms)})` : `2^${p}\u22121 composite (${fmtDuration(duration_ms)})`,
          is_prime ? "prime" : "info"
        );
        client?.tell(coordinatorId, "result", { p, is_prime, duration_ms, actor_id: myActorId }).catch(() => {
        });
      };
      client.onMessage((_from, msgType, payload) => {
        if (msgType === "assign_work") {
          const pw = payload;
          if (pw.done || pw.p == null) {
            log2("No more work for this worker");
            return;
          }
          log2(`Assigned: 2^${pw.p}\u22121`, "assign");
          worker?.postMessage({ p: pw.p });
        }
      });
      await client.ask(coordinatorId, "join", { actor_id: myActorId, cpu_cores: myCores }, 1e4);
      log2(`Joined coordinator as worker (${myCores} cores) \u2014 click "Start New Run" to begin`);
      mStatus.textContent = "Idle (connected)";
      try {
        const status = await client.ask(coordinatorId, "status", {}, 5e3);
        applyStatus(status);
      } catch {
      }
      setButtonState(true, false);
      startElapsedTimer();
    } catch (err) {
      log2(`Error: ${err.message}`, "error");
      mStatus.textContent = "Error";
      connectBtn.disabled = false;
      client = null;
    }
  });
  startRunBtn.addEventListener("click", async () => {
    if (!client)
      return;
    const count = Math.min(parseInt(candidateCountInput.value) || 20, CANDIDATES.length);
    setButtonState(true, true);
    isRunning = true;
    mStatus.textContent = "Running\u2026";
    startedAt = Date.now();
    try {
      const startResp = await client.ask(coordinatorId, "start", { count }, 1e4);
      log2(`Run started \u2014 ${startResp.total ?? count} candidates, ${startResp.workers_dispatched ?? "?"} workers`);
    } catch (err) {
      log2(`Start failed: ${err.message}`, "error");
      isRunning = false;
      setButtonState(true, false);
      return;
    }
    if (pollTimer)
      clearInterval(pollTimer);
    pollTimer = setInterval(async () => {
      if (!client)
        return;
      try {
        const status = await client.ask(coordinatorId, "status", {}, 5e3);
        applyStatus(status);
      } catch {
      }
    }, 1500);
  });
  disconnectBtn.addEventListener("click", async () => {
    if (pollTimer) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
    if (elapsedTimer) {
      clearInterval(elapsedTimer);
      elapsedTimer = null;
    }
    if (client && myActorId) {
      client.tell(coordinatorId, "leave", { actor_id: myActorId }).catch(() => {
      });
    }
    worker?.terminate();
    worker = null;
    await client?.disconnect();
    client = null;
    isRunning = false;
    mStatus.textContent = "Disconnected";
    setButtonState(false, false);
    log2("Disconnected");
  });
})();
