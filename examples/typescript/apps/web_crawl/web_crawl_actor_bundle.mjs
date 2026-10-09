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

// src/index.ts
var FETCHER_POOL = "fetcher_pool";
var CRAWL_WORKERS_GROUP = "crawl_workers";
var ANALYZER_GROUP = "analyzer_shards";
var CHECKOUT_TIMEOUT_MS = 5e3;
function appIdFromActorId(actorId) {
  if (actorId.includes("//") && actorId.includes("::")) {
    const suffix = actorId.split("//", 2)[1];
    const qualified = suffix.split("@", 1)[0];
    const parts = qualified.split("::", 2);
    if (parts.length === 2) return parts[1];
  }
  return "";
}
function urlHash(url) {
  let h = 0;
  for (let i = 0; i < url.length; i++) {
    h = Math.imul(h, 31) + url.charCodeAt(i);
    h |= 0;
  }
  return Math.abs(h);
}
function simulateLinks(url) {
  const base = url.replace(/\/+$/, "");
  const h = urlHash(url);
  const sections = [
    "about",
    "docs",
    "api",
    "blog",
    "pricing",
    "features",
    "integrations",
    "changelog",
    "security",
    "status",
    "community",
    "enterprise",
    "solutions",
    "resources"
  ];
  const paths = ["overview", "quickstart", "reference", "guide", "examples", "faq", "support", "contact"];
  const links = [];
  for (let i = 0; i < 8; i++) links.push(`${base}/${sections[(h + i) % sections.length]}`);
  for (let i = 0; i < 4; i++) {
    const sec = sections[(h + i * 3) % sections.length];
    const pth = paths[(h + i * 7) % paths.length];
    links.push(`${base}/${sec}/${pth}`);
  }
  return links;
}
function simulateWordCounts(url) {
  const h = urlHash(url);
  const vocab = [
    "distributed",
    "actor",
    "system",
    "runtime",
    "protocol",
    "message",
    "async",
    "concurrent",
    "parallel",
    "scale",
    "fault",
    "tolerant",
    "cluster",
    "node",
    "network",
    "latency",
    "throughput",
    "pipeline",
    "stream",
    "queue",
    "worker",
    "scheduler",
    "executor",
    "dispatch",
    "route",
    "wasm",
    "sandbox",
    "module",
    "instance",
    "memory",
    "tenant",
    "namespace",
    "isolation",
    "security",
    "auth",
    "deploy",
    "version",
    "rollback",
    "canary",
    "health",
    "metric",
    "trace",
    "span",
    "log",
    "monitor",
    "pool",
    "checkout",
    "checkin",
    "timeout",
    "retry",
    "tuplespace",
    "tuple",
    "pattern",
    "match",
    "read",
    "shard",
    "partition",
    "replicate",
    "consensus",
    "leader",
    "broadcast",
    "scatter",
    "gather",
    "reduce",
    "aggregate",
    "workflow",
    "state",
    "checkpoint",
    "journal",
    "replay"
  ];
  const counts = {};
  for (const seg of url.split("/")) {
    if (!seg || seg === "https:" || seg === "http:") continue;
    for (const word of seg.split(/[^a-zA-Z0-9]/)) {
      if (word.length > 2) {
        const w = word.toLowerCase();
        counts[w] = (counts[w] ?? 0) + 8 + h % 5;
      }
    }
  }
  for (let i = 0; i < 25; i++) {
    const word = vocab[(h + i * 17) % vocab.length];
    const rank = i + 1;
    const count = Math.floor(50 / rank) + 1 + (h + i) % 3;
    counts[word] = (counts[word] ?? 0) + count;
  }
  return counts;
}
var PageFetcher = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", role: "fetcher", pool_slot: 0, fetch_count: 0, last_url: "", worker_joined: false };
  }
  onInit(config) {
    const args = config.args ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "fetcher");
    this.state.pool_slot = Number(args.pool_slot ?? 0);
    try {
      host.processGroups.join(CRAWL_WORKERS_GROUP);
      this.state.worker_joined = true;
    } catch {
    }
  }
  onFetch(payload) {
    if (!this.state.worker_joined) {
      try {
        host.processGroups.join(CRAWL_WORKERS_GROUP);
        this.state.worker_joined = true;
      } catch {
      }
    }
    const url = String(payload.url ?? "");
    if (!url) return { error: "missing url" };
    const links = simulateLinks(url);
    const word_counts = simulateWordCounts(url);
    this.state.fetch_count += 1;
    this.state.last_url = url;
    return { status: "ok", url, links, word_counts };
  }
  // Handles ScatterGather batch: each shard fetches its own slice by pool_slot
  onFetch_batch(payload) {
    if (!this.state.worker_joined) {
      try {
        host.processGroups.join(CRAWL_WORKERS_GROUP);
        this.state.worker_joined = true;
      } catch {
      }
    }
    const urlsRaw = payload.urls ?? [];
    const shardCount = Number(payload.shard_count ?? 1);
    const shardIndex = Number(payload.shard_index ?? this.state.pool_slot);
    let totalWords = 0;
    let pagesFetched = 0;
    for (let i = shardIndex; i < urlsRaw.length; i += shardCount) {
      const url = urlsRaw[i];
      if (!url) continue;
      const wc = simulateWordCounts(url);
      for (const c of Object.values(wc)) totalWords += c;
      this.state.fetch_count += 1;
      this.state.last_url = url;
      pagesFetched += 1;
    }
    return {
      status: "ok",
      fetch_count: this.state.fetch_count,
      pages_fetched: pagesFetched,
      total_words: totalWords,
      shard_index: shardIndex,
      shard_count: shardCount
    };
  }
  // Worker-local benchmark: generates pages locally and runs num_passes of crawl simulation.
  // Leader sends one broadcast SG — enables true weak/strong scaling with one round trip.
  onBenchmark_crawl(payload) {
    const pagesPerWorker = Number(payload.pages_per_worker ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const seed = Number(payload.seed ?? 42);
    const compStart = host.nowMs();
    const domains = ["example.com", "docs.example.com", "api.example.com", "blog.example.com"];
    const sections = ["about", "docs", "api", "blog", "pricing", "features", "integrations", "changelog"];
    const subpaths = ["overview", "quickstart", "reference", "guide", "examples", "faq", "support", "contact"];
    const urls = [];
    let h = seed;
    for (let i = 0; i < pagesPerWorker; i++) {
      h = h * 1103515245 + 12345 & 2147483647;
      const d = domains[h % domains.length];
      const s = sections[(h >> 8) % sections.length];
      const p = subpaths[(h >> 16) % subpaths.length];
      urls.push(`https://${d}/${s}/${p}/${i}`);
    }
    let totalWords = 0;
    let totalLinks = 0;
    for (let pass = 0; pass < numPasses; pass++) {
      for (const url of urls) {
        const wc = simulateWordCounts(url);
        for (const c of Object.values(wc)) totalWords += c;
        const links = simulateLinks(url);
        totalLinks += links.length;
      }
    }
    const computeMs = host.nowMs() - compStart;
    return {
      pages_crawled: pagesPerWorker,
      total_words: totalWords,
      total_links: totalLinks,
      passes: numPasses,
      compute_ms: computeMs
    };
  }
  onStatus_request() {
    return {
      fetch_count: this.state.fetch_count,
      last_url: this.state.last_url,
      idle: true
    };
  }
  onStatus() {
    return { ...this.state };
  }
};
var LinkAnalyzer = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", role: "analyzer", index: {}, urls_analyzed: 0, analyzer_joined: false };
  }
  onInit(config) {
    const args = config.args ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.role = String(args.role ?? "analyzer");
    this.state.index = {};
  }
  onAnalyze(payload) {
    if (!this.state.analyzer_joined) {
      try {
        host.processGroups.join(ANALYZER_GROUP);
        this.state.analyzer_joined = true;
      } catch {
      }
    }
    const results = payload.results ?? [];
    for (const result of results) {
      const wc = result.word_counts;
      if (wc) {
        for (const [word, count] of Object.entries(wc)) {
          this.state.index[word] = (this.state.index[word] ?? 0) + Number(count);
        }
      }
      this.state.urls_analyzed += 1;
    }
    return { status: "ok", urls_analyzed: this.state.urls_analyzed };
  }
  onTop_words(payload) {
    const n = Number(payload.n ?? 10);
    const sorted = Object.entries(this.state.index).sort((a, b) => b[1] - a[1]).slice(0, n);
    return { top_words: sorted };
  }
  onStatus() {
    return { ...this.state };
  }
};
var WebCrawlOrchestrator = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      application_id: "",
      role: "orchestrator",
      pages_crawled: 0,
      total_links: 0,
      top_words: [],
      pool_metrics: {},
      worker_stats: []
    };
  }
  onInit(config) {
    const args = config.args ?? {};
    const actorId = String(config.actor_id ?? "");
    this.state.actor_id = actorId;
    this.state.application_id = appIdFromActorId(actorId);
    this.state.role = String(args.role ?? "orchestrator");
    this.state.pages_crawled = 0;
    this.state.total_links = 0;
    this.state.top_words = [];
  }
  onCrawl(payload) {
    const seedUrls = payload.seed_urls ?? ["https://example.com"];
    const maxPages = Number(payload.max_pages ?? 20);
    const maxDepth = Number(payload.max_depth ?? 2);
    const appId = this.state.application_id;
    const frontier = [];
    const visited = /* @__PURE__ */ new Set();
    for (const url of seedUrls) {
      host.ts.write(["url_queue", url, "pending", "0"]);
      visited.add(url);
      frontier.push({ url, depth: 0 });
    }
    const allResults = [];
    let pagesCrawled = 0;
    let coordTimeMs = 0;
    let fetchTimeMs = 0;
    const t0Crawl = host.nowMs();
    while (frontier.length > 0 && pagesCrawled < maxPages) {
      const task = frontier.shift();
      const { url, depth } = task;
      if (depth > maxDepth) continue;
      let result;
      let handle = null;
      const tCoord = host.nowMs();
      try {
        handle = host.poolCheckout(FETCHER_POOL, CHECKOUT_TIMEOUT_MS);
      } catch {
        handle = null;
      }
      coordTimeMs += host.nowMs() - tCoord;
      const actorId = handle ? String(handle.actor_id ?? "") : "";
      const checkoutId = handle ? String(handle.checkout_id ?? "") : "";
      const tFetch = host.nowMs();
      try {
        if (actorId) {
          result = host.ask(actorId, "fetch", { url, depth }, 1e4);
        } else {
          result = {
            status: "ok",
            url,
            links: simulateLinks(url),
            word_counts: simulateWordCounts(url)
          };
        }
      } catch {
        result = {
          status: "ok",
          url,
          links: simulateLinks(url),
          word_counts: simulateWordCounts(url)
        };
      } finally {
        if (handle && actorId && checkoutId) {
          const tCheckin = host.nowMs();
          try {
            host.poolCheckin(FETCHER_POOL, actorId, checkoutId, true);
          } catch {
          }
          coordTimeMs += host.nowMs() - tCheckin;
        }
      }
      fetchTimeMs += host.nowMs() - tFetch;
      const tCoord2 = host.nowMs();
      const links = result.links ?? [];
      for (const link of links) {
        if (depth + 1 <= maxDepth && !visited.has(link)) {
          visited.add(link);
          host.ts.write(["url_queue", link, "pending", String(depth + 1)]);
          frontier.push({ url: link, depth: depth + 1 });
          this.state.total_links += 1;
        }
      }
      coordTimeMs += host.nowMs() - tCoord2;
      allResults.push(result);
      pagesCrawled += 1;
    }
    this.state.pages_crawled = pagesCrawled;
    const elapsedMs = host.nowMs() - t0Crawl;
    const pagesPerSec = elapsedMs > 0 ? pagesCrawled * 1e3 / elapsedMs : 0;
    const parallelFraction = elapsedMs > 0 ? 1 - coordTimeMs / Math.max(elapsedMs, 1) : 1;
    try {
      const metrics = host.poolGetMetrics(FETCHER_POOL);
      this.state.pool_metrics = metrics ?? { total_checkouts: pagesCrawled, pool_size: 4 };
    } catch {
      this.state.pool_metrics = { total_checkouts: pagesCrawled, pool_size: 4 };
    }
    const numShards = 2;
    const globalCounts = {};
    for (let shardIdx = 0; shardIdx < numShards; shardIdx++) {
      const chunk = allResults.filter((_, i) => i % numShards === shardIdx);
      if (chunk.length === 0) continue;
      const analyzerId = `${appId}/analyzer-${shardIdx}@`;
      try {
        host.ask(analyzerId, "analyze", { results: chunk }, 1e4);
        const top = host.ask(analyzerId, "top_words", { n: 20 }, 1e4);
        for (const [word, count] of top.top_words ?? []) {
          globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
        }
      } catch {
        for (const res of chunk) {
          const wc = res.word_counts;
          if (wc) {
            for (const [word, count] of Object.entries(wc)) {
              globalCounts[word] = (globalCounts[word] ?? 0) + Number(count);
            }
          }
        }
      }
    }
    this.state.top_words = Object.entries(globalCounts).sort((a, b) => b[1] - a[1]).slice(0, 10);
    const workerStats = [];
    try {
      let members = host.processGroups.members(CRAWL_WORKERS_GROUP);
      if (!members || members.length === 0) {
        members = Array.from({ length: 4 }, (_, i) => `${appId}/fetcher-${i}@`);
      }
      for (const memberId of members) {
        try {
          const stats = host.ask(memberId, "status_request", {}, 5e3);
          const parts = memberId.split("/");
          const shortId = (parts[parts.length - 1] ?? memberId).replace(/@$/, "");
          workerStats.push({ worker_id: shortId, ...stats });
        } catch {
        }
      }
    } catch {
    }
    this.state.worker_stats = workerStats;
    return {
      status: "ok",
      pages_crawled: this.state.pages_crawled,
      total_links: this.state.total_links,
      top_words: this.state.top_words,
      pool_metrics: this.state.pool_metrics,
      worker_stats: this.state.worker_stats,
      elapsed_ms: elapsedMs,
      coord_time_ms: coordTimeMs,
      fetch_time_ms: fetchTimeMs,
      pages_per_sec: pagesPerSec,
      parallel_fraction: parallelFraction
    };
  }
  runBenchmarkSg(workerCounts, getPagesPerWorker, numPasses, seedMultiplier, prefix) {
    const results = [];
    let baselinePps = 0;
    for (const numWorkers of workerCounts) {
      const groupId = `${prefix}-${numWorkers}-${host.nowMs() % 1e5}`;
      try {
        host.createShardGroup({
          groupId,
          actorType: "fetcher",
          shardCount: numWorkers,
          partitionStrategy: "hash",
          rebalancePolicy: "manual",
          placement: { strategy: "from_registry" },
          initialState: {}
        });
      } catch (e) {
        results.push({
          workers: numWorkers,
          pages: 0,
          elapsed_ms: 0,
          coord_ms: 0,
          fetch_ms: 0,
          pages_per_sec: 0,
          speedup: 0,
          efficiency_pct: 0,
          total_words: 0,
          error_count: 1,
          error: String(e)
        });
        continue;
      }
      const pagesPerWorker = getPagesPerWorker(numWorkers);
      const t0 = host.nowMs();
      let sgResult = null;
      try {
        sgResult = host.scatterGather({
          groupId,
          query: { op: "benchmark_crawl", pages_per_worker: pagesPerWorker, num_passes: numPasses, seed: numWorkers * seedMultiplier },
          timeoutMs: 6e4
        });
      } catch (e) {
        const elapsed2 = host.nowMs() - t0;
        results.push({
          workers: numWorkers,
          pages: 0,
          elapsed_ms: elapsed2,
          coord_ms: elapsed2,
          fetch_ms: 0,
          pages_per_sec: 0,
          speedup: 0,
          efficiency_pct: 0,
          total_words: 0,
          error_count: 1,
          error: String(e)
        });
        continue;
      }
      const elapsed = host.nowMs() - t0;
      let totalWords = 0;
      let totalCrawled = 0;
      let totalCompute = 0;
      let errorCount = 0;
      for (const sr of sgResult?.shardResponses ?? []) {
        const p = normalizePayload(sr.payload ?? {});
        if (!sr.success || p.error) {
          errorCount++;
          continue;
        }
        totalWords += Number(p.total_words ?? 0);
        totalCrawled += Number(p.pages_crawled ?? 0);
        totalCompute += Number(p.compute_ms ?? 0);
      }
      const avgComputeMs = numWorkers > 0 ? Math.round(totalCompute / numWorkers) : 0;
      const coordMs = Math.max(elapsed - avgComputeMs, 1);
      const pps = elapsed > 0 ? totalCrawled * 1e3 / elapsed : 0;
      if (!baselinePps && pps > 0) baselinePps = pps;
      const speedup = baselinePps > 0 ? pps / baselinePps : 1;
      const efficiency = speedup / numWorkers * 100;
      results.push({
        workers: numWorkers,
        pages: totalCrawled,
        elapsed_ms: elapsed,
        coord_ms: coordMs,
        fetch_ms: avgComputeMs,
        pages_per_sec: pps,
        speedup,
        efficiency_pct: efficiency,
        total_words: totalWords,
        error_count: errorCount
      });
    }
    return results;
  }
  // Strong scaling: fixed total 200 pages split across N workers (pages_per_worker=ceil(200/N))
  onBenchmark(payload) {
    const workerCountsRaw = payload.worker_counts ?? [2, 4, 8, 16];
    const workerCounts = workerCountsRaw.map(Number);
    const totalPages = Number(payload.pages_per_round ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const results = this.runBenchmarkSg(
      workerCounts,
      (n) => Math.ceil(totalPages / n),
      numPasses,
      100,
      "bench-strong"
    );
    return { status: "ok", results };
  }
  // Weak scaling: fixed pages_per_worker (200) × N workers — total work grows with N.
  onRun_weak_scaling_benchmark(payload) {
    const shardCountsRaw = payload.shard_counts ?? [2, 4, 8, 16];
    const shardCounts = shardCountsRaw.map(Number);
    const pagesPerWorker = Number(payload.pages_per_worker ?? 200);
    const numPasses = Number(payload.num_passes ?? 4);
    const results = this.runBenchmarkSg(
      shardCounts,
      (_n) => pagesPerWorker,
      numPasses,
      200,
      "bench-weak"
    );
    return { status: "ok", results };
  }
  onStatus() {
    return { ...this.state };
  }
};
function normalizePayload(m) {
  if ("status" in m || "pages_fetched" in m) return m;
  for (const k of ["payload", "result", "response", "data"]) {
    const nested = m[k];
    if (nested && typeof nested === "object" && !Array.isArray(nested)) {
      return normalizePayload(nested);
    }
  }
  return m;
}
var router = new ActorRouter({
  orchestrator: () => new WebCrawlOrchestrator(),
  fetcher: () => new PageFetcher(),
  analyzer: () => new LinkAnalyzer()
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
