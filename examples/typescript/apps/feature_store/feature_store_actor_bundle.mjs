// node_modules/@plexspaces/sdk/dist/actor.js
import { log as hostLog } from "plexspaces:actor/host-logging@0.1.0";

// node_modules/@plexspaces/sdk/dist/decorators.js
var ACTOR_METADATA = Symbol.for("plexspaces.actor.metadata");
function getActorDefinition(target) {
  const ctor = typeof target === "function" ? target : target.constructor;
  return Reflect.get(ctor, ACTOR_METADATA);
}

// node_modules/@plexspaces/sdk/dist/wit-payload.js
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

// node_modules/@plexspaces/sdk/dist/actor.js
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

// node_modules/@plexspaces/sdk/dist/wire/proto-wire-common.js
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

// node_modules/@plexspaces/sdk/dist/wire/tuplespace-proto-wire.js
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
    const bytes = new Uint8Array(enc3.encode(v));
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
  return appendLengthDelimited(new Uint8Array(0), 2, tupleBody);
}
function encodeReadRequest(pattern, take, maxResults) {
  const templateBody = encodeTupleFields(pattern, true);
  let out = appendLengthDelimited(new Uint8Array(0), 2, templateBody);
  if (take) {
    out = concatBytes(out, new Uint8Array([40, 1]));
  }
  out = concatBytes(out, new Uint8Array([48]));
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
    if (fn === 2 && wt === 2) {
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

// node_modules/@plexspaces/sdk/dist/wire/http-fetch-proto-wire.js
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

// node_modules/@plexspaces/sdk/dist/wire/shard-group-proto-wire.js
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
  const op = query["op"] || query["message_type"] || "call";
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 5, op);
  const payloadObj = { ...query, message_type: op };
  const payloadBytes = new Uint8Array(encW.encode(JSON.stringify(payloadObj)));
  buf = appendBytes(buf, 6, payloadBytes);
  return buf;
}
function encodeCreateShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  const cfgFields = {
    group_id: req.group_id,
    shard_count: req.shard_count,
    partition_strategy: req.partition_strategy,
    rebalance_policy: req.rebalance_policy,
    placement: req.placement
  };
  const cfgBytes = encodeDataParallelConfig(cfgFields);
  buf = appendLengthDelimited(buf, 2, cfgBytes);
  buf = appendString(buf, 3, req.actor_type ?? "");
  const initialState = req.initial_state;
  if (initialState !== void 0 && initialState !== null) {
    const stateBytes = new Uint8Array(encW.encode(JSON.stringify(initialState)));
    if (stateBytes.length > 0) {
      buf = appendBytes(buf, 5, stateBytes);
    }
  }
  const metadata = req.metadata;
  if (metadata && typeof metadata === "object") {
    for (const [k, v] of Object.entries(metadata)) {
      let entry = new Uint8Array(0);
      entry = appendString(entry, 1, k);
      entry = appendString(entry, 2, String(v));
      buf = appendLengthDelimited(buf, 6, entry);
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
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 2, req.group_id ?? "");
  const query = req.query;
  if (query && typeof query === "object") {
    const msgBytes = encodeMessage(query);
    buf = appendLengthDelimited(buf, 3, msgBytes);
  }
  const timeoutMs = Number(req.timeout_ms ?? 3e4);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 4, durBytes);
  }
  const agg = aggregationStrategyEnum(req.aggregation);
  if (agg !== 0)
    buf = appendUint32(buf, 5, agg);
  const minResponses = Number(req.min_responses ?? 0) >>> 0;
  if (minResponses > 0)
    buf = appendUint32(buf, 6, minResponses);
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
    if (fn === 2 && wt === 2) {
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
function decodeScatterGatherStats(data) {
  const result = {
    shards_queried: 0,
    shards_responded: 0,
    shards_failed: 0,
    max_latency_ms: 0
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 1 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.shards_queried = value;
      pos = nextPos;
    } else if (fn === 2 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.shards_responded = value;
      pos = nextPos;
    } else if (fn === 3 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.shards_failed = value;
      pos = nextPos;
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return result;
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
    if (fn === 2 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.shard_id = value;
    } else if (fn === 3 && wt === 2) {
      const { value, nextPos } = readString(data, pos);
      pos = nextPos;
      result.shard_actor_id = value;
    } else if (fn === 4 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      result.payload = decodeMessagePayload(slice);
    } else if (fn === 6 && wt === 0) {
      const { value, nextPos } = readUint32(data, pos);
      pos = nextPos;
      result.success = value !== 0;
    } else if (fn === 7 && wt === 2) {
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
  let stats = null;
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 3 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      shardResponses.push(decodeShardQueryResponse(slice));
    } else if (fn === 4 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      stats = decodeScatterGatherStats(slice);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { shard_responses: shardResponses, stats: stats ?? {} };
}
function decodeBroadcastLikeResponse(data) {
  const shardResponses = [];
  let stats = null;
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
    } else if (fn === 3 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      stats = decodeScatterGatherStats(slice);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { shard_responses: shardResponses, stats: stats ?? {} };
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
function encodeBulkUpdateShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, req.request_id ?? "");
  buf = appendString(buf, 2, req.group_id ?? "");
  const rawUpdates = req.updates;
  let items;
  if (Array.isArray(rawUpdates)) {
    items = rawUpdates;
  } else if (rawUpdates && typeof rawUpdates === "object") {
    items = Object.entries(rawUpdates).map(([k, v]) => ({
      key: k,
      payload: v
    }));
  } else {
    items = [];
  }
  for (const entry of items) {
    const partitionKey = String(entry.key ?? "");
    const payload = entry.payload && typeof entry.payload === "object" ? entry.payload : {};
    const msgBytes = encodeMessage(payload);
    let mapEntry = new Uint8Array(0);
    mapEntry = appendString(mapEntry, 1, partitionKey);
    mapEntry = appendLengthDelimited(mapEntry, 2, msgBytes);
    buf = appendLengthDelimited(buf, 3, mapEntry);
  }
  const consistencyLevel = Number(req.consistency_level ?? 0) >>> 0;
  if (consistencyLevel !== 0)
    buf = appendUint32(buf, 4, consistencyLevel);
  const timeoutMs = Number(req.timeout_ms ?? 5e3);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 5, durBytes);
  }
  const waitForResponses = req.wait_for_responses !== false;
  if (waitForResponses) {
    buf = appendUint32(buf, 6, 1);
  }
  return buf;
}
function decodeBulkUpdateShardGroupResponse(data) {
  const result = {
    request_id: "",
    updates_sent: 0,
    updates_succeeded: 0,
    updates_failed: 0,
    shard_stats: [],
    errors: []
  };
  if (!data || data.length === 0)
    return result;
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
      result.shard_stats.push(decodeShardUpdateStats(slice));
      pos = nextPos;
    } else if (fieldNum === 6 && wireType === 2) {
      const { value, nextPos } = readString(data, pos);
      result.errors.push(value);
      pos = nextPos;
    } else {
      pos = skipField(data, pos, wireType);
    }
  }
  return result;
}
function decodeShardUpdateStats(data) {
  const result = {
    shard_id: 0,
    shard_actor_id: "",
    updates_sent: 0,
    updates_succeeded: 0,
    updates_failed: 0
  };
  if (!data || data.length === 0)
    return result;
  let pos = 0;
  while (pos < data.length) {
    const { value: tagVal, n: tagN } = readVarint(data, pos);
    pos += tagN;
    const fieldNum = Number(tagVal >> BigInt(3));
    const wireType = Number(tagVal & BigInt(7));
    if (fieldNum === 1 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.shard_id = value;
      pos = nextPos;
    } else if (fieldNum === 2 && wireType === 2) {
      const { value, nextPos } = readString(data, pos);
      result.shard_actor_id = value;
      pos = nextPos;
    } else if (fieldNum === 3 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_sent = value;
      pos = nextPos;
    } else if (fieldNum === 4 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_succeeded = value;
      pos = nextPos;
    } else if (fieldNum === 5 && wireType === 0) {
      const { value, nextPos } = readUint32(data, pos);
      result.updates_failed = value;
      pos = nextPos;
    } else {
      pos = skipField(data, pos, wireType);
    }
  }
  return result;
}
function collectiveReductionEnum(s) {
  switch ((s ?? "").toLowerCase()) {
    case "sum":
      return 1;
    case "min":
      return 2;
    case "max":
      return 3;
    case "product":
      return 4;
    case "concat":
      return 5;
    case "bool_and":
      return 6;
    case "bool_or":
      return 7;
    default:
      return 0;
  }
}
function encodeBroadcastShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 2, req.group_id ?? "");
  const message = req.message;
  if (message && typeof message === "object") {
    buf = appendLengthDelimited(buf, 3, encodeMessage(message));
  }
  const timeoutMs = Number(req.timeout_ms ?? 3e4);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 4, durBytes);
  }
  const minAcks = Number(req.min_acks ?? 0) >>> 0;
  if (minAcks > 0)
    buf = appendUint32(buf, 5, minAcks);
  return buf;
}
function encodeReduceShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 2, req.group_id ?? "");
  const mapFn = req.map_function;
  if (mapFn && typeof mapFn === "object") {
    buf = appendLengthDelimited(buf, 3, encodeMessage(mapFn));
  }
  const timeoutMs = Number(req.timeout_ms ?? 3e4);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 4, durBytes);
  }
  const minResponses = Number(req.min_responses ?? 0) >>> 0;
  if (minResponses > 0)
    buf = appendUint32(buf, 5, minResponses);
  const reduction = collectiveReductionEnum(req.reduction);
  if (reduction !== 0)
    buf = appendUint32(buf, 6, reduction);
  const target = req.target;
  if (target) {
    let targetField = new Uint8Array(0);
    targetField = appendString(targetField, 1, target);
    buf = appendLengthDelimited(buf, 7, targetField);
  }
  return buf;
}
function encodeAllReduceShardGroupRequest(req) {
  return encodeReduceShardGroupRequest(req);
}
function encodeBarrierShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 2, req.group_id ?? "");
  buf = appendString(buf, 3, req.barrier_id ?? "");
  const round = Number(req.round ?? 0);
  if (round > 0) {
    buf = appendVarint(buf, 4 << 3 | 0);
    buf = appendVarint(buf, round);
  }
  const timeoutMs = Number(req.timeout_ms ?? 3e4);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 5, durBytes);
  }
  const minAcks = Number(req.min_acks ?? 0) >>> 0;
  if (minAcks > 0)
    buf = appendUint32(buf, 6, minAcks);
  return buf;
}
function encodeMapShardGroupRequest(req) {
  let buf = new Uint8Array(0);
  buf = appendString(buf, 1, ulid());
  buf = appendString(buf, 2, req.group_id ?? "");
  const mapFn = req.map_function;
  if (mapFn && typeof mapFn === "object") {
    buf = appendLengthDelimited(buf, 3, encodeMessage(mapFn));
  }
  const timeoutMs = Number(req.timeout_ms ?? 3e4);
  if (timeoutMs > 0) {
    const durBytes = encodeDurationMs(timeoutMs);
    if (durBytes.length > 0)
      buf = appendLengthDelimited(buf, 4, durBytes);
  }
  const minResponses = Number(req.min_responses ?? 0) >>> 0;
  if (minResponses > 0)
    buf = appendUint32(buf, 5, minResponses);
  return buf;
}
function decodeBroadcastShardGroupResponse(data) {
  return decodeBroadcastLikeResponse(data);
}
function decodeReduceShardGroupResponse(data) {
  const shardResponses = [];
  let result = {};
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 2 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      result = decodeMessagePayload(slice);
    } else if (fn === 3 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      shardResponses.push(decodeShardQueryResponse(slice));
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { result, shard_responses: shardResponses };
}
function decodeAllReduceShardGroupResponse(data) {
  return decodeReduceShardGroupResponse(data);
}
function decodeBarrierShardGroupResponse(data) {
  return decodeBroadcastLikeResponse(data);
}
function decodeMapShardGroupResponse(data) {
  const shardResults = [];
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (fn === 2 && wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      shardResults.push(decodeShardQueryResponse(slice));
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return { shard_results: shardResults };
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

// node_modules/@plexspaces/sdk/dist/wire/registry-proto-wire.js
var enc2 = new TextEncoder();
var dec = new TextDecoder();
function appendStringField(buf, fieldNum, s) {
  if (!s)
    return buf;
  const encoded = enc2.encode(s);
  const bytes = new Uint8Array(encoded.length);
  bytes.set(encoded);
  const tag = fieldNum << 3 | 2;
  let b = appendVarint(buf, tag);
  b = appendVarint(b, bytes.length);
  return concatBytes(b, bytes);
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
      const str = dec.decode(chunk);
      switch (fn_) {
        case 1:
          reg.objectId = str;
          break;
        case 5:
          reg.tenantId = str;
          break;
        case 6:
          reg.namespace = str;
          break;
        case 8:
          reg.grpcAddress = str;
          break;
        case 9:
          reg.objectCategory = str;
          break;
        case 10:
          (reg.capabilities ?? (reg.capabilities = [])).push(str);
          break;
        case 13:
          (reg.labels ?? (reg.labels = [])).push(str);
          break;
        case 18:
          reg.alias = str;
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

// node_modules/@plexspaces/sdk/dist/process_groups.js
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

// node_modules/@plexspaces/sdk/dist/host.js
import { log as hostLog2, nowMs as hostNowMs } from "plexspaces:actor/host-logging@0.1.0";
import { send as hostSend, ask as hostAsk, selfId as hostSelfId, spawn as hostSpawn, stop as hostStop, link as hostLink, unlink as hostUnlink, monitor as hostMonitor, demonitor as hostDemonitor, sendAfter as hostSendAfter, pgJoin as hostPgJoin, pgLeave as hostPgLeave, pgMembers as hostPgMembers, pgBroadcast as hostPgBroadcast } from "plexspaces:actor/host-actor@0.1.0";
import { kvGet as hostKvGet, kvPut as hostKvPut, kvDelete as hostKvDelete, kvList as hostKvList, kvPutWithTtl as hostKvPutWithTtl, kvGetTtl as hostKvGetTtl, kvCas as hostKvCas, kvIncrement as hostKvIncrement, kvMultiGet as hostKvMultiGet, kvMultiPut as hostKvMultiPut, alarmSet as hostAlarmSet, alarmGet as hostAlarmGet, alarmDelete as hostAlarmDelete } from "plexspaces:actor/host-kv@0.1.0";
import { tsWrite as hostTsWrite, tsRead as hostTsRead, tsTake as hostTsTake, tsReadAll as hostTsReadAll } from "plexspaces:actor/host-ts@0.1.0";
import { lockAcquire as hostLockAcquire, lockRelease as hostLockRelease, lockRenew as hostLockRenew } from "plexspaces:actor/host-locks@0.1.0";
import { blobUpload as hostBlobUpload, blobDownload as hostBlobDownload, blobDelete as hostBlobDelete, blobList as hostBlobList } from "plexspaces:actor/host-blob@0.1.0";
import { poolCheckout as hostPoolCheckout, poolCheckin as hostPoolCheckin, poolGetMetrics as hostPoolGetMetrics } from "plexspaces:actor/host-pool@0.1.0";
import { createShardGroup as hostCreateShardGroup, bulkUpdateShardGroup as hostBulkUpdateShardGroup, mapShardGroup as hostMapShardGroup, broadcastShardGroup as hostBroadcastShardGroup, reduceShardGroup as hostReduceShardGroup, allReduceShardGroup as hostAllReduceShardGroup, barrierShardGroup as hostBarrierShardGroup, scatterGather as hostScatterGather, spawnActors as hostSpawnActors, applicationMetricsAdd as hostApplicationMetricsAdd, applicationGetMetrics as hostApplicationGetMetrics, applicationGetStatus as hostApplicationGetStatus } from "plexspaces:actor/host-shard@0.1.0";
import { httpFetch as hostHttpFetch } from "plexspaces:actor/host-http@0.1.0";
import { channelSend as hostChannelSend, channelSendWithOptions as hostChannelSendWithOptions, channelReceive as hostChannelReceive, channelPublish as hostChannelPublish, channelSubscribe as hostChannelSubscribe, channelUnsubscribe as hostChannelUnsubscribe, channelAck as hostChannelAck, channelNack as hostChannelNack, channelCreate as hostChannelCreate, channelDelete as hostChannelDelete, channelDepth as hostChannelDepth } from "plexspaces:actor/channels@0.1.0";
import { register as hostRegistryRegister, unregister as hostRegistryUnregister, lookup as hostRegistryLookup, lookupByAlias as hostRegistryLookupByAlias, discover as hostRegistryDiscover, heartbeat as hostRegistryHeartbeat } from "plexspaces:actor/registry@0.1.0";
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
function bytesToBase64(bytes) {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let result = "";
  const len = bytes.length;
  for (let i = 0; i < len; i += 3) {
    const b0 = bytes[i];
    const b1 = i + 1 < len ? bytes[i + 1] : 0;
    const b2 = i + 2 < len ? bytes[i + 2] : 0;
    result += chars[b0 >> 2] + chars[(b0 & 3) << 4 | b1 >> 4] + (i + 1 < len ? chars[(b1 & 15) << 2 | b2 >> 6] : "=") + (i + 2 < len ? chars[b2 & 63] : "=");
  }
  return result;
}
function base64ToBytes(b64) {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const clean = b64.replace(/=+$/, "");
  const len = clean.length;
  const bytes = new Uint8Array(Math.floor(len * 3 / 4));
  let pos = 0;
  for (let i = 0; i < len; i += 4) {
    const c0 = chars.indexOf(clean[i]);
    const c1 = chars.indexOf(clean[i + 1]);
    const c2 = i + 2 < len ? chars.indexOf(clean[i + 2]) : 0;
    const c3 = i + 3 < len ? chars.indexOf(clean[i + 3]) : 0;
    bytes[pos++] = c0 << 2 | c1 >> 4;
    if (i + 2 < len)
      bytes[pos++] = (c1 & 15) << 4 | c2 >> 2;
    if (i + 3 < len)
      bytes[pos++] = (c2 & 3) << 6 | c3;
  }
  return bytes.subarray(0, pos);
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
    const raw = safeCall(hostPgMembers, group);
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
    const result = safeCall(hostPgBroadcast, group, msgType, payloadBytes);
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
    const result = safeCall(hostRegistryRegister, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /**
   * Unregister an object from the registry.
   */
  unregister(objectId, objectType, tenantId, namespace) {
    const reqBytes = encodeUnregisterRequest(objectId, objectType, tenantId, namespace);
    const result = safeCall(hostRegistryUnregister, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /**
   * Look up an object by ID. Returns null if not found, throws on storage errors.
   */
  lookup(objectId, objectType = 0, tenantId, namespace) {
    const reqBytes = encodeLookupRequest(objectId, objectType, tenantId, namespace);
    const raw = safeCall(hostRegistryLookup, reqBytes);
    if (typeof raw === "string" && raw.startsWith("ERROR:")) {
      throw new Error(raw);
    }
    if (!raw)
      return null;
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0)
      return null;
    return decodeLookupResponse(bytes);
  }
  /**
   * Look up an object by alias (Orleans grain directory pattern).
   * Alias format: "{actor_type}:{name}:{namespace}:{tenant_id}"
   * Returns null if not found, throws on storage errors.
   */
  lookupByAlias(alias) {
    const raw = safeCall(hostRegistryLookupByAlias, alias);
    if (typeof raw === "string" && raw.startsWith("ERROR:")) {
      throw new Error(raw);
    }
    if (!raw)
      return null;
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0)
      return null;
    return decodeLookupResponse(bytes);
  }
  /**
   * Discover objects with optional filtering.
   */
  discover(options = {}) {
    const reqBytes = encodeDiscoverRequest(options);
    const raw = safeCall(hostRegistryDiscover, reqBytes);
    if (!raw)
      return [];
    if (typeof raw === "string" && raw.startsWith("ERROR:")) {
      throw new Error(raw);
    }
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0)
      return [];
    return decodeDiscoverResponse(bytes);
  }
  /**
   * Update the heartbeat for a registered object.
   */
  heartbeat(objectId, objectType = 0, tenantId, namespace) {
    const reqBytes = encodeHeartbeatRequest(objectId, objectType, tenantId, namespace);
    const result = safeCall(hostRegistryHeartbeat, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
};
var KVStore = class {
  get(key) {
    if (typeof hostKvGet !== "function")
      return null;
    try {
      const v = decodeWitPayloadUtf8(hostKvGet(key));
      return v || null;
    } catch {
      return null;
    }
  }
  put(key, value) {
    if (typeof hostKvPut !== "function")
      return;
    try {
      hostKvPut(key, encodeWitPayloadUtf8(value));
    } catch {
    }
  }
  delete(key) {
    if (typeof hostKvDelete !== "function")
      return;
    try {
      hostKvDelete(key);
    } catch {
    }
  }
  list(prefix) {
    if (typeof hostKvList !== "function")
      return [];
    try {
      return hostKvList(prefix);
    } catch {
      return [];
    }
  }
  putWithTtl(key, value, ttlSeconds) {
    if (typeof hostKvPutWithTtl !== "function")
      return;
    hostKvPutWithTtl(key, encodeWitPayloadUtf8(value), BigInt(ttlSeconds));
  }
  getTtl(key) {
    if (typeof hostKvGetTtl !== "function")
      return 0;
    try {
      return Number(hostKvGetTtl(key));
    } catch {
      return 0;
    }
  }
  cas(key, expected, newValue) {
    if (typeof hostKvCas !== "function")
      return false;
    const expectedBytes = expected !== null ? encodeWitPayloadUtf8(expected) : new Uint8Array(0);
    return hostKvCas(key, expectedBytes, encodeWitPayloadUtf8(newValue));
  }
  increment(key, delta) {
    if (typeof hostKvIncrement !== "function")
      return 0;
    try {
      return Number(hostKvIncrement(key, BigInt(delta)));
    } catch {
      return 0;
    }
  }
  multiGet(keys) {
    if (typeof hostKvMultiGet !== "function")
      return keys.map(() => null);
    try {
      const keysJson = encodeWitPayloadUtf8(JSON.stringify(keys));
      const resultBytes = hostKvMultiGet(keysJson);
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
    if (typeof hostKvMultiPut !== "function")
      return;
    const encoded = {};
    for (const [k, v] of Object.entries(entries)) {
      encoded[k] = bytesToBase64(new TextEncoder().encode(v));
    }
    const entriesJson = encodeWitPayloadUtf8(JSON.stringify(encoded));
    hostKvMultiPut(entriesJson);
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
    if (typeof hostAlarmSet !== "function")
      return;
    hostAlarmSet(BigInt(timestampMs));
  }
  setIn(delayMs) {
    if (typeof hostNowMs !== "function")
      return;
    const now = Number(hostNowMs());
    this.set(now + delayMs);
  }
  get() {
    if (typeof hostAlarmGet !== "function")
      return 0;
    try {
      return Number(hostAlarmGet());
    } catch {
      return 0;
    }
  }
  delete() {
    if (typeof hostAlarmDelete !== "function")
      return;
    hostAlarmDelete();
  }
};
var LockClient = class {
  acquire(holderId, lockName, leaseDurationSecs, timeoutMs) {
    const result = hostLockAcquire(holderId, lockName, leaseDurationSecs, timeoutMs);
    if (typeof result === "string")
      throw new Error(result);
    return result;
  }
  release(lockId, holderId, lockVersion) {
    const result = hostLockRelease(lockId, holderId, lockVersion);
    if (typeof result === "string")
      throw new Error(result);
  }
  renew(lockId, holderId, lockVersion, leaseDurationSecs) {
    const result = hostLockRenew(lockId, holderId, lockVersion, leaseDurationSecs);
    if (typeof result === "string")
      throw new Error(result);
    return result;
  }
};
var BlobClient = class {
  upload(name, data, contentType) {
    const result = hostBlobUpload(name, data, contentType);
    if (typeof result !== "string" || result.startsWith("ERROR:"))
      throw new Error(String(result));
    return result;
  }
  download(blobId) {
    const result = hostBlobDownload(blobId);
    if (typeof result === "string")
      throw new Error(result);
    return result;
  }
  delete(blobId) {
    const result = hostBlobDelete(blobId);
    if (typeof result === "string")
      throw new Error(result);
  }
  list(prefix) {
    const result = hostBlobList(prefix);
    if (Array.isArray(result))
      return result;
    return [];
  }
};
var Channel = class {
  /** Send a message to a channel (queue semantics). Returns message ID. */
  send(channelName, msgType, payload) {
    const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "{}");
    const result = safeCall(hostChannelSend, "", channelName, msgType, payloadBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Send with delay, TTL, and custom headers. Returns message ID. */
  sendWithOptions(channelName, msgType, payload, delayMs = 0, ttlMs = 0, headers) {
    const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "{}");
    const headersJson = JSON.stringify(headers ?? {});
    const result = safeCall(hostChannelSendWithOptions, "", channelName, msgType, payloadBytes, BigInt(delayMs), BigInt(ttlMs), headersJson);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Receive one message from a channel. Returns null on timeout/empty. */
  receive(channelName, timeoutMs = 0) {
    const raw = safeCall(hostChannelReceive, "", channelName, BigInt(timeoutMs));
    if (!raw || raw === "" || raw === void 0)
      return null;
    if (typeof raw === "string") {
      if (raw.startsWith("ERROR:"))
        throw new Error(raw);
      try {
        return JSON.parse(raw);
      } catch {
        return null;
      }
    }
    const msg = raw;
    if (!msg || !msg.id)
      return null;
    let decodedPayload;
    try {
      const payloadStr = decodeWitPayloadUtf8(msg.payload);
      decodedPayload = JSON.parse(payloadStr);
    } catch {
      decodedPayload = msg.payload;
    }
    const hdrs = {};
    if (Array.isArray(msg.headers)) {
      for (const [k, v] of msg.headers) {
        hdrs[k] = v;
      }
    }
    return {
      id: msg.id,
      msgType: msg.msgType,
      payload: decodedPayload,
      timestamp: typeof msg.timestamp === "bigint" ? Number(msg.timestamp) : Number(msg.timestamp ?? 0),
      deliveryCount: msg.deliveryCount,
      headers: hdrs
    };
  }
  /** Publish a message to a channel (pub/sub — all subscribers receive). Returns message ID. */
  publish(channelName, msgType, payload) {
    const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "{}");
    const result = safeCall(hostChannelPublish, "", channelName, msgType, payloadBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Subscribe to a channel (pub/sub). Returns subscription ID. */
  subscribe(channelName, filter = "") {
    const result = safeCall(hostChannelSubscribe, "", channelName, filter);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return result;
  }
  /** Cancel a subscription by ID. */
  unsubscribe(subscriptionId) {
    const result = safeCall(hostChannelUnsubscribe, subscriptionId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Acknowledge successful processing (prevents redelivery). */
  ack(channelName, messageId) {
    const result = safeCall(hostChannelAck, "", channelName, messageId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Negative-acknowledge a message. requeue=true retries; false sends to dead-letter. */
  nack(channelName, messageId, requeue = true) {
    const result = safeCall(hostChannelNack, "", channelName, messageId, requeue);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Create a channel if it does not exist. maxSize=0 means unbounded. */
  create(channelName, maxSize = 0, messageTtlMs = 0) {
    const result = safeCall(hostChannelCreate, "", channelName, maxSize, BigInt(messageTtlMs));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Delete a channel and all pending messages. */
  delete(channelName) {
    const result = safeCall(hostChannelDelete, "", channelName);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
  }
  /** Return the number of pending (unacked) messages in a channel. */
  depth(channelName) {
    const result = safeCall(hostChannelDepth, "", channelName);
    if (typeof result === "bigint")
      return Number(result);
    if (typeof result === "number")
      return result;
    if (typeof result === "string") {
      if (result.startsWith("ERROR:"))
        throw new Error(result);
      const n = parseInt(result, 10);
      return isNaN(n) ? 0 : n;
    }
    return 0;
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
    this.channel = new Channel();
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
    const raw = safeCall(hostSend, to, msgType, payloadBytes);
    if (typeof raw !== "string") {
      return "";
    }
    return raw;
  }
  /** Send request and wait for response (request-reply) */
  ask(to, msgType, payload, timeoutMs = 5e3) {
    const payloadBytes = encodeWitPayloadUtf8(payload !== void 0 ? JSON.stringify(payload) : "");
    const raw = safeCall(hostAsk, to, msgType, payloadBytes, BigInt(timeoutMs));
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
    return safeCall(hostSelfId);
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
    const result = safeCall(hostSpawn, moduleRef, actorName, role, argsJson);
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
    const wireReq = {
      group_id: request.groupId,
      actor_type: request.actorType,
      shard_count: request.shardCount,
      partition_strategy: request.partitionStrategy ?? "hash",
      rebalance_policy: request.rebalancePolicy ?? "manual",
      placement: request.placement ? {
        strategy: request.placement.strategy ?? "from_registry",
        node_ids: request.placement.nodeIds ?? [],
        cluster: request.placement.cluster ?? ""
      } : void 0,
      initial_state: request.initialState,
      metadata: request.metadata
    };
    const reqBytes = encodeCreateShardGroupRequest(wireReq);
    const result = safeCall(hostCreateShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { groupId: "", actorType: "", shardActorIds: [], shardCount: 0 };
    const decoded = decodeCreateShardGroupResponse(bytes);
    const group = decoded.group ?? {};
    const config = group.config ?? {};
    return {
      groupId: config.group_id ?? "",
      actorType: group.actor_type ?? "",
      shardActorIds: group.shard_actor_ids ?? [],
      shardCount: config.shard_count ?? 0
    };
  }
  bulkUpdateShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      updates: request.updates,
      consistency_level: request.consistencyLevel === "strong" ? 2 : request.consistencyLevel === "sequential" ? 3 : 1,
      timeout_ms: request.timeoutMs ?? 5e3,
      wait_for_responses: request.waitForResponses ?? false
    };
    const reqBytes = encodeBulkUpdateShardGroupRequest(wireReq);
    const result = safeCall(hostBulkUpdateShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { updates_sent: 0, updates_succeeded: 0, updates_failed: 0, errors: [] };
    return decodeBulkUpdateShardGroupResponse(bytes);
  }
  mapShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      map_function: request.mapFunction,
      timeout_ms: request.timeoutMs ?? 3e4,
      min_responses: request.minResponses ?? 0
    };
    const reqBytes = encodeMapShardGroupRequest(wireReq);
    const result = safeCall(hostMapShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_results: [] };
    return decodeMapShardGroupResponse(bytes);
  }
  scatterGather(request) {
    const wireReq = {
      group_id: request.groupId,
      query: request.query,
      aggregation: request.aggregation ?? "concat",
      timeout_ms: request.timeoutMs ?? 3e4,
      min_responses: request.minResponses ?? 0
    };
    const reqBytes = encodeScatterGatherRequest(wireReq);
    const result = safeCall(hostScatterGather, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shardResponses: [] };
    const decoded = decodeScatterGatherResponse(bytes);
    return { shardResponses: decoded.shard_responses.map((r) => ({
      shardId: r.shard_id ?? 0,
      shardActorId: r.shard_actor_id ?? "",
      payload: r.payload ?? {},
      success: r.success ?? false,
      error: r.error ?? ""
    })) };
  }
  broadcastShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      message: request.message,
      timeout_ms: request.timeoutMs ?? 3e4,
      min_acks: request.minAcks ?? 0
    };
    const reqBytes = encodeBroadcastShardGroupRequest(wireReq);
    const result = safeCall(hostBroadcastShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_responses: [] };
    return decodeBroadcastShardGroupResponse(bytes);
  }
  reduceShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      map_function: request.mapFunction,
      reduction: request.reduction,
      target: request.target,
      timeout_ms: request.timeoutMs ?? 3e4,
      min_responses: request.minResponses ?? 0
    };
    const reqBytes = encodeReduceShardGroupRequest(wireReq);
    const result = safeCall(hostReduceShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_responses: [] };
    return decodeReduceShardGroupResponse(bytes);
  }
  allReduceShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      map_function: request.mapFunction,
      reduction: request.reduction,
      target: request.target,
      timeout_ms: request.timeoutMs ?? 3e4,
      min_responses: request.minResponses ?? 0
    };
    const reqBytes = encodeAllReduceShardGroupRequest(wireReq);
    const result = safeCall(hostAllReduceShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_responses: [] };
    return decodeAllReduceShardGroupResponse(bytes);
  }
  barrierShardGroup(request) {
    const wireReq = {
      group_id: request.groupId,
      barrier_id: request.barrierId,
      round: request.round ?? 0,
      timeout_ms: request.timeoutMs ?? 3e4,
      min_acks: request.minAcks ?? 0
    };
    const reqBytes = encodeBarrierShardGroupRequest(wireReq);
    const result = safeCall(hostBarrierShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_responses: [] };
    return decodeBarrierShardGroupResponse(bytes);
  }
  spawnActors(request) {
    const result = safeCall(hostSpawnActors, JSON.stringify(request));
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
  }
  applicationMetricsAdd(applicationId, metrics) {
    const metricsBytes = encodeApplicationMetrics(metrics);
    const result = safeCall(hostApplicationMetricsAdd, applicationId, metricsBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return {};
    try {
      return JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      return {};
    }
  }
  applicationGetMetrics(applicationId, nodeId) {
    const result = safeCall(hostApplicationGetMetrics, applicationId, nodeId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return {};
    return decodeApplicationMetrics(bytes);
  }
  applicationGetStatus(applicationId, nodeId) {
    const result = safeCall(hostApplicationGetStatus, applicationId, nodeId);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { node_id: nodeId, node_address: "", application: null };
    return decodeGetApplicationStatusResponse(bytes);
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

// node_modules/@plexspaces/sdk/dist/router.js
var ActorRouter = class {
  constructor(routes) {
    this.active = null;
    this.factories = routes;
  }
  /** WIT `init(config: payload) -> result<_, actor-error>` */
  init(configJson) {
    const text = decodeWitPayloadUtf8(configJson);
    const config = text.trim() ? JSON.parse(text) : {};
    const actorType = config.actor_type || "";
    const role = config.role || "";
    let factory = actorType ? this.factories[actorType] : void 0;
    if (!factory && role) {
      factory = this.factories[role];
    }
    if (!factory) {
      throw new Error(`ERROR: no actor registered for actor_type='${actorType}' role='${role}'`);
    }
    this.active = factory();
    this.active.init(text);
  }
  /** WIT `handle(...) -> result<payload, actor-error>` */
  handle(fromActor, msgType, payloadJson) {
    if (!this.active) {
      return encodeWitPayloadUtf8('{"error":"no active actor (init not called)"}');
    }
    return this.active.handle(fromActor, msgType, payloadJson);
  }
  /** WIT `get-state() -> result<payload, actor-error>` */
  getState() {
    if (!this.active) {
      return encodeWitPayloadUtf8("{}");
    }
    return this.active.getState();
  }
  /** WIT `set-state(state: payload) -> result<_, actor-error>` */
  setState(stateJson) {
    if (!this.active) {
      throw new Error("ERROR: no active actor");
    }
    this.active.setState(stateJson);
  }
};

// node_modules/@plexspaces/sdk/dist/wire/ws-frame-wire.js
var textEnc = new TextEncoder();
var textDec = new TextDecoder("utf-8", { fatal: false });

// feature_store_actor.ts
var USER_FEATURES = [
  "user.age",
  "user.income",
  "user.signup_days",
  "user.lifetime_value",
  "user.login_frequency"
];
var PRODUCT_FEATURES = [
  "product.price",
  "product.category_id",
  "product.popularity_score",
  "product.stock_level",
  "product.review_avg"
];
var INTERACTION_FEATURES = [
  "interaction.click_count",
  "interaction.purchase_count",
  "interaction.avg_session_duration",
  "interaction.cart_abandonment_rate",
  "interaction.page_views_7d"
];
var ALL_FEATURES = [...USER_FEATURES, ...PRODUCT_FEATURES, ...INTERACTION_FEATURES];
function generateFeatures(entityCount, seed) {
  const records = [];
  let rng = seed;
  for (let e = 0; e < entityCount; e++) {
    const entityId = `entity-${e}`;
    for (const featureName of ALL_FEATURES) {
      rng = rng * 1103515245 + 12345 & 2147483647;
      let value;
      if (featureName.includes("age")) {
        value = 18 + rng % 62;
      } else if (featureName.includes("income")) {
        value = 2e4 + rng % 18e4;
      } else if (featureName.includes("price")) {
        value = Math.round((1 + rng % 99900) / 100);
      } else if (featureName.includes("category_id")) {
        value = rng % 50;
      } else if (featureName.includes("score") || featureName.includes("rate") || featureName.includes("avg")) {
        value = Math.round(rng % 1e4 / 100);
      } else if (featureName.includes("count") || featureName.includes("views")) {
        value = rng % 5e3;
      } else if (featureName.includes("duration")) {
        value = 10 + rng % 3590;
      } else if (featureName.includes("days")) {
        value = rng % 3650;
      } else if (featureName.includes("level")) {
        value = rng % 1e4;
      } else {
        value = rng % 1e4;
      }
      records.push({
        entity_id: entityId,
        feature_name: featureName,
        value,
        version: 1,
        timestamp: 17e11 + rng % 864e5
      });
    }
  }
  return records;
}
function hashEntityToShard(entityId, shardCount) {
  let h = 0;
  for (let i = 0; i < entityId.length; i++) {
    h = h * 31 + entityId.charCodeAt(i) & 2147483647;
  }
  return h % shardCount;
}
var LeaderActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      application_id: "",
      role: "leader",
      total_compute_ms: 0,
      total_coord_ms: 0
    };
  }
  onInit(config) {
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = "leader";
  }
  onRun(payload) {
    const entityCount = intValue(payload.entity_count, 500);
    const workerCount = intValue(payload.worker_count, 8);
    const lookupCount = intValue(payload.lookup_count, 2e3);
    const cacheSize = intValue(payload.cache_size, 200);
    const coordStart = host.nowMs();
    const groupId = `feature-store-ts-${host.nowMs()}`;
    const group = host.createShardGroup({
      groupId,
      actorType: "feature-worker",
      shardCount: workerCount,
      partitionStrategy: "hash",
      rebalancePolicy: "manual",
      placement: { strategy: "from_registry" },
      initialState: { cache_max_size: cacheSize }
    });
    const shardActorIds = group.shardActorIds;
    if (shardActorIds.length === 0) {
      return { status: "error", error: "failed to create worker shard group" };
    }
    const coordCreate = host.nowMs() - coordStart;
    const features = generateFeatures(entityCount, host.nowMs() % 1e5);
    const batchSize = 500;
    let totalComputeMs = 0;
    let totalCoordMs = coordCreate;
    let totalIngested = 0;
    let errorCount = 0;
    const shardBatches = Array.from({ length: workerCount }, () => []);
    for (const f of features) {
      const shard = hashEntityToShard(f.entity_id, workerCount);
      shardBatches[shard].push(f);
    }
    for (let s = 0; s < workerCount; s++) {
      const batch = shardBatches[s];
      for (let i = 0; i < batch.length; i += batchSize) {
        const chunk = batch.slice(i, i + batchSize);
        const sgStart = host.nowMs();
        const sgResult = host.scatterGather({
          groupId,
          query: {
            op: "ingest_batch",
            features: chunk
          },
          timeoutMs: 3e4
        });
        const sgElapsed = host.nowMs() - sgStart;
        totalCoordMs += sgElapsed;
        for (const resp of sgResult.shardResponses) {
          const result = recordValue(resp.payload ?? resp);
          if (result.error) {
            errorCount++;
            continue;
          }
          totalIngested += intValue(result.ingested_count, 0);
          totalComputeMs += intValue(result.compute_ms, 0);
        }
      }
    }
    let totalLookupMs = 0;
    let cacheHits = 0;
    let cacheMisses = 0;
    let lookupsServed = 0;
    const lookupLatencies = [];
    const pointLookupCount = Math.floor(lookupCount * 0.5);
    for (let i = 0; i < pointLookupCount; i += batchSize) {
      const batchEnd = Math.min(i + batchSize, pointLookupCount);
      const lookupEntities = [];
      for (let j = i; j < batchEnd; j++) {
        lookupEntities.push(`entity-${j % entityCount}`);
      }
      const sgStart = host.nowMs();
      const sgResult = host.scatterGather({
        groupId,
        query: {
          op: "batch_lookup",
          entity_ids: lookupEntities,
          feature_names: ALL_FEATURES.slice(0, 5)
        },
        timeoutMs: 3e4
      });
      const sgElapsed = host.nowMs() - sgStart;
      totalCoordMs += sgElapsed;
      for (const resp of sgResult.shardResponses) {
        const result = recordValue(resp.payload ?? resp);
        cacheHits += intValue(result.cache_hits, 0);
        cacheMisses += intValue(result.cache_misses, 0);
        lookupsServed += intValue(result.lookups_served, 0);
        totalComputeMs += intValue(result.compute_ms, 0);
        const latency = intValue(result.compute_ms, 0);
        if (latency > 0) lookupLatencies.push(latency);
      }
    }
    const vectorLookupCount = lookupCount - pointLookupCount;
    for (let i = 0; i < vectorLookupCount; i += batchSize) {
      const batchEnd = Math.min(i + batchSize, vectorLookupCount);
      const lookupEntities = [];
      for (let j = i; j < batchEnd; j++) {
        lookupEntities.push(`entity-${j % entityCount}`);
      }
      const sgStart = host.nowMs();
      const sgResult = host.scatterGather({
        groupId,
        query: {
          op: "get_feature_vectors",
          entity_ids: lookupEntities
        },
        timeoutMs: 3e4
      });
      const sgElapsed = host.nowMs() - sgStart;
      totalCoordMs += sgElapsed;
      for (const resp of sgResult.shardResponses) {
        const result = recordValue(resp.payload ?? resp);
        cacheHits += intValue(result.cache_hits, 0);
        cacheMisses += intValue(result.cache_misses, 0);
        lookupsServed += intValue(result.lookups_served, 0);
        totalComputeMs += intValue(result.compute_ms, 0);
        const latency = intValue(result.compute_ms, 0);
        if (latency > 0) lookupLatencies.push(latency);
      }
    }
    lookupLatencies.sort((a, b) => a - b);
    const p50 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.5)] : 0;
    const p95 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.95)] : 0;
    const p99 = lookupLatencies.length > 0 ? lookupLatencies[Math.floor(lookupLatencies.length * 0.99)] : 0;
    const wallTime = totalComputeMs + totalCoordMs;
    const total = totalComputeMs + totalCoordMs || 1;
    const granularity = totalCoordMs > 0 ? Math.round(totalComputeMs / totalCoordMs * 10) / 10 : 0;
    const lookupsPerSec = wallTime > 0 ? Math.round(lookupsServed / wallTime * 1e3) : 0;
    const cacheHitRate = cacheHits + cacheMisses > 0 ? Math.round(cacheHits / (cacheHits + cacheMisses) * 1e3) / 10 : 0;
    const leaderNodeId = actorNodeId(this.state.actor_id);
    const nodeActorCounts = computeActorCounts(leaderNodeId, shardActorIds);
    try {
      host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "leader.compute": totalComputeMs, "leader.coordination": totalCoordMs } });
    } catch (_e) {
    }
    return {
      status: "ok",
      entity_count: entityCount,
      feature_count: features.length,
      worker_count: workerCount,
      lookup_count: lookupCount,
      cache_size: cacheSize,
      wall_time_ms: wallTime,
      compute_time_ms: totalComputeMs,
      coordination_time_ms: totalCoordMs,
      granularity_ratio: granularity,
      total_ingested: totalIngested,
      lookups_served: lookupsServed,
      lookups_per_sec: lookupsPerSec,
      cache_hits: cacheHits,
      cache_misses: cacheMisses,
      cache_hit_rate_pct: cacheHitRate,
      latency_p50_ms: p50,
      latency_p95_ms: p95,
      latency_p99_ms: p99,
      node_count: Object.keys(nodeActorCounts).length,
      actor_count: shardActorIds.length + 1,
      leader_node_id: leaderNodeId,
      nodes: nodeActorCounts,
      error_count: errorCount
    };
  }
  onRun_scaling_benchmark(payload) {
    const entityCount = intValue(payload.entity_count, 500);
    const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
    const lookupCount = intValue(payload.lookup_count, 2e3);
    const cacheSize = intValue(payload.cache_size, 200);
    const warmupRounds = intValue(payload.warmup_rounds, 1);
    const benchmarkRounds = intValue(payload.benchmark_rounds, 2);
    const results = [];
    let baselineWall = 0;
    for (const shardCount of shardCounts) {
      for (let w = 0; w < warmupRounds; w++) {
        this.onRun({
          entity_count: Math.min(entityCount, 100),
          worker_count: shardCount,
          lookup_count: Math.min(lookupCount, 200),
          cache_size: cacheSize
        });
      }
      let totalWall = 0;
      let totalCompute = 0;
      let totalCoord = 0;
      let totalLookupsPerSec = 0;
      let totalCacheHitRate = 0;
      let nodeCount = 0;
      let errorCount = 0;
      const runEntityCount = Math.min(entityCount, 100);
      const runLookupCount = Math.min(lookupCount, 200);
      for (let r = 0; r < benchmarkRounds; r++) {
        const result = this.onRun({
          entity_count: runEntityCount,
          worker_count: shardCount,
          lookup_count: runLookupCount,
          cache_size: cacheSize
        });
        totalWall += intValue(result.wall_time_ms, 0);
        totalCompute += intValue(result.compute_time_ms, 0);
        totalCoord += intValue(result.coordination_time_ms, 0);
        totalLookupsPerSec += intValue(result.lookups_per_sec, 0);
        totalCacheHitRate += floatValue(result.cache_hit_rate_pct, 0);
        nodeCount = intValue(result.node_count, 0);
        errorCount += intValue(result.error_count, 0);
      }
      const avgWall = Math.round(totalWall / benchmarkRounds);
      const avgCompute = Math.round(totalCompute / benchmarkRounds);
      const avgCoord = Math.round(totalCoord / benchmarkRounds);
      const avgLookupsPerSec = Math.round(totalLookupsPerSec / benchmarkRounds);
      const avgCacheHitRate = Math.round(totalCacheHitRate / benchmarkRounds * 10) / 10;
      const total = avgCompute + avgCoord || 1;
      if (baselineWall === 0) baselineWall = avgWall;
      const speedup = baselineWall > 0 && avgWall > 0 ? baselineWall / avgWall : 1;
      const efficiency = speedup / (shardCount / shardCounts[0]) * 100;
      results.push({
        shards: shardCount,
        lookups_per_sec: avgLookupsPerSec,
        wall_time_ms: avgWall,
        compute_time_ms: avgCompute,
        coordination_time_ms: avgCoord,
        compute_pct: avgCompute * 100 / total,
        granularity_ratio: avgCoord > 0 ? Math.round(avgCompute / avgCoord * 10) / 10 : 0,
        speedup: Math.round(speedup * 100) / 100,
        efficiency_pct: Math.round(efficiency * 10) / 10,
        cache_hit_rate_pct: avgCacheHitRate,
        node_count: nodeCount,
        error_count: errorCount
      });
    }
    return {
      status: "ok",
      entity_count: entityCount,
      lookup_count: lookupCount,
      results
    };
  }
  onRun_weak_scaling_benchmark(payload) {
    const entitiesPerShard = intValue(payload.entities_per_shard, 200);
    const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
    const numPasses = intValue(payload.num_passes, 4);
    const benchmarkRounds = intValue(payload.benchmark_rounds, 1);
    const results = [];
    let baselineThroughput = 0;
    for (const shardCount of shardCounts) {
      const groupId = `feature-bench-ts-${host.nowMs()}`;
      host.createShardGroup({
        groupId,
        actorType: "feature-worker",
        shardCount,
        partitionStrategy: "hash",
        rebalancePolicy: "manual",
        placement: { strategy: "from_registry" },
        initialState: { cache_max_size: entitiesPerShard }
      });
      let totalComputeMs = 0;
      let totalCoordMs = 0;
      let totalLookups = 0;
      let errorCount = 0;
      for (let r = 0; r < benchmarkRounds; r++) {
        const wallStart = host.nowMs();
        const sgResult = host.scatterGather({
          groupId,
          query: {
            op: "benchmark_lookups",
            entities_per_shard: entitiesPerShard,
            num_passes: numPasses,
            seed: r + shardCount * 100
          },
          timeoutMs: 6e4
        });
        const wallMs = host.nowMs() - wallStart;
        let roundCompute = 0;
        let roundLookups = 0;
        for (const resp of sgResult.shardResponses) {
          const result = recordValue(resp.payload ?? resp);
          if (result.error) {
            errorCount++;
            continue;
          }
          roundCompute += intValue(result.compute_ms, 0);
          roundLookups += intValue(result.lookups_served, 0);
        }
        totalCoordMs += wallMs;
        totalComputeMs += roundCompute;
        totalLookups += roundLookups;
      }
      const avgWall = Math.round(totalCoordMs / benchmarkRounds);
      const avgCompute = Math.round(totalComputeMs / benchmarkRounds);
      const avgCoord = Math.max(avgWall - Math.round(avgCompute / shardCount), 1);
      const avgLookups = Math.round(totalLookups / benchmarkRounds);
      const lookupsPerSec = avgWall > 0 ? Math.round(avgLookups * 1e3 / avgWall) : 0;
      const granularity = avgCoord > 0 ? Math.round(Math.round(avgCompute / shardCount) / avgCoord * 10) / 10 : 0;
      if (baselineThroughput === 0) baselineThroughput = lookupsPerSec;
      const efficiency = baselineThroughput > 0 ? lookupsPerSec / baselineThroughput * 100 : 100;
      results.push({
        shards: shardCount,
        total_lookups: avgLookups,
        lookups_per_sec: lookupsPerSec,
        wall_time_ms: avgWall,
        compute_time_ms: avgCompute,
        coordination_time_ms: avgCoord,
        granularity_ratio: granularity,
        efficiency_pct: Math.round(efficiency * 10) / 10,
        error_count: errorCount
      });
    }
    return {
      status: "ok",
      entities_per_shard: entitiesPerShard,
      num_passes: numPasses,
      results
    };
  }
};
var WorkerActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      application_id: "",
      role: "worker",
      features: {},
      cache: {},
      cache_order: [],
      cache_max_size: 200,
      cache_hits: 0,
      cache_misses: 0,
      lookups_served: 0
    };
  }
  onInit(config) {
    const args = recordValue(config.args);
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = "worker";
    this.state.cache_max_size = intValue(args.cache_max_size, 200);
  }
  // Worker-local benchmark: generates entities locally, ingests into state, then does
  // num_passes of full-vector lookups. Leader sends one broadcast SG instead of N
  // sequential per-shard calls — enables true weak scaling.
  onBenchmark_lookups(payload) {
    const entitiesPerShard = intValue(payload.entities_per_shard, 200);
    const numPasses = intValue(payload.num_passes, 4);
    const seed = intValue(payload.seed, 42);
    const compStart = host.nowMs();
    const features = generateFeatures(entitiesPerShard, seed);
    for (const f of features) {
      const key = `${f.entity_id}:${f.feature_name}`;
      const existing = this.state.features[key] ?? { versions: [] };
      existing.versions.push({ value: f.value, version: 1, timestamp: compStart });
      if (existing.versions.length > 3) existing.versions.shift();
      this.state.features[key] = existing;
    }
    let totalLookups = 0;
    let cacheHits = 0;
    let cacheMisses = 0;
    const entityIds = Array.from({ length: entitiesPerShard }, (_, i) => `entity-${i}`);
    for (let p = 0; p < numPasses; p++) {
      if (p % 2 === 0) {
        this.state.cache = {};
        this.state.cache_order = [];
      }
      for (const entityId of entityIds) {
        const cached = this.state.cache[entityId];
        if (cached && Object.keys(cached.features).length >= ALL_FEATURES.length) {
          cacheHits++;
          this.state.cache_hits++;
        } else {
          cacheMisses++;
          this.state.cache_misses++;
          const vector = {};
          for (const fname of ALL_FEATURES) {
            const key = `${entityId}:${fname}`;
            const fv = this.state.features[key];
            if (fv && fv.versions.length > 0) {
              vector[fname] = fv.versions[fv.versions.length - 1].value;
            }
          }
          this.addToCache(entityId, vector);
        }
        totalLookups++;
      }
    }
    const computeMs = host.nowMs() - compStart;
    try {
      host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.bench_compute": computeMs } });
    } catch (_e) {
    }
    return {
      lookups_served: totalLookups,
      entities_per_shard: entitiesPerShard,
      passes: numPasses,
      cache_hits: cacheHits,
      cache_misses: cacheMisses,
      compute_ms: computeMs
    };
  }
  onIngest_batch(payload) {
    const compStart = host.nowMs();
    const rawFeatures = anyArray(payload.features);
    let ingested = 0;
    for (const raw of rawFeatures) {
      const f = recordValue(raw);
      const entityId = stringValue(f.entity_id);
      const featureName = stringValue(f.feature_name);
      const value = floatValue(f.value, 0);
      const version = intValue(f.version, 1);
      const timestamp = intValue(f.timestamp, 0);
      if (!entityId || !featureName) continue;
      const key = `${entityId}:${featureName}`;
      if (!this.state.features[key]) {
        this.state.features[key] = { versions: [] };
      }
      const fv = this.state.features[key];
      fv.versions.push({ value, version, timestamp });
      if (fv.versions.length > 3) {
        fv.versions = fv.versions.slice(-3);
      }
      if (this.state.cache[entityId]) {
        delete this.state.cache[entityId];
        this.state.cache_order = this.state.cache_order.filter((id) => id !== entityId);
      }
      ingested++;
    }
    const computeMs = host.nowMs() - compStart;
    try {
      host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.ingest_compute": computeMs } });
    } catch (_e) {
    }
    return {
      ingested_count: ingested,
      compute_ms: computeMs
    };
  }
  onBatch_lookup(payload) {
    const compStart = host.nowMs();
    const entityIds = stringArray(anyArray(payload.entity_ids));
    const featureNames = stringArray(anyArray(payload.feature_names));
    let hits = 0;
    let misses = 0;
    const results = {};
    for (const entityId of entityIds) {
      const cached = this.state.cache[entityId];
      if (cached) {
        hits++;
        this.state.cache_hits++;
        const entityResult2 = {};
        for (const fname of featureNames) {
          if (cached.features[fname] !== void 0) {
            entityResult2[fname] = cached.features[fname];
          }
        }
        results[entityId] = entityResult2;
        this.state.cache_order = this.state.cache_order.filter((id) => id !== entityId);
        this.state.cache_order.push(entityId);
        continue;
      }
      misses++;
      this.state.cache_misses++;
      const entityResult = {};
      for (const fname of featureNames) {
        const key = `${entityId}:${fname}`;
        const fv = this.state.features[key];
        if (fv && fv.versions.length > 0) {
          entityResult[fname] = fv.versions[fv.versions.length - 1].value;
        }
      }
      results[entityId] = entityResult;
      this.addToCache(entityId, entityResult);
      this.state.lookups_served++;
    }
    const computeMs = host.nowMs() - compStart;
    try {
      host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.lookup_compute": computeMs } });
    } catch (_e) {
    }
    return {
      lookups_served: entityIds.length,
      cache_hits: hits,
      cache_misses: misses,
      compute_ms: computeMs,
      result_count: Object.keys(results).length
    };
  }
  onGet_feature_vectors(payload) {
    const compStart = host.nowMs();
    const entityIds = stringArray(anyArray(payload.entity_ids));
    let hits = 0;
    let misses = 0;
    const vectors = {};
    for (const entityId of entityIds) {
      const cached = this.state.cache[entityId];
      if (cached && Object.keys(cached.features).length >= ALL_FEATURES.length) {
        hits++;
        this.state.cache_hits++;
        vectors[entityId] = { ...cached.features };
        this.state.cache_order = this.state.cache_order.filter((id) => id !== entityId);
        this.state.cache_order.push(entityId);
        continue;
      }
      misses++;
      this.state.cache_misses++;
      const vector = {};
      for (const fname of ALL_FEATURES) {
        const key = `${entityId}:${fname}`;
        const fv = this.state.features[key];
        if (fv && fv.versions.length > 0) {
          vector[fname] = fv.versions[fv.versions.length - 1].value;
        }
      }
      vectors[entityId] = vector;
      this.addToCache(entityId, vector);
      this.state.lookups_served++;
    }
    const computeMs = host.nowMs() - compStart;
    try {
      host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.vector_compute": computeMs } });
    } catch (_e) {
    }
    return {
      lookups_served: entityIds.length,
      cache_hits: hits,
      cache_misses: misses,
      compute_ms: computeMs,
      vector_count: Object.keys(vectors).length
    };
  }
  addToCache(entityId, features) {
    this.state.cache[entityId] = {
      entity_id: entityId,
      features,
      cached_at: host.nowMs()
    };
    this.state.cache_order.push(entityId);
    while (this.state.cache_order.length > this.state.cache_max_size) {
      const evicted = this.state.cache_order.shift();
      if (evicted) delete this.state.cache[evicted];
    }
  }
};
function actorApplicationId(actorId) {
  const doubleSlash = actorId.indexOf("//");
  if (doubleSlash < 0) return "";
  const rest = actorId.slice(doubleSlash + 2);
  const colonColon = rest.indexOf("::");
  if (colonColon < 0) return "";
  const afterColon = rest.slice(colonColon + 2);
  const atSign = afterColon.indexOf("@");
  return atSign >= 0 ? afterColon.slice(0, atSign) : afterColon;
}
function actorNodeId(actorId) {
  const atSign = actorId.lastIndexOf("@");
  return atSign >= 0 ? actorId.slice(atSign + 1) : "";
}
function intValue(value, fallback) {
  if (typeof value === "number" && Number.isFinite(value)) return Math.trunc(value);
  if (typeof value === "string") {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}
function floatValue(value, fallback) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}
function intArrayValue(value, fallback) {
  if (!Array.isArray(value)) return fallback;
  return value.map((v) => intValue(v, 0)).filter((v) => v > 0);
}
function stringValue(value) {
  return typeof value === "string" ? value : "";
}
function recordValue(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value : {};
}
function anyArray(value) {
  return Array.isArray(value) ? value : [];
}
function stringArray(value) {
  return value.map((item) => String(item)).filter((item) => item.length > 0);
}
function computeActorCounts(leaderNodeId, shardActorIds) {
  const nodes = {
    [leaderNodeId]: { actors: 1, leader_actors: 1, worker_actors: 0 }
  };
  for (const actorId of shardActorIds) {
    const nodeId = actorNodeId(actorId);
    if (!nodes[nodeId]) {
      nodes[nodeId] = { actors: 0, leader_actors: 0, worker_actors: 0 };
    }
    nodes[nodeId].actors += 1;
    nodes[nodeId].worker_actors += 1;
  }
  return nodes;
}
var router = new ActorRouter({
  leader: () => new LeaderActor(),
  worker: () => new WorkerActor(),
  "feature-leader": () => new LeaderActor(),
  "feature-worker": () => new WorkerActor()
});
var actor2 = {
  init: (configJson) => router.init(configJson),
  handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson) => router.setState(stateJson)
};
export {
  actor2 as actor
};
