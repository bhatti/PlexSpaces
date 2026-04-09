// ../../../../sdks/typescript/dist/actor.js
import { log as hostLog } from "plexspaces:actor/host@0.1.0";

// ../../../../sdks/typescript/dist/decorators.js
var ACTOR_METADATA = Symbol.for("plexspaces.actor.metadata");
function getActorDefinition(target) {
  const ctor = typeof target === "function" ? target : target.constructor;
  return Reflect.get(ctor, ACTOR_METADATA);
}

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
function actorLog(level, location, message, extra) {
  try {
    if (typeof hostLog === "function") {
      const entry = extra ? `[${location}] ${message} ${extra}` : `[${location}] ${message}`;
      hostLog(level, entry);
    }
  } catch {
  }
}
var PlexSpacesActor = class {
  constructor() {
    this.cachedStateJson = null;
    this.state = this.getDefaultState();
    this.cachedStateJson = null;
  }
  /** Optional: called from init() with parsed config. Override to apply config to state. */
  onInit(_config) {
  }
  /**
   * WIT `init(config: payload) -> result<_, actor-error>`.
   * Success: return (unit). Failure: throw (jco maps throws to `err` for function-return `result`).
   */
  init(configJson) {
    try {
      const text = decodeWitPayloadUtf8(configJson);
      const config = text.trim() ? JSON.parse(text) : {};
      this.onInit(config);
      this.cachedStateJson = null;
    } catch {
      throw new Error("ERROR:init failed");
    }
  }
  /**
   * WIT `handle(...) -> result<payload, actor-error>` (`payload` is `list<u8>` → `Uint8Array` in jco).
   * Dispatches by msgType for Workflow behavior (workflow_run, workflow_signal:name, workflow_query:name),
   * then by payload.op (or payload) to on<Op>(payload). Returns UTF-8 JSON bytes.
   * Uses iterative serializer to avoid WASM recursion.
   *
   * Workflow behavior (aligned with Rust Workflow trait and Python @workflow_actor):
   * - msgType "workflow_run" -> run(payload)
   * - msgType "workflow_signal:name" -> signal(name, payload)
   * - msgType "workflow_query:name" -> query(name, payload)
   */
  handle(_fromActor, msgType, payloadJson) {
    try {
      const text = decodeWitPayloadUtf8(payloadJson);
      const payload = text.trim() ? JSON.parse(text) : {};
      const definition = getActorDefinition(this);
      if (msgType === "workflow_run") {
        const runMethod = definition?.runHandler;
        const runFn = runMethod ? this[runMethod] : this.run;
        if (typeof runFn === "function") {
          const result = runFn.call(this, payload);
          this.cachedStateJson = null;
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        }
      }
      if (msgType.startsWith("workflow_signal:")) {
        const name = msgType.slice("workflow_signal:".length).trim();
        const signalMethod = definition?.signalHandlers?.[name];
        const signalFn = signalMethod ? this[signalMethod] : this.signal;
        if (typeof signalFn === "function") {
          if (signalMethod) {
            signalFn.call(this, payload);
          } else {
            signalFn.call(this, name, payload);
          }
          this.cachedStateJson = null;
          return encodeWitPayloadUtf8("{}");
        }
      }
      if (msgType.startsWith("workflow_query:")) {
        const name = msgType.slice("workflow_query:".length).trim();
        const queryMethod = definition?.queryHandlers?.[name];
        const queryFn = queryMethod ? this[queryMethod] : this.query;
        if (typeof queryFn === "function") {
          const result = queryMethod ? queryFn.call(this, payload) : queryFn.call(this, name, payload);
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        }
      }
      const opRaw = payload.message_type ?? payload.op ?? payload.msg_type;
      const op = typeof opRaw === "string" && opRaw ? opRaw : msgType;
      const decoratedMethod = this.resolveDecoratedHandler(op, definition);
      const opKey = typeof op === "string" ? this.capitalize(op) : "";
      const methodName = decoratedMethod ?? (opKey ? `on${opKey}` : "");
      const method = methodName && typeof this[methodName] === "function" ? this[methodName] : null;
      if (method) {
        let result;
        try {
          result = method.call(this, payload);
        } catch (handlerError) {
          const errorMsg = handlerError instanceof Error ? handlerError.message : String(handlerError);
          actorLog("error", "actor.ts:handle", `Handler ${methodName} failed`, errorMsg);
          throw new Error("ERROR:" + errorMsg);
        }
        this.cachedStateJson = null;
        try {
          return encodeWitPayloadUtf8(iterativeStringify(result ?? {}));
        } catch (jsonError) {
          const errorMsg = jsonError instanceof Error ? jsonError.message : String(jsonError);
          actorLog("error", "actor.ts:handle", "JSON serialization failed", errorMsg);
          throw new Error("ERROR:JSON serialization failed: " + errorMsg);
        }
      }
      actorLog("warn", "actor.ts:handle", "Unknown operation", String(op));
      return encodeWitPayloadUtf8(iterativeStringify({ error: "unknown_op", op: String(op) }));
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      actorLog("error", "actor.ts:handle", "Handle failed", errorMsg);
      if (e instanceof Error && errorMsg.startsWith("ERROR:")) {
        throw e;
      }
      throw new Error("ERROR:" + errorMsg);
    }
  }
  /** WIT `get-state() -> result<payload, actor-error>`. Returns JSON state as UTF-8 bytes. */
  getState() {
    if (this.cachedStateJson !== null) {
      return encodeWitPayloadUtf8(this.cachedStateJson);
    }
    try {
      const serialized = iterativeStringify(this.state);
      this.cachedStateJson = serialized;
      return encodeWitPayloadUtf8(serialized);
    } catch {
      return encodeWitPayloadUtf8("{}");
    }
  }
  /** WIT `set-state(state: payload) -> result<_, actor-error>`. */
  setState(stateJson) {
    try {
      const text = decodeWitPayloadUtf8(stateJson);
      if (text.trim()) {
        this.state = JSON.parse(text);
        this.cachedStateJson = null;
      }
    } catch {
      throw new Error("ERROR:set_state failed");
    }
  }
  capitalize(s) {
    if (!s)
      return "";
    return s.charAt(0).toUpperCase() + s.slice(1);
  }
  resolveDecoratedHandler(op, definition = getActorDefinition(this)) {
    if (!definition)
      return null;
    return definition.handlers[op]?.methodName ?? null;
  }
  /**
   * Serialize object to JSON string using fully iterative approach (zero recursion).
   *
   * jco componentize compiles JS to WASM (StarlingMonkey) with a tiny call stack.
   * Native JSON.stringify recurses per-element and per-nesting-level, hitting
   * stack limits with arrays of 2+ items. This iterative serializer uses a work
   * stack instead of recursive function calls.
   */
  json(obj) {
    return iterativeStringify(obj);
  }
  error(message) {
    return "ERROR:" + message;
  }
};
var WorkflowActor = class extends PlexSpacesActor {
};
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
function escapeStr(s) {
  let out = '"';
  for (let i = 0, len = s.length; i < len; i++) {
    const c = s.charCodeAt(i);
    if (c < 128) {
      const esc = ESCAPE_TABLE[c];
      if (esc) {
        out += esc;
      } else {
        out += String.fromCharCode(c);
      }
    } else {
      out += String.fromCharCode(c);
    }
  }
  out += '"';
  return out;
}
var TAG_VALUE = 0;
var TAG_LITERAL = 1;
function iterativeStringify(root) {
  const stackTags = [];
  const stackPayloads = [];
  let sp = 0;
  stackTags[0] = TAG_VALUE;
  stackPayloads[0] = root;
  sp = 1;
  const fragments = [];
  let fragCount = 0;
  while (sp > 0) {
    sp--;
    const tag = stackTags[sp];
    const payload = stackPayloads[sp];
    stackPayloads[sp] = null;
    if (tag === TAG_LITERAL) {
      fragments[fragCount++] = payload;
      continue;
    }
    if (payload === null || payload === void 0) {
      fragments[fragCount++] = "null";
      continue;
    }
    const t = typeof payload;
    if (t === "string") {
      fragments[fragCount++] = escapeStr(payload);
      continue;
    }
    if (t === "number") {
      fragments[fragCount++] = "" + payload;
      continue;
    }
    if (t === "boolean") {
      fragments[fragCount++] = payload ? "true" : "false";
      continue;
    }
    if (t === "function") {
      fragments[fragCount++] = "null";
      continue;
    }
    const obj = payload;
    const len = obj["length"];
    const isArr = typeof len === "number" && len >= 0 && len >>> 0 === len;
    if (isArr) {
      const arr = payload;
      const arrLen = arr.length;
      if (arrLen === 0) {
        fragments[fragCount++] = "[]";
        continue;
      }
      stackTags[sp] = TAG_LITERAL;
      stackPayloads[sp] = "]";
      sp++;
      for (let i = arrLen - 1; i >= 0; i--) {
        stackTags[sp] = TAG_VALUE;
        stackPayloads[sp] = arr[i];
        sp++;
        if (i > 0) {
          stackTags[sp] = TAG_LITERAL;
          stackPayloads[sp] = ",";
          sp++;
        }
      }
      stackTags[sp] = TAG_LITERAL;
      stackPayloads[sp] = "[";
      sp++;
      continue;
    }
    let keys = [];
    try {
      const allProps = Object.getOwnPropertyNames(obj);
      for (let i = 0; i < allProps.length; i++) {
        const k = allProps[i];
        const v = obj[k];
        if (v !== void 0 && typeof v !== "function") {
          keys.push(k);
        }
      }
    } catch {
      fragments[fragCount++] = "{}";
      continue;
    }
    if (keys.length === 0) {
      fragments[fragCount++] = "{}";
      continue;
    }
    stackTags[sp] = TAG_LITERAL;
    stackPayloads[sp] = "}";
    sp++;
    for (let i = keys.length - 1; i >= 0; i--) {
      stackTags[sp] = TAG_VALUE;
      stackPayloads[sp] = obj[keys[i]];
      sp++;
      stackTags[sp] = TAG_LITERAL;
      stackPayloads[sp] = escapeStr(keys[i]) + ":";
      sp++;
      if (i > 0) {
        stackTags[sp] = TAG_LITERAL;
        stackPayloads[sp] = ",";
        sp++;
      }
    }
    stackTags[sp] = TAG_LITERAL;
    stackPayloads[sp] = "{";
    sp++;
  }
  let result = "";
  for (let i = 0; i < fragCount; i++) {
    result += fragments[i];
  }
  return result;
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
    const enc = new TextEncoder();
    const bytes = new Uint8Array(enc.encode(v));
    let inner = new Uint8Array([26]);
    inner = appendVarint(inner, bytes.length);
    inner = concatBytes(inner, bytes);
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
function utf8Valid(bytes) {
  try {
    new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    return true;
  } catch {
    return false;
  }
}
function bytesToBase64Sync(bytes) {
  let bin = "";
  for (let i = 0; i < bytes.length; i++)
    bin += String.fromCharCode(bytes[i]);
  if (typeof btoa !== "undefined")
    return btoa(bin);
  const Buf = globalThis.Buffer;
  if (Buf)
    return Buf.from(bytes).toString("base64");
  throw new Error("base64 encode unavailable");
}
function encodeHttpFetchRequestWire(headers, body) {
  let buf = new Uint8Array(0);
  const enc = new TextEncoder();
  for (const [k, v] of Object.entries(headers)) {
    const kb = new Uint8Array(enc.encode(k));
    const vb = new Uint8Array(enc.encode(v));
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

// ../../../../sdks/typescript/dist/host.js
import {
  send as hostSend,
  ask as hostAsk,
  log as hostLog2,
  nowMs as hostNowMs,
  selfId as hostSelfId,
  spawn as hostSpawn,
  stop as hostStop,
  link as hostLink,
  unlink as hostUnlink,
  monitor as hostMonitor,
  demonitor as hostDemonitor,
  sendAfter as hostSendAfter,
  kvGet as hostKvGet,
  kvPut as hostKvPut,
  kvDelete as hostKvDelete,
  kvList as hostKvList,
  tsWrite as hostTsWrite,
  tsRead as hostTsRead,
  tsTake as hostTsTake,
  tsReadAll as hostTsReadAll,
  lockAcquire as hostLockAcquire,
  lockRelease as hostLockRelease,
  lockRenew as hostLockRenew,
  blobUpload as hostBlobUpload,
  blobDownload as hostBlobDownload,
  blobDelete as hostBlobDelete,
  blobList as hostBlobList,
  pgJoin as hostPgJoin,
  pgLeave as hostPgLeave,
  pgMembers as hostPgMembers,
  pgBroadcast as hostPgBroadcast,
  poolCheckout as hostPoolCheckout,
  poolCheckin as hostPoolCheckin,
  poolGetMetrics as hostPoolGetMetrics,
  createShardGroup as hostCreateShardGroup,
  bulkUpdateShardGroup as hostBulkUpdateShardGroup,
  mapShardGroup as hostMapShardGroup,
  scatterGather as hostScatterGather,
  broadcastShardGroup as hostBroadcastShardGroup,
  reduceShardGroup as hostReduceShardGroup,
  allReduceShardGroup as hostAllReduceShardGroup,
  barrierShardGroup as hostBarrierShardGroup,
  spawnActors as hostSpawnActors,
  applicationMetricsAdd as hostApplicationMetricsAdd,
  applicationGetStatus as hostApplicationGetStatus,
  httpFetch as hostHttpFetch
} from "plexspaces:actor/host@0.1.0";
function safeCall(fn, ...args) {
  if (typeof fn === "function") {
    return fn(...args);
  }
  return "";
}
function hostPayloadToBytes(result) {
  if (result instanceof Uint8Array)
    return result;
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
    const result = safeCall(hostPgJoin, group);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Leave a named process group */
  leave(group) {
    const result = safeCall(hostPgLeave, group);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Get members of a process group */
  members(group) {
    const result = safeCall(hostPgMembers, group);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
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
    const payloadJson = payload !== void 0 ? JSON.stringify(payload) : "{}";
    const result = safeCall(hostPgBroadcast, group, msgType, payloadJson);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
};
var Host = class {
  constructor() {
    this.processGroups = new ProcessGroups();
    this.ts = new TupleSpace(this);
  }
  // ========================================================================
  // Messaging
  // ========================================================================
  /** Send message to another actor (fire-and-forget) */
  send(to, msgType, payload) {
    const payloadJson = payload !== void 0 ? JSON.stringify(payload) : "";
    const raw = safeCall(hostSend, to, msgType, payloadJson);
    if (typeof raw !== "string") {
      return "";
    }
    return raw;
  }
  /** Send request and wait for response (request-reply) */
  ask(to, msgType, payload, timeoutMs = 5e3) {
    const payloadJson = payload !== void 0 ? JSON.stringify(payload) : "";
    const result = safeCall(hostAsk, to, msgType, payloadJson, BigInt(timeoutMs));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
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
    return safeCall(hostSelfId);
  }
  // ========================================================================
  // Actor Lifecycle
  // ========================================================================
  /**
   * Spawn a new actor through the framework-owned actor spawn path exposed by the host.
   * @param moduleRef - Actor type/module reference (must be deployed)
   * @param actorId - Unique ID for the new actor (empty = auto-generated ULID)
   * @param initConfig - Optional config passed to the new actor's init()
   * @returns Spawned actor ID string (may be auto-generated if actorId was empty)
   */
  spawn(moduleRef, actorId = "", initConfig) {
    const configJson = initConfig !== void 0 ? JSON.stringify(initConfig) : "{}";
    const result = safeCall(hostSpawn, moduleRef, actorId, configJson);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Stop an actor gracefully */
  stop(actorId) {
    const result = safeCall(hostStop, actorId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  // ========================================================================
  // Actor Linking & Monitoring
  // ========================================================================
  /** Bidirectional link */
  link(actorId) {
    const result = safeCall(hostLink, actorId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Remove bidirectional link */
  unlink(actorId) {
    const result = safeCall(hostUnlink, actorId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Monitor an actor (returns monitor reference) */
  monitor(actorId) {
    const result = safeCall(hostMonitor, actorId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Cancel a monitor */
  demonitor(monitorRef) {
    const result = safeCall(hostDemonitor, monitorRef);
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
    const raw = safeCall(hostSendAfter, BigInt(delayMs), msgType, payloadBytes);
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
    safeCall(hostLog2, level, message);
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
    const result = safeCall(hostNowMs);
    return typeof result === "bigint" ? Number(result) : typeof result === "number" ? result : 0;
  }
  // ========================================================================
  // Key-Value Store
  // ========================================================================
  kvGet(key) {
    return safeCall(hostKvGet, key);
  }
  kvPut(key, value) {
    return safeCall(hostKvPut, key, value);
  }
  kvDelete(key) {
    return safeCall(hostKvDelete, key);
  }
  kvList(prefix) {
    return safeCall(hostKvList, prefix);
  }
  // ========================================================================
  // TupleSpace (protobuf WriteRequest / ReadRequest / ReadResponse wire bytes)
  // ========================================================================
  /** @internal TupleSpace — plexspaces.tuplespace.v1 wire bytes. */
  tsWritePayload(data) {
    const r = safeCall(hostTsWrite, data);
    return typeof r === "string" ? r : "";
  }
  /** @internal */
  tsReadPayload(data) {
    return hostPayloadToBytes(safeCall(hostTsRead, data));
  }
  /** @internal */
  tsTakePayload(data) {
    return hostPayloadToBytes(safeCall(hostTsTake, data));
  }
  /** @internal */
  tsReadAllPayload(data) {
    return hostPayloadToBytes(safeCall(hostTsReadAll, data));
  }
  // ========================================================================
  // Distributed Locks
  // ========================================================================
  lockAcquire(tenantId, namespace, holderId, lockName, leaseDurationSecs = 30, timeoutMs = 0) {
    return safeCall(hostLockAcquire, tenantId, namespace, holderId, lockName, leaseDurationSecs, BigInt(timeoutMs));
  }
  lockRelease(lockId, tenantId, namespace, holderId, lockVersion) {
    return safeCall(hostLockRelease, lockId, tenantId, namespace, holderId, lockVersion);
  }
  lockRenew(lockId, tenantId, namespace, holderId, lockVersion, leaseDurationSecs = 30) {
    return safeCall(hostLockRenew, lockId, tenantId, namespace, holderId, lockVersion, leaseDurationSecs);
  }
  // ========================================================================
  // Blob Storage
  // ========================================================================
  blobUpload(blobId, data, contentType = "application/octet-stream") {
    return safeCall(hostBlobUpload, blobId, data, contentType);
  }
  blobDownload(blobId) {
    return safeCall(hostBlobDownload, blobId);
  }
  blobDelete(blobId) {
    return safeCall(hostBlobDelete, blobId);
  }
  blobList(prefix) {
    return safeCall(hostBlobList, prefix);
  }
  // ========================================================================
  // Elastic pool (checkout/checkin)
  // ========================================================================
  /**
   * Checkout an actor from a named pool. Returns handle { actor_id, pool_name, checkout_id } or null on failure.
   */
  poolCheckout(poolName, timeoutMs = 5e3) {
    const result = safeCall(hostPoolCheckout, poolName, BigInt(timeoutMs));
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
    const result = safeCall(hostPoolCheckin, poolName, actorId, checkoutId, healthy);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /**
   * Get pool metrics (total_actors, available_actors, busy_actors, current_load, etc.). Returns null if not available.
   */
  poolGetMetrics(poolName) {
    const result = safeCall(hostPoolGetMetrics, poolName);
    if (typeof result !== "string" || result === "" || result.startsWith("ERROR:"))
      return null;
    try {
      return JSON.parse(result);
    } catch {
      return null;
    }
  }
  createShardGroup(request) {
    const result = safeCall(hostCreateShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  bulkUpdateShardGroup(request) {
    const result = safeCall(hostBulkUpdateShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  mapShardGroup(request) {
    const result = safeCall(hostMapShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  scatterGather(request) {
    const result = safeCall(hostScatterGather, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  broadcastShardGroup(request) {
    const result = safeCall(hostBroadcastShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  reduceShardGroup(request) {
    const result = safeCall(hostReduceShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  allReduceShardGroup(request) {
    const result = safeCall(hostAllReduceShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  barrierShardGroup(request) {
    const result = safeCall(hostBarrierShardGroup, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  spawnActors(request) {
    const result = safeCall(hostSpawnActors, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  applicationMetricsAdd(applicationId, metrics) {
    const result = safeCall(hostApplicationMetricsAdd, applicationId, JSON.stringify(metrics));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  applicationGetStatus(applicationId, nodeId) {
    const result = safeCall(hostApplicationGetStatus, applicationId, nodeId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
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
    const result = safeCall(hostHttpFetch, linkName, method, pathAndQuery, reqWire);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) {
      return { status: 0, headers: {}, body: "" };
    }
    if (hostErrorPrefixBytes(bytes)) {
      throw new Error(new TextDecoder("utf-8", { fatal: false }).decode(bytes));
    }
    const asText = new TextDecoder("utf-8", { fatal: false }).decode(bytes);
    try {
      return JSON.parse(asText);
    } catch {
      return decodeHttpFetchResponseWire(bytes);
    }
  }
};
var host = new Host();

// stream_actor.ts
var DEFAULT_WINDOW_SIZE = 10;
var COMPUTE_MS_PER_EVENT = 0.2;
var WindowedStreamActor = class extends WorkflowActor {
  getDefaultState() {
    return {
      stream_id: "",
      window_size: DEFAULT_WINDOW_SIZE,
      window: [],
      processed_count: 0,
      windows_emitted: 0,
      status: "idle",
      total_compute_ms: 0,
      total_coord_ms: 0,
      created_at_ms: 0,
      updated_at_ms: 0,
      cancel_requested: false
    };
  }
  onInit(config) {
    this.state.stream_id = String(config.stream_id ?? this.state.stream_id);
    const ws = config.window_size;
    if (typeof ws === "number" && ws > 0) this.state.window_size = ws;
    this.state.created_at_ms = host.nowMs();
    this.state.updated_at_ms = this.state.created_at_ms;
  }
  /** Main workflow: process event batch, window aggregation, return metrics. */
  run(payload) {
    const t0 = host.nowMs();
    this.state.stream_id = String(payload.stream_id ?? this.state.stream_id);
    const windowSize = Number(payload.window_size) || this.state.window_size;
    if (windowSize > 0) this.state.window_size = windowSize;
    this.state.updated_at_ms = host.nowMs();
    if (this.state.cancel_requested) {
      this.state.status = "cancelled";
      return this.finish(t0, 0, "cancelled");
    }
    const events = payload.events ?? [];
    if (events.length === 0) {
      return this.finish(t0, 0, this.state.status || "idle");
    }
    this.state.status = "running";
    let computeMs = 0;
    for (const ev of events) {
      if (this.state.cancel_requested) {
        this.state.status = "cancelled";
        return this.finish(t0, computeMs, "cancelled");
      }
      this.state.window.push(ev);
      this.state.processed_count += 1;
      computeMs += COMPUTE_MS_PER_EVENT;
      if (this.state.window.length >= this.state.window_size) {
        const agg = this.emitWindow();
        this.state.windows_emitted += 1;
        agg.window_index = this.state.windows_emitted;
      }
    }
    this.state.updated_at_ms = host.nowMs();
    this.state.status = "idle";
    return this.finish(t0, computeMs, "idle");
  }
  signal(name, _data) {
    if (name === "cancel") {
      this.state.cancel_requested = true;
      this.state.updated_at_ms = host.nowMs();
    }
  }
  query(name, _params) {
    if (name === "status") {
      return {
        stream_id: this.state.stream_id,
        status: this.state.status,
        window_size: this.state.window_size,
        window_count: this.state.window.length,
        processed_count: this.state.processed_count,
        windows_emitted: this.state.windows_emitted,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        created_at_ms: this.state.created_at_ms,
        updated_at_ms: this.state.updated_at_ms,
        cancel_requested: this.state.cancel_requested
      };
    }
    return { error: "unknown_query", name };
  }
  /** Add single event (handler). */
  onIngest(payload) {
    const ev = {
      event_id: String(payload.event_id ?? host.nowMs()),
      value: Number(payload.value ?? 0),
      ts: Number(payload.ts ?? host.nowMs()),
      ...payload
    };
    this.state.window.push(ev);
    this.state.processed_count += 1;
    this.state.updated_at_ms = host.nowMs();
    return { ok: true, window_count: this.state.window.length };
  }
  /** Emit aggregate for current window and clear (handler). */
  onWindow_flush(_payload) {
    const agg = this.emitWindow();
    this.state.windows_emitted += 1;
    this.state.updated_at_ms = host.nowMs();
    return { ok: true, aggregate: agg, windows_emitted: this.state.windows_emitted };
  }
  emitWindow() {
    const w = this.state.window;
    const values = w.map((e) => typeof e.value === "number" ? e.value : 0);
    const sum = values.reduce((a, b) => a + b, 0);
    const count = values.length;
    const avg = count > 0 ? sum / count : 0;
    const min = count > 0 ? Math.min(...values) : 0;
    const max = count > 0 ? Math.max(...values) : 0;
    this.state.window = [];
    return { count, sum, avg, min, max };
  }
  finish(t0, computeMs, status) {
    const elapsed = host.nowMs() - t0;
    const coordMs = Math.max(0, elapsed - computeMs);
    this.state.total_compute_ms += computeMs;
    this.state.total_coord_ms += coordMs;
    this.state.status = status;
    this.state.updated_at_ms = host.nowMs();
    return {
      status: this.state.status,
      stream_id: this.state.stream_id,
      processed_count: this.state.processed_count,
      windows_emitted: this.state.windows_emitted,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms
    };
  }
};
var actorInstance = new WindowedStreamActor();
var actor2 = {
  init: (configJson) => actorInstance.init(configJson),
  handle: (from, msgType, payloadJson) => actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson) => actorInstance.setState(stateJson)
};
export {
  WindowedStreamActor,
  actor2 as actor
};
