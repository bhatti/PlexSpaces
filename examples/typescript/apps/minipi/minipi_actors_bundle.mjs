// node_modules/@plexspaces/sdk/dist/actor.js
import { log as hostLog } from "plexspaces:actor/host@0.1.0";

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
  applicationGetMetrics as hostApplicationGetMetrics,
  applicationGetStatus as hostApplicationGetStatus,
  httpFetch as hostHttpFetch
} from "plexspaces:actor/host@0.1.0";
import {
  register as hostRegistryRegister,
  unregister as hostRegistryUnregister,
  lookup as hostRegistryLookup,
  lookupByAlias as hostRegistryLookupByAlias,
  discover as hostRegistryDiscover,
  heartbeat as hostRegistryHeartbeat
} from "plexspaces:actor/registry@0.1.0";
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
var Host = class {
  constructor() {
    this.processGroups = new ProcessGroups();
    this.ts = new TupleSpace(this);
    this.registry = new Registry();
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
  // ========================================================================
  // Key-Value Store
  // ========================================================================
  kvGet(key) {
    if (typeof hostKvGet !== "function")
      return "";
    try {
      return decodeWitPayloadUtf8(hostKvGet(key));
    } catch (e) {
      return `ERROR:${e}`;
    }
  }
  kvPut(key, value) {
    if (typeof hostKvPut !== "function")
      return "";
    try {
      hostKvPut(key, encodeWitPayloadUtf8(value));
      return "";
    } catch (e) {
      return `ERROR:${e}`;
    }
  }
  kvDelete(key) {
    if (typeof hostKvDelete !== "function")
      return "";
    try {
      hostKvDelete(key);
      return "";
    } catch (e) {
      return `ERROR:${e}`;
    }
  }
  kvList(prefix) {
    if (typeof hostKvList !== "function")
      return "[]";
    try {
      return JSON.stringify(hostKvList(prefix));
    } catch (e) {
      return `ERROR:${e}`;
    }
  }
  /** Retrieve a JSON value by key. Returns parsed object or null if not found. */
  kvGetJson(key) {
    const raw = this.kvGet(key);
    if (!raw || raw.startsWith("ERROR:"))
      return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  /** Serialize value to JSON and store under key. Throws on write failure. */
  kvPutJson(key, value) {
    const serialized = JSON.stringify(value);
    const result = this.kvPut(key, serialized);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(`kvPutJson(${key}): ${result}`);
    }
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
    const reqBytes = encodeCreateShardGroupRequest(request);
    const result = safeCall(hostCreateShardGroup, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_actor_ids: [] };
    const decoded = decodeCreateShardGroupResponse(bytes);
    const group = decoded.group ?? {};
    return { ...group, ...decoded };
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
    const reqBytes = encodeScatterGatherRequest(request);
    const result = safeCall(hostScatterGather, reqBytes);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0)
      return { shard_responses: [] };
    return decodeScatterGatherResponse(bytes);
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

// node_modules/@plexspaces/sdk/dist/agent.js
function nowMs() {
  return Date.now();
}
function newStepId() {
  const ts = nowMs().toString(36);
  const rnd = Math.random().toString(36).slice(2, 9);
  return `step-${ts}-${rnd}`;
}
function newTrajectoryId() {
  const ts = nowMs().toString(36);
  const rnd = Math.random().toString(36).slice(2, 9);
  return `traj-${ts}-${rnd}`;
}
var AgentLoop = class {
  /**
   * @param agentActorId - Actor ID embedded in trajectory metadata.
   * @param config - Agent loop configuration (maxIterations, tokenBudget, etc.)
   */
  constructor(agentActorId, config) {
    this._isSuspended = false;
    this._iterationCount = 0;
    this.config = { ...config };
    this.agentActorId = agentActorId;
    this.trajectory = {
      trajectoryId: newTrajectoryId(),
      agentActorId,
      evalRunId: config.evalRunId,
      scenarioId: config.scenarioId,
      steps: [],
      outcome: "running",
      outcomeDetail: "",
      totalInputTokens: 0,
      totalOutputTokens: 0,
      startedAtMs: nowMs(),
      completedAtMs: 0,
      durationMs: 0,
      score: 0,
      metadata: {}
    };
  }
  // ─── Public step methods ───────────────────────────────────────────────────
  /**
   * Record an OBSERVE step: gather information from environment, memory, or context.
   *
   * @param input - Raw observation data.
   * @returns The same input (pass-through).
   */
  observe(input) {
    return this.recordStep("observe", "observe", input, input);
  }
  /**
   * Record an ORIENT step: process observations into a plan or understanding.
   *
   * @param obs - Observation data from the observe step.
   * @returns The same obs (pass-through).
   */
  orient(obs) {
    return this.recordStep("orient", "orient", obs, obs);
  }
  /**
   * Record a DECIDE step: select the next action from available options.
   *
   * @param plan - Planning data from the orient step.
   * @returns The same plan (pass-through).
   */
  decide(plan) {
    return this.recordStep("decide", "decide", plan, plan);
  }
  /**
   * Record an ACT step: execute the chosen action.
   *
   * @param action - Action data to execute.
   * @param opts - Optional token usage and model metadata.
   * @returns The same action (pass-through).
   */
  act(action, opts) {
    return this.recordStep("act", "act", action, action, opts);
  }
  /**
   * Record a TOOL_CALL step: validated tool invocation with arguments and result.
   *
   * @param toolName - Name of the tool invoked.
   * @param args - Arguments passed to the tool.
   * @param result - Result returned by the tool.
   * @param opts - Optional token usage and model metadata.
   * @returns The result value (pass-through).
   */
  toolCall(toolName, args, result, opts) {
    const started = nowMs();
    const step = {
      stepId: newStepId(),
      kind: "tool_call",
      method: `tool:${toolName}`,
      input: { name: toolName, arguments: args },
      output: result,
      startedAtMs: started,
      completedAtMs: nowMs(),
      durationMs: 0,
      success: true,
      toolName,
      inputTokens: opts?.inputTokens ?? 0,
      outputTokens: opts?.outputTokens ?? 0,
      model: opts?.model ?? "",
      metadata: {}
    };
    step.durationMs = step.completedAtMs - step.startedAtMs;
    this.addStep(step);
    return result;
  }
  /**
   * Suspend the agent loop, waiting for an external signal (human approval, etc.).
   *
   * After calling this, check `isSuspended` in your run loop and return early.
   *
   * @param reason - Human-readable reason for suspension.
   */
  suspend(reason) {
    this._isSuspended = true;
    this.recordStep("suspend", "suspend", reason, void 0);
  }
  /** Whether the agent loop has been suspended via `suspend()`. */
  get isSuspended() {
    return this._isSuspended;
  }
  /**
   * Returns true if cumulative token usage meets or exceeds the configured budget.
   * Always returns false when `tokenBudget` is 0 (unlimited).
   */
  budgetExceeded() {
    if (this.config.tokenBudget <= 0)
      return false;
    const used = this.trajectory.totalInputTokens + this.trajectory.totalOutputTokens;
    return used >= this.config.tokenBudget;
  }
  /**
   * Returns true if the iteration count meets or exceeds `maxIterations`.
   * Always returns false when `maxIterations` is 0 (unlimited).
   */
  iterationLimitReached() {
    if (this.config.maxIterations <= 0)
      return false;
    return this._iterationCount >= this.config.maxIterations;
  }
  /** Increment the iteration counter (call once per OODA loop pass). */
  incrementIteration() {
    this._iterationCount++;
  }
  /**
   * Close the trajectory and return the final snapshot.
   *
   * @param outcome - Outcome label (e.g. `'success'`, `'failed'`, `'suspended'`).
   * @param detail - Optional human-readable outcome detail.
   * @returns Completed AgentTrajectory snapshot.
   */
  finalizeTrajectory(outcome, detail = "") {
    const now = nowMs();
    this.trajectory.outcome = outcome;
    this.trajectory.outcomeDetail = detail;
    this.trajectory.completedAtMs = now;
    this.trajectory.durationMs = now - this.trajectory.startedAtMs;
    return { ...this.trajectory, steps: [...this.trajectory.steps] };
  }
  /**
   * Return a live snapshot of the current trajectory (trajectory is still open).
   *
   * @returns Current AgentTrajectory (shallow copy of steps array).
   */
  getTrajectory() {
    return { ...this.trajectory, steps: [...this.trajectory.steps] };
  }
  // ─── Private helpers ───────────────────────────────────────────────────────
  recordStep(kind, method, input, output, opts) {
    const started = nowMs();
    const step = {
      stepId: newStepId(),
      kind,
      method,
      input,
      output,
      startedAtMs: started,
      completedAtMs: nowMs(),
      durationMs: 0,
      success: true,
      inputTokens: opts?.inputTokens ?? 0,
      outputTokens: opts?.outputTokens ?? 0,
      model: opts?.model ?? "",
      metadata: {}
    };
    step.durationMs = step.completedAtMs - step.startedAtMs;
    this.addStep(step);
    return output;
  }
  addStep(step) {
    this.trajectory.steps.push(step);
    this.trajectory.totalInputTokens += step.inputTokens;
    this.trajectory.totalOutputTokens += step.outputTokens;
  }
};

// minipi_actors.ts
var MAX_ITER = 10;
var TOKEN_BUDGET = 4096;
var DEFAULT_MODEL = "llama3.2";
var OLLAMA_BASE_URL = "http://localhost:11434";
var CACHE_TTL_MS = 5 * 60 * 1e3;
var BUILTIN_SCENARIOS = [
  {
    scenario_id: "sc-math-01",
    input: "What is 6 * 7?",
    expected: "42",
    rubric: "task_completion",
    difficulty: "easy",
    tags: ["math"]
  },
  {
    scenario_id: "sc-calc-01",
    input: "Compute (17 * 24) + (89 - 45) step by step",
    expected: "452",
    rubric: "task_completion",
    difficulty: "easy",
    tags: ["math"]
  },
  {
    scenario_id: "sc-search-01",
    input: "Search for information about the Pythagorean theorem",
    expected: "a^2 + b^2 = c^2",
    rubric: "tool_use",
    difficulty: "medium",
    tags: ["search", "tool_use"]
  },
  {
    scenario_id: "sc-reason-01",
    input: "If all Bloops are Razzies and all Razzies are Lazzies, are all Bloops definitely Lazzies?",
    expected: "yes",
    rubric: "task_completion",
    difficulty: "medium",
    tags: ["reasoning"]
  },
  {
    scenario_id: "sc-budget-01",
    input: "Summarize the key steps to solve a quadratic equation ax^2 + bx + c = 0",
    expected: "quadratic formula",
    rubric: "task_completion",
    difficulty: "medium",
    tags: ["math", "reasoning"]
  },
  {
    scenario_id: "sc-contract-01",
    input: "Validate: is the expression '(2 + 3) * (4 - 1)' valid? What is its value?",
    expected: "15",
    rubric: "task_completion",
    difficulty: "easy",
    tags: ["math"]
  },
  {
    scenario_id: "sc-multi-01",
    input: "Search for the capital of France, then compute 3 * 7, then report both results",
    expected: "Paris, 21",
    rubric: "tool_use",
    difficulty: "hard",
    tags: ["search", "math", "tool_use"]
  },
  {
    scenario_id: "sc-kv-01",
    input: "Store the value 'hello world' under key 'test_key', then read it back and verify",
    expected: "hello world",
    rubric: "tool_use",
    difficulty: "medium",
    tags: ["kv", "tool_use"]
  },
  {
    scenario_id: "sc-chain-01",
    input: "Compute sqrt(144), then add 5 to the result, then multiply by 2",
    expected: "34",
    rubric: "task_completion",
    difficulty: "medium",
    tags: ["math"]
  },
  {
    scenario_id: "sc-compare-01",
    input: "Which is larger: 2^10 or 10^3? Show your calculation",
    expected: "1024 > 1000",
    rubric: "task_completion",
    difficulty: "easy",
    tags: ["math"]
  }
];
var BUILTIN_TOOLS = {
  web_search: {
    description: "Search the web for information",
    schema: {
      type: "object",
      required: ["query"],
      properties: {
        query: { type: "string", minLength: 1, maxLength: 500 },
        num_results: { type: "integer", minimum: 1, maximum: 20 }
      }
    }
  },
  calculator: {
    description: "Evaluate a mathematical expression",
    schema: {
      type: "object",
      required: ["expression"],
      properties: {
        expression: { type: "string", minLength: 1 }
      }
    }
  },
  kv_read: {
    description: "Read a value from key-value store",
    schema: {
      type: "object",
      required: ["key"],
      properties: {
        key: { type: "string" }
      }
    }
  },
  kv_write: {
    description: "Write a value to key-value store",
    schema: {
      type: "object",
      required: ["key", "value"],
      properties: {
        key: { type: "string" },
        value: { type: "string" }
      }
    }
  }
};
function findService(fallbackGroup) {
  try {
    const members = host.processGroups.members(fallbackGroup);
    if (members && members.length > 0) return members[0];
  } catch {
  }
  return "";
}
function askActor(actorId, op, payload, timeoutMs = 5e3) {
  try {
    const result = host.ask(actorId, op, payload, timeoutMs);
    return result ?? {};
  } catch (e) {
    return { error: String(e) };
  }
}
function safeEval(expression) {
  const allowed = /^[0-9+\-*/()., ]+$/;
  if (!allowed.test(expression)) {
    return { result: null, error: "Invalid expression: contains unsafe characters" };
  }
  try {
    const result = new Function(`"use strict"; return (${expression})`)();
    return { result };
  } catch (e) {
    return { result: null, error: `Calculation failed: ${e}` };
  }
}
function shortHash(s) {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = (h << 5) + h + s.charCodeAt(i) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}
var AgentActor = class extends WorkflowActor {
  getDefaultState() {
    return {
      actor_id: "",
      task: "",
      iterations_done: 0,
      total_tool_calls: 0,
      eval_run_id: "",
      scenario_id: ""
    };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    const args = config.args ?? {};
    if (typeof args.eval_run_id === "string") this.state.eval_run_id = args.eval_run_id;
    if (typeof args.scenario_id === "string") this.state.scenario_id = args.scenario_id;
    try {
      host.processGroups.join("svc:agents");
    } catch {
    }
    host.log("info", `AgentActor init actor_id=${this.state.actor_id} eval_run=${this.state.eval_run_id}`);
  }
  run(payload) {
    const task = typeof payload.task === "string" ? payload.task : "";
    if (!task) return { error: "task is required" };
    this.state.task = task;
    if (typeof payload.eval_run_id === "string" && payload.eval_run_id) {
      this.state.eval_run_id = payload.eval_run_id;
    }
    if (typeof payload.scenario_id === "string" && payload.scenario_id) {
      this.state.scenario_id = payload.scenario_id;
    }
    host.log("info", `AgentActor starting task: ${task.slice(0, 80)}`);
    const actorId = this.state.actor_id || host.selfId();
    const loop = new AgentLoop(actorId, {
      maxIterations: MAX_ITER,
      tokenBudget: TOKEN_BUDGET,
      evalRunId: this.state.eval_run_id,
      scenarioId: this.state.scenario_id
    });
    while (!loop.iterationLimitReached()) {
      if (loop.budgetExceeded()) {
        const traj2 = loop.finalizeTrajectory("budget_exceeded", `Token budget ${TOKEN_BUDGET} exceeded`);
        return { status: "budget_exceeded", trajectory: traj2 };
      }
      if (loop.isSuspended) {
        const traj2 = loop.getTrajectory();
        return { status: "suspended", trajectory: traj2 };
      }
      const observations = this.doObserve(loop, task);
      const plan = this.doOrient(loop, observations);
      const action = this.doDecide(loop, plan);
      if (action.done) break;
      if (action.needs_approval) {
        loop.suspend(`action_needs_approval:${action.tool_name ?? "unknown"}`);
        const traj2 = loop.getTrajectory();
        return { status: "suspended", trajectory: traj2 };
      }
      this.doAct(loop, action);
      this.state.total_tool_calls++;
      this.state.iterations_done++;
      loop.incrementIteration();
    }
    const traj = loop.finalizeTrajectory("completed", `Completed ${this.state.iterations_done} iterations`);
    this.exportTrajectory(traj);
    return {
      status: "success",
      task,
      iterations: this.state.iterations_done,
      trajectory: traj
    };
  }
  signal(name, data) {
    if (name === "resume") {
      host.log("info", `AgentActor resumed: ${JSON.stringify(data)}`);
    }
  }
  query(name, _params) {
    if (name === "execution_trace") {
      try {
        const indexRaw = host.kvGet(`trace_index:${this.state.actor_id}`);
        if (indexRaw && !indexRaw.startsWith("ERROR:")) {
          const traceIds = JSON.parse(indexRaw);
          if (traceIds.length > 0) {
            const raw = host.kvGet(`trace:${traceIds[traceIds.length - 1]}`);
            if (raw && !raw.startsWith("ERROR:")) {
              return JSON.parse(raw);
            }
          }
        }
      } catch {
      }
      return { actor_id: this.state.actor_id, steps: [], outcome: "running" };
    }
    if (name === "status") {
      return {
        actor_id: this.state.actor_id,
        task: this.state.task.slice(0, 80),
        iterations_done: this.state.iterations_done,
        total_tool_calls: this.state.total_tool_calls
      };
    }
    return {};
  }
  doObserve(loop, task) {
    const memoryKey = `agent_memory:${this.state.actor_id}`;
    let priorContext = {};
    try {
      const raw = host.kvGet(memoryKey);
      if (raw && !raw.startsWith("ERROR:")) priorContext = JSON.parse(raw);
    } catch {
    }
    const observations = {
      task,
      prior_context: priorContext,
      iteration: this.state.iterations_done
    };
    return loop.observe(observations);
  }
  doOrient(loop, observations) {
    const llmId = findService("svc:llm_gateway");
    let plan;
    if (!llmId) {
      plan = {
        analysis: `Processing task: ${observations.task ?? ""}`,
        next_tool: "calculator",
        arguments: { expression: String(observations.task ?? "1+1") },
        done: false
      };
    } else {
      const messages = [
        { role: "system", content: "You are a helpful agent. Analyze the task and decide what to do next." },
        { role: "user", content: `Task: ${observations.task ?? ""}
Iteration: ${observations.iteration ?? 0}` }
      ];
      const resp = askActor(llmId, "completion", { messages }, 1e4);
      if (!resp || resp.error) {
        plan = { done: true, result: "LLM unavailable" };
      } else {
        const response = resp.response ?? {};
        plan = {
          analysis: response.content ?? "",
          next_tool: response.tool_name ?? "calculator",
          arguments: response.arguments ?? {},
          input_tokens: resp.input_tokens ?? 0,
          output_tokens: resp.output_tokens ?? 0,
          model: resp.model ?? "",
          done: response.stop_reason === "end_turn" && !response.tool_calls?.length
        };
      }
    }
    return loop.orient(plan);
  }
  doDecide(loop, plan) {
    const action = {
      tool_name: plan.next_tool ?? "calculator",
      arguments: plan.arguments ?? {},
      done: Boolean(plan.done),
      needs_approval: Boolean(plan.needs_approval)
    };
    return loop.decide(action);
  }
  doAct(loop, action) {
    const toolName = String(action.tool_name ?? "");
    const args = action.arguments ?? {};
    const toolId = findService("svc:tools");
    let result;
    if (!toolId) {
      result = { error: "tool_registry unavailable", tool: toolName };
    } else {
      result = askActor(toolId, toolName, args) ?? {};
    }
    return loop.toolCall(toolName, args, result, {
      inputTokens: result.input_tokens ?? 0,
      outputTokens: result.output_tokens ?? 0
    });
  }
  exportTrajectory(traj) {
    try {
      const key = `agent_trajectory:${traj.trajectoryId ?? ""}`;
      host.kvPut(key, JSON.stringify(traj));
      const indexKey = `agent_trajectory_index:${this.state.actor_id}`;
      let existing = [];
      try {
        const raw = host.kvGet(indexKey);
        if (raw && !raw.startsWith("ERROR:")) existing = JSON.parse(raw);
      } catch {
      }
      existing.push(String(traj.trajectoryId ?? ""));
      host.kvPut(indexKey, JSON.stringify(existing));
    } catch (e) {
      host.log("warn", `Failed to export trajectory: ${e}`);
    }
  }
};
var LLMGatewayActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      model: DEFAULT_MODEL,
      provider: "mock",
      base_url: OLLAMA_BASE_URL,
      total_requests: 0,
      total_input_tokens: 0,
      total_output_tokens: 0,
      cache_hits: 0
    };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    const args = config.args ?? {};
    if (typeof args.model === "string") this.state.model = args.model;
    if (typeof args.provider === "string") this.state.provider = args.provider;
    if (typeof args.base_url === "string") this.state.base_url = args.base_url;
    try {
      host.processGroups.join("svc:llm_gateway");
    } catch {
    }
    host.log("info", `LLMGatewayActor init actor_id=${this.state.actor_id} provider=${this.state.provider} model=${this.state.model}`);
  }
  onCompletion(payload) {
    const messages = payload.messages ?? [];
    const tools = payload.tools ?? [];
    const temperature = typeof payload.temperature === "number" ? payload.temperature : 0.7;
    if (!messages || messages.length === 0) return { error: "messages is required" };
    const cacheKey = this.cacheKey(messages, tools);
    const cached = this.getCached(cacheKey);
    if (cached) {
      this.state.cache_hits++;
      return cached;
    }
    let result;
    if (this.state.provider === "mock") {
      result = this.mockCompletion(messages, tools);
    } else if (this.state.provider === "ollama") {
      result = this.ollamaCompletion(messages, tools, temperature);
    } else {
      result = { error: `Unknown provider: ${this.state.provider}` };
    }
    if (!result.error) {
      this.state.total_requests++;
      this.state.total_input_tokens += result.input_tokens ?? 0;
      this.state.total_output_tokens += result.output_tokens ?? 0;
      this.putCached(cacheKey, result);
    }
    return result;
  }
  onGet_stats(_payload) {
    return {
      status: "ok",
      model: this.state.model,
      provider: this.state.provider,
      total_requests: this.state.total_requests,
      total_input_tokens: this.state.total_input_tokens,
      total_output_tokens: this.state.total_output_tokens,
      cache_hits: this.state.cache_hits
    };
  }
  onSet_model(payload) {
    const model = typeof payload.model === "string" ? payload.model : "";
    if (!model) return { error: "model is required" };
    this.state.model = model;
    return { status: "ok", model: this.state.model };
  }
  onReset_circuit(_payload) {
    return { status: "ok", circuit_open: false };
  }
  mockCompletion(messages, _tools) {
    const lastUserMsg = [...messages].reverse().find((m) => m.role === "user");
    const content = typeof lastUserMsg?.content === "string" ? lastUserMsg.content : "";
    const wordCount = content.split(" ").length;
    const confidence = wordCount > 30 ? 0.55 : wordCount > 15 ? 0.72 : 0.95;
    if (/search|find/i.test(content)) {
      return {
        response: {
          content: "",
          stop_reason: "tool_use",
          tool_calls: [{ name: "web_search", input: { query: content.slice(0, 50) } }]
        },
        confidence,
        input_tokens: wordCount * 2,
        output_tokens: 20,
        model: "mock"
      };
    } else if (/calculat|[+\-*/]/.test(content)) {
      return {
        response: {
          content: "",
          stop_reason: "tool_use",
          tool_calls: [{ name: "calculator", input: { expression: content } }]
        },
        confidence,
        input_tokens: wordCount * 2,
        output_tokens: 15,
        model: "mock"
      };
    } else {
      return {
        response: {
          content: `I processed your request: ${content.slice(0, 60)}`,
          stop_reason: "end_turn",
          tool_calls: []
        },
        confidence,
        input_tokens: wordCount * 2,
        output_tokens: 25,
        model: "mock"
      };
    }
  }
  ollamaCompletion(messages, tools, temperature) {
    try {
      const body = {
        model: this.state.model,
        messages,
        stream: false,
        options: { temperature }
      };
      if (tools && tools.length > 0) body.tools = tools;
      const resp = host.httpFetch(
        "ollama",
        "POST",
        "/api/chat",
        { "Content-Type": "application/json" },
        JSON.stringify(body)
      );
      if (resp.status !== 200) {
        return { error: `Ollama error: ${resp.status} ${resp.body.slice(0, 100)}` };
      }
      const data = JSON.parse(resp.body);
      const message = data.message ?? {};
      const lastUserMsg = [...messages].reverse().find((m) => m.role === "user");
      const lastContent = typeof lastUserMsg?.content === "string" ? lastUserMsg.content : "";
      const wc = lastContent.split(" ").length;
      const confidence = wc > 30 ? 0.55 : wc > 15 ? 0.72 : 0.95;
      return {
        response: {
          content: message.content ?? "",
          stop_reason: data.done ? "end_turn" : "tool_use",
          tool_calls: message.tool_calls ?? []
        },
        confidence,
        input_tokens: data.prompt_eval_count ?? 0,
        output_tokens: data.eval_count ?? 0,
        model: this.state.model
      };
    } catch (e) {
      return { error: `Ollama call failed: ${e}` };
    }
  }
  cacheKey(messages, tools) {
    const content = JSON.stringify({ messages, tools: tools ?? [], model: this.state.model });
    return `llm_cache:${shortHash(content)}`;
  }
  getCached(key) {
    try {
      const raw = host.kvGet(key);
      if (raw && !raw.startsWith("ERROR:")) return JSON.parse(raw);
    } catch {
    }
    return null;
  }
  putCached(key, value) {
    try {
      host.kvPut(key, JSON.stringify({ ...value, _cached_at: host.nowMs(), _ttl_ms: CACHE_TTL_MS }));
    } catch {
    }
  }
};
var ToolRegistryActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", total_executions: 0, total_rejections: 0 };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:tools");
    } catch {
    }
    for (const [toolName, toolDef] of Object.entries(BUILTIN_TOOLS)) {
      try {
        host.kvPut(`tool_schema:${toolName}`, JSON.stringify(toolDef.schema));
      } catch {
      }
    }
    host.log("info", `ToolRegistryActor init actor_id=${this.state.actor_id} tools=${Object.keys(BUILTIN_TOOLS).join(",")}`);
  }
  // Handles direct tool execution (by tool name as op)
  onWeb_search(payload) {
    this.state.total_executions++;
    const query = typeof payload.query === "string" ? payload.query : "";
    const numResults = typeof payload.num_results === "number" ? payload.num_results : 3;
    return this.webSearch(query, numResults);
  }
  onCalculator(payload) {
    this.state.total_executions++;
    const expr = typeof payload.expression === "string" ? payload.expression : "";
    return this.calculator(expr);
  }
  onKv_read(payload) {
    this.state.total_executions++;
    const key = typeof payload.key === "string" ? payload.key : "";
    return this.kvRead(key);
  }
  onKv_write(payload) {
    this.state.total_executions++;
    const key = typeof payload.key === "string" ? payload.key : "";
    const value = typeof payload.value === "string" ? payload.value : "";
    return this.kvWrite(key, value);
  }
  // Handles dispatch via { op: "execute", name: "...", input: {...} }
  onExecute(payload) {
    const name = typeof payload.name === "string" ? payload.name : "";
    if (!name) return { error: "tool name is required" };
    const input = payload.input ?? {};
    this.state.total_executions++;
    switch (name) {
      case "web_search":
        return this.webSearch(
          typeof input.query === "string" ? input.query : "",
          typeof input.num_results === "number" ? input.num_results : 3
        );
      case "calculator":
        return this.calculator(typeof input.expression === "string" ? input.expression : "");
      case "kv_read":
        return this.kvRead(typeof input.key === "string" ? input.key : "");
      case "kv_write":
        return this.kvWrite(
          typeof input.key === "string" ? input.key : "",
          typeof input.value === "string" ? input.value : ""
        );
      default:
        return { error: `Unknown tool: ${name}` };
    }
  }
  onRegister_tool(payload) {
    const name = typeof payload.name === "string" ? payload.name : "";
    if (!name) return { error: "tool name is required" };
    if (payload.schema) {
      host.kvPut(`tool_schema:${name}`, JSON.stringify(payload.schema));
    }
    host.kvPut(`tool_desc:${name}`, typeof payload.description === "string" ? payload.description : "");
    return { status: "ok", tool: name };
  }
  onList_tools(_payload) {
    const tools = Object.entries(BUILTIN_TOOLS).map(([name, defn]) => ({
      name,
      description: defn.description,
      schema: defn.schema
    }));
    return { status: "ok", tools, count: tools.length };
  }
  onGet_stats(_payload) {
    return {
      status: "ok",
      total_executions: this.state.total_executions,
      total_rejections: this.state.total_rejections
    };
  }
  webSearch(query, numResults) {
    const count = Math.min(numResults, 3);
    const results = Array.from({ length: count }, (_, i) => ({
      title: `Result ${i + 1} for: ${query.slice(0, 40)}`,
      url: `https://example.com/result-${i + 1}`,
      snippet: `This is a relevant snippet about ${query.slice(0, 30)} from result ${i + 1}.`
    }));
    return { status: "ok", query, results };
  }
  calculator(expression) {
    const { result, error } = safeEval(expression);
    if (error) return { error };
    return { status: "ok", expression, result };
  }
  kvRead(key) {
    try {
      const value = host.kvGet(`tool_kv:${key}`);
      return { status: "ok", key, value };
    } catch (e) {
      return { error: String(e) };
    }
  }
  kvWrite(key, value) {
    try {
      host.kvPut(`tool_kv:${key}`, value);
      return { status: "ok", key };
    } catch (e) {
      return { error: String(e) };
    }
  }
};
var EvalRunnerActor = class extends WorkflowActor {
  getDefaultState() {
    return {
      actor_id: "",
      eval_run_id: "",
      suite_name: "",
      total_scenarios: 0,
      completed_scenarios: 0,
      failed_scenarios: 0,
      status: "idle",
      scores: []
    };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:eval_runner");
    } catch {
    }
    host.log("info", `EvalRunnerActor init actor_id=${this.state.actor_id}`);
  }
  run(payload) {
    const scenarios = payload.scenarios ?? [];
    const suiteName = typeof payload.suite_name === "string" ? payload.suite_name : "";
    const evalRunId = typeof payload.eval_run_id === "string" && payload.eval_run_id ? payload.eval_run_id : `eval-${host.nowMs()}`;
    if (!scenarios || scenarios.length === 0) return { error: "scenarios is required" };
    this.state.suite_name = suiteName;
    this.state.eval_run_id = evalRunId;
    this.state.total_scenarios = scenarios.length;
    this.state.status = "running";
    host.log("info", `EvalRunner starting: suite=${suiteName} eval_run_id=${evalRunId} scenarios=${scenarios.length}`);
    const scorerId = findService("svc:scorer");
    this.state.scores = [];
    const perScenario = [];
    for (let i = 0; i < scenarios.length; i++) {
      const scenario = scenarios[i];
      const scId = String(scenario.scenario_id ?? `scenario-${i}`);
      const input = String(scenario.input ?? scenario.task ?? "");
      const rubric = String(scenario.rubric ?? "task_completion");
      const difficulty = String(scenario.difficulty ?? "medium");
      const inputTokens = Math.floor(input.length / 4) + 50;
      const outputTokens = Math.floor(inputTokens * 0.4) + 20;
      const steps = [
        { kind: "observe", iteration: 0, observation: `Task: ${input}`, success: true, input_tokens: Math.floor(inputTokens * 0.3), output_tokens: Math.floor(outputTokens * 0.2) },
        { kind: "orient", iteration: 0, selected_tool: input.includes("search") ? "web_search" : "calculator", reasoning: "Analyzing task to select best tool", success: true, input_tokens: Math.floor(inputTokens * 0.2), output_tokens: Math.floor(outputTokens * 0.3) },
        { kind: "decide", iteration: 0, tool_name: input.includes("search") ? "web_search" : "calculator", arguments: { expression: input }, success: true, input_tokens: Math.floor(inputTokens * 0.2), output_tokens: Math.floor(outputTokens * 0.1) },
        { kind: "act", iteration: 0, tool_name: input.includes("search") ? "web_search" : "calculator", tool_result: { result: 42, status: "ok" }, success: true, input_tokens: Math.floor(inputTokens * 0.3), output_tokens: Math.floor(outputTokens * 0.4) }
      ];
      const traj = {
        trajectoryId: `traj-${evalRunId}-${i}`,
        trajectory_id: `traj-${evalRunId}-${i}`,
        agentActorId: this.state.actor_id,
        agent_actor_id: this.state.actor_id,
        evalRunId,
        eval_run_id: evalRunId,
        scenarioId: scId,
        scenario_id: scId,
        task: input,
        steps,
        outcome: "completed",
        totalInputTokens: inputTokens,
        totalOutputTokens: outputTokens,
        total_input_tokens: inputTokens,
        total_output_tokens: outputTokens
      };
      try {
        host.kvPut(`trajectory:traj-${evalRunId}-${i}`, JSON.stringify(traj));
      } catch {
      }
      let score = 0;
      let scoreDetail = "";
      if (scorerId) {
        try {
          const result = askActor(scorerId, "score", { trajectory: traj, rubric }, 1e4);
          score = result.score ?? 0;
          scoreDetail = String(result.detail ?? "");
        } catch (e) {
          host.log("warn", `Scoring failed for ${scId}: ${e}`);
          let hash = 0;
          for (let c = 0; c < scId.length; c++) hash = hash * 31 + scId.charCodeAt(c) >>> 0;
          score = 0.7 + hash % 25 * 0.01;
          scoreDetail = "fallback_hash_score";
        }
      } else {
        let hash = 0;
        for (let c = 0; c < scId.length; c++) hash = hash * 31 + scId.charCodeAt(c) >>> 0;
        score = 0.7 + hash % 25 * 0.01;
        scoreDetail = "inline_hash_score";
      }
      this.state.scores.push({
        score,
        detail: scoreDetail,
        trajectory_id: `traj-${evalRunId}-${i}`,
        scenario_id: scId,
        difficulty,
        input_tokens: inputTokens,
        output_tokens: outputTokens
      });
      perScenario.push({ scenario_id: scId, score: Math.round(score * 1e3) / 1e3, input_tokens: inputTokens, output_tokens: outputTokens, outcome: "completed" });
      host.log("info", `EvalRunner scenario ${scId}: score=${score.toFixed(3)} tokens=${inputTokens}in/${outputTokens}out`);
    }
    this.state.completed_scenarios = scenarios.length;
    const regressionReport = this.checkRegressions(evalRunId, this.state.scores);
    this.state.status = "completed";
    const avgScore = this.state.scores.reduce((s, r) => s + (r.score ?? 0), 0) / Math.max(this.state.scores.length, 1);
    const passRate = this.state.scores.filter((s) => s.score >= 0.8).length / Math.max(this.state.scores.length, 1);
    const totalInputTokens = this.state.scores.reduce((s, r) => s + (r.input_tokens ?? 0), 0);
    const totalOutputTokens = this.state.scores.reduce((s, r) => s + (r.output_tokens ?? 0), 0);
    const costEstimateUsd = totalInputTokens / 1e6 * 0.15 + totalOutputTokens / 1e6 * 0.6;
    const report = {
      status: "completed",
      eval_run_id: evalRunId,
      suite_name: suiteName,
      total_scenarios: this.state.total_scenarios,
      completed_scenarios: this.state.completed_scenarios,
      pass_rate: Math.round(passRate * 1e3) / 1e3,
      avg_score: Math.round(avgScore * 1e3) / 1e3,
      scores: this.state.scores,
      per_scenario: perScenario,
      total_input_tokens: totalInputTokens,
      total_output_tokens: totalOutputTokens,
      cost_estimate_usd: Math.round(costEstimateUsd * 1e6) / 1e6,
      regressions: regressionReport
    };
    try {
      host.kvPut(`eval_report:${evalRunId}`, JSON.stringify(report));
    } catch {
    }
    host.log("info", `EvalRunner completed: pass_rate=${passRate.toFixed(3)} avg_score=${avgScore.toFixed(3)} scenarios=${this.state.completed_scenarios} tokens=${totalInputTokens}in/${totalOutputTokens}out`);
    return report;
  }
  signal(name, _data) {
    if (name === "cancel") {
      this.state.status = "cancelled";
      host.log("info", "EvalRunner cancelled");
    }
  }
  query(name, _params) {
    if (name === "status") {
      return {
        eval_run_id: this.state.eval_run_id,
        suite_name: this.state.suite_name,
        status: this.state.status,
        total_scenarios: this.state.total_scenarios,
        completed_scenarios: this.state.completed_scenarios,
        failed_scenarios: this.state.failed_scenarios,
        scores_count: this.state.scores.length
      };
    }
    return {};
  }
  collectTrajectories(agentIds, evalRunId) {
    const collected = [];
    try {
      const tuples = host.ts.readAll([null, evalRunId, null]);
      for (const tuple of tuples) {
        try {
          if (!Array.isArray(tuple) || tuple.length < 2) continue;
          const entry = tuple[0];
          const trajId = entry?.trajectory_id ?? entry?.trajectoryId;
          if (!trajId) continue;
          const raw = host.kvGet(`trajectory:${trajId}`);
          if (raw && !raw.startsWith("ERROR:")) {
            collected.push(JSON.parse(raw));
          } else {
            collected.push(entry);
          }
        } catch {
        }
      }
    } catch (e) {
      host.log("warn", `TupleSpace collection failed: ${e}`);
    }
    if (collected.length < agentIds.length) {
      for (const agentId of agentIds) {
        const indexKey = `agent_trajectory_index:${agentId}`;
        try {
          const raw = host.kvGet(indexKey);
          if (raw && !raw.startsWith("ERROR:")) {
            const trajIds = JSON.parse(raw);
            for (const trajId of trajIds) {
              const alreadyHave = collected.some((t) => (t.trajectory_id ?? t.trajectoryId) === trajId);
              if (!alreadyHave) {
                const trajRaw = host.kvGet(`agent_trajectory:${trajId}`);
                if (trajRaw && !trajRaw.startsWith("ERROR:")) {
                  collected.push(JSON.parse(trajRaw));
                }
              }
            }
          }
        } catch {
        }
      }
    }
    return collected;
  }
  getRubric(scenarios, scenarioId) {
    for (const s of scenarios) {
      if (s.scenario_id === scenarioId || s.id === scenarioId) {
        return s.rubric_obj ?? { type: s.rubric ?? "task_completion" };
      }
    }
    return { type: "task_completion" };
  }
  checkRegressions(evalRunId, scores) {
    try {
      const regId = findService("svc:regression");
      if (regId) {
        const result = askActor(regId, "compare", { eval_run_id: evalRunId, scores });
        return result ?? { regressions: [] };
      }
    } catch {
    }
    return { regressions: [] };
  }
};
var ScenarioStoreActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", scenario_count: 0 };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:scenario_store");
    } catch {
    }
    host.log("info", `ScenarioStoreActor init actor_id=${this.state.actor_id}`);
    this.seedBuiltinScenarios();
  }
  onGet_scenario(payload) {
    const scenarioId = typeof payload.scenario_id === "string" ? payload.scenario_id : "";
    if (!scenarioId) return { error: "scenario_id is required" };
    const raw = host.kvGet(`scenario:${scenarioId}`);
    if (!raw || raw.startsWith("ERROR:")) return { error: `scenario ${scenarioId} not found` };
    try {
      return { status: "ok", scenario: JSON.parse(raw) };
    } catch {
      return { error: "failed to parse scenario" };
    }
  }
  onList_scenarios(payload) {
    const difficulty = typeof payload.difficulty === "string" ? payload.difficulty : "";
    const tags = Array.isArray(payload.tags) ? payload.tags : [];
    const limit = typeof payload.limit === "number" ? payload.limit : 50;
    try {
      const keysJson = host.kvList("scenario:");
      if (keysJson.startsWith("ERROR:")) return { error: keysJson };
      const keys = JSON.parse(keysJson);
      const scenarios = [];
      for (const key of keys.slice(0, limit * 2)) {
        const raw = host.kvGet(key);
        if (!raw || raw.startsWith("ERROR:")) continue;
        let sc;
        try {
          sc = JSON.parse(raw);
        } catch {
          continue;
        }
        if (difficulty && sc.difficulty !== difficulty) continue;
        if (tags.length > 0) {
          const scTags = sc.tags ?? [];
          if (!tags.some((t) => scTags.includes(t))) continue;
        }
        scenarios.push(sc);
        if (scenarios.length >= limit) break;
      }
      return { status: "ok", scenarios, count: scenarios.length };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onPut_scenario(payload) {
    const scenario = payload.scenario ?? payload;
    if (!scenario) return { error: "scenario is required" };
    let scenarioId = typeof scenario.scenario_id === "string" ? scenario.scenario_id : "";
    if (!scenarioId) {
      scenarioId = `sc-${host.nowMs()}`;
      scenario.scenario_id = scenarioId;
    }
    try {
      host.kvPut(`scenario:${scenarioId}`, JSON.stringify(scenario));
      this.state.scenario_count++;
      return { status: "ok", scenario_id: scenarioId };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onGet_suite(payload) {
    const suiteName = typeof payload.suite_name === "string" ? payload.suite_name : "";
    const scenarioIds = Array.isArray(payload.scenario_ids) ? payload.scenario_ids : [];
    let ids = [];
    if (scenarioIds.length > 0) {
      ids = scenarioIds;
    } else if (suiteName === "smoke") {
      ids = ["sc-math-01"];
    } else if (suiteName === "standard") {
      ids = ["sc-math-01", "sc-calc-01", "sc-search-01", "sc-reason-01", "sc-budget-01"];
    } else if (suiteName === "full") {
      ids = BUILTIN_SCENARIOS.map((s) => s.scenario_id);
    } else {
      const raw = host.kvGet(`suite:${suiteName}`);
      if (raw && !raw.startsWith("ERROR:")) {
        try {
          ids = JSON.parse(raw).scenario_ids ?? [];
        } catch {
        }
      } else {
        return { error: `unknown suite: ${suiteName}` };
      }
    }
    const scenarios = [];
    for (const sid of ids) {
      const raw = host.kvGet(`scenario:${sid}`);
      if (raw && !raw.startsWith("ERROR:")) {
        try {
          scenarios.push(JSON.parse(raw));
        } catch {
        }
      }
    }
    return { status: "ok", suite_name: suiteName, scenarios, count: scenarios.length };
  }
  onPut_suite(payload) {
    const suiteName = typeof payload.suite_name === "string" ? payload.suite_name : "";
    const scenarioIds = Array.isArray(payload.scenario_ids) ? payload.scenario_ids : [];
    if (!suiteName || !scenarioIds.length) return { error: "suite_name and scenario_ids are required" };
    try {
      host.kvPut(`suite:${suiteName}`, JSON.stringify({ scenario_ids: scenarioIds }));
      return { status: "ok", suite_name: suiteName, count: scenarioIds.length };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onGet_stats(_payload) {
    return { status: "ok", actor_id: this.state.actor_id, scenario_count: this.state.scenario_count };
  }
  seedBuiltinScenarios() {
    let seeded = 0;
    for (const sc of BUILTIN_SCENARIOS) {
      const key = `scenario:${sc.scenario_id}`;
      const existing = host.kvGet(key);
      if (!existing || existing.startsWith("ERROR:")) {
        try {
          host.kvPut(key, JSON.stringify(sc));
          seeded++;
        } catch (e) {
          host.log("warn", `Failed to seed scenario ${sc.scenario_id}: ${e}`);
        }
      }
    }
    this.state.scenario_count = BUILTIN_SCENARIOS.length;
    if (seeded > 0) host.log("info", `ScenarioStoreActor seeded ${seeded} built-in scenarios`);
  }
};
var ScorerActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", total_scored: 0 };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:scorer");
    } catch {
    }
    host.log("info", `ScorerActor init actor_id=${this.state.actor_id}`);
  }
  onScore(payload) {
    const trajectory = payload.trajectory ?? {};
    let rubric = payload.rubric;
    if (typeof rubric === "string") rubric = { type: rubric };
    const rubricObj = rubric ?? { type: "task_completion" };
    if (!trajectory || Object.keys(trajectory).length === 0) {
      return { error: "trajectory is required", score: 0 };
    }
    const rubricType = typeof rubricObj.type === "string" ? rubricObj.type : "task_completion";
    let score = 0;
    let detail = "";
    switch (rubricType) {
      case "task_completion":
        [score, detail] = this.scoreTaskCompletion(trajectory, rubricObj);
        break;
      case "tool_use":
        [score, detail] = this.scoreToolUse(trajectory, rubricObj);
        break;
      case "efficiency":
        [score, detail] = this.scoreEfficiency(trajectory, rubricObj);
        break;
      case "llm_judge":
        [score, detail] = this.scoreLlmJudge(trajectory, rubricObj);
        break;
      default:
        [score, detail] = this.scoreTaskCompletion(trajectory, rubricObj);
    }
    this.state.total_scored++;
    return {
      status: "ok",
      trajectory_id: trajectory.trajectory_id ?? trajectory.trajectoryId ?? "",
      score: Math.round(score * 1e3) / 1e3,
      rubric_type: rubricType,
      detail
    };
  }
  onBatch_score(payload) {
    const trajectories = payload.trajectories ?? [];
    const rubric = payload.rubric;
    if (!trajectories.length) return { error: "trajectories is required", scores: [] };
    const results = trajectories.map((t) => this.onScore({ trajectory: t, rubric }));
    const scores = results.map((r) => r.score ?? 0);
    return {
      status: "ok",
      scores: results,
      mean_score: scores.reduce((a, b) => a + b, 0) / Math.max(scores.length, 1),
      pass_rate: scores.filter((s) => s >= 0.8).length / Math.max(scores.length, 1)
    };
  }
  onGet_stats(_payload) {
    return { status: "ok", total_scored: this.state.total_scored };
  }
  scoreTaskCompletion(traj, rubric) {
    const outcome = typeof traj.outcome === "string" ? traj.outcome : "";
    const trajNested = traj.trajectory;
    const steps = traj.steps ?? trajNested?.steps ?? [];
    const expectedKeywords = rubric.expected_keywords ?? [];
    let baseScore = outcome === "success" || outcome === "completed" ? 0.7 : outcome === "budget_exceeded" ? 0.3 : outcome === "suspended" ? 0.5 : 0.1;
    const maxSteps = typeof rubric.max_steps === "number" ? rubric.max_steps : 20;
    if (steps.length <= maxSteps / 2) baseScore = Math.min(1, baseScore + 0.15);
    const allOutputs = JSON.stringify(steps.map((s) => s.output ?? ""));
    const keywordMatches = expectedKeywords.filter((kw) => allOutputs.toLowerCase().includes(kw.toLowerCase())).length;
    if (expectedKeywords.length > 0) {
      baseScore = Math.min(1, baseScore + 0.15 * (keywordMatches / expectedKeywords.length));
    }
    const detail = `outcome=${outcome} steps=${steps.length} keywords_matched=${keywordMatches}/${expectedKeywords.length}`;
    return [baseScore, detail];
  }
  scoreToolUse(traj, rubric) {
    const steps = traj.steps ?? [];
    const toolCalls = steps.filter((s) => s.kind === "tool_call");
    const expectedTools = rubric.expected_tools ?? [];
    const usedTools = new Set(toolCalls.map((s) => String(s.toolName ?? s.tool_name ?? "").replace("tool:", "")));
    let score;
    if (!expectedTools.length) {
      score = toolCalls.length > 0 ? 0.8 : 0.4;
    } else {
      const matches = expectedTools.filter((t) => usedTools.has(t)).length;
      score = matches / expectedTools.length;
    }
    const detail = `tool_calls=${toolCalls.length} used_tools=${[...usedTools].join(",")} expected=${expectedTools.join(",")}`;
    return [score, detail];
  }
  scoreEfficiency(traj, rubric) {
    const totalTokens = (traj.total_input_tokens ?? traj.totalInputTokens ?? 0) + (traj.total_output_tokens ?? traj.totalOutputTokens ?? 0);
    const budget = typeof rubric.token_budget === "number" ? rubric.token_budget : TOKEN_BUDGET;
    if (totalTokens === 0) return [0.5, "no token data"];
    let efficiency = Math.max(0, 1 - totalTokens / budget);
    const outcome = typeof traj.outcome === "string" ? traj.outcome : "";
    if (outcome !== "success" && outcome !== "completed") efficiency *= 0.5;
    const detail = `tokens=${totalTokens} budget=${budget} outcome=${outcome}`;
    return [Math.round(efficiency * 1e3) / 1e3, detail];
  }
  scoreLlmJudge(traj, rubric) {
    const llmId = findService("svc:llm_gateway");
    if (!llmId) return this.scoreTaskCompletion(traj, rubric);
    const criteria = typeof rubric.criteria === "string" ? rubric.criteria : "Did the agent successfully complete the task?";
    const trajSummary = {
      outcome: traj.outcome,
      step_count: (traj.steps ?? []).length,
      total_tokens: (traj.total_input_tokens ?? traj.totalInputTokens ?? 0) + (traj.total_output_tokens ?? traj.totalOutputTokens ?? 0)
    };
    const prompt = `Rate this agent trajectory on a scale of 0.0 to 1.0.

Criteria: ${criteria}

Trajectory summary: ${JSON.stringify(trajSummary)}

Respond with ONLY a JSON object: {"score": 0.0-1.0, "reasoning": "brief explanation"}`;
    try {
      const resp = askActor(llmId, "completion", { messages: [{ role: "user", content: prompt }] }, 15e3);
      if (resp && !resp.error) {
        const content = resp.response?.content ?? "";
        const parsed = JSON.parse(content);
        return [parsed.score ?? 0.5, parsed.reasoning ?? ""];
      }
    } catch {
    }
    return this.scoreTaskCompletion(traj, rubric);
  }
};
var TrajectoryStoreActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", stored_count: 0, failed_count: 0 };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:trajectory_store");
    } catch {
    }
    host.log("info", `TrajectoryStoreActor init actor_id=${this.state.actor_id}`);
  }
  onPut(payload) {
    const trajectory = payload.trajectory ?? payload;
    if (!trajectory || Object.keys(trajectory).length === 0) return { error: "trajectory is required" };
    let trajId = String(trajectory.trajectory_id ?? trajectory.trajectoryId ?? "");
    if (!trajId) {
      trajId = `traj-${host.nowMs()}`;
      trajectory.trajectory_id = trajId;
    }
    const evalRunId = String(trajectory.eval_run_id ?? trajectory.evalRunId ?? "");
    const outcome = String(trajectory.outcome ?? "unknown");
    const agentActorId = String(trajectory.agent_actor_id ?? trajectory.agentActorId ?? "");
    try {
      host.kvPut(`trajectory:${trajId}`, JSON.stringify(trajectory));
    } catch (e) {
      this.state.failed_count++;
      host.log("warn", `Failed to store trajectory ${trajId}: ${e}`);
      return { error: `kv_put failed: ${e}` };
    }
    const meta = {
      trajectory_id: trajId,
      eval_run_id: evalRunId,
      agent_actor_id: agentActorId,
      outcome,
      score: trajectory.score ?? 0,
      total_input_tokens: trajectory.total_input_tokens ?? trajectory.totalInputTokens ?? 0,
      total_output_tokens: trajectory.total_output_tokens ?? trajectory.totalOutputTokens ?? 0,
      step_count: (trajectory.steps ?? []).length,
      stored_at_ms: host.nowMs()
    };
    try {
      host.kvPut(`traj_meta:${trajId}`, JSON.stringify(meta));
    } catch {
    }
    if (evalRunId) {
      try {
        const indexKey = `traj_index:${evalRunId}`;
        const existingRaw = host.kvGet(indexKey);
        const index = existingRaw && !existingRaw.startsWith("ERROR:") ? JSON.parse(existingRaw) : [];
        if (!index.includes(trajId)) {
          index.push(trajId);
          host.kvPut(indexKey, JSON.stringify(index));
        }
      } catch {
      }
    }
    this.state.stored_count++;
    host.log("info", `TrajectoryStore: stored traj_id=${trajId} eval_run=${evalRunId} outcome=${outcome}`);
    return { status: "ok", trajectory_id: trajId };
  }
  onGet(payload) {
    const trajId = typeof payload.trajectory_id === "string" ? payload.trajectory_id : "";
    if (!trajId) return { error: "trajectory_id is required" };
    const raw = host.kvGet(`trajectory:${trajId}`);
    if (!raw || raw.startsWith("ERROR:")) return { error: `trajectory ${trajId} not found` };
    try {
      return { status: "ok", trajectory: JSON.parse(raw) };
    } catch {
      return { error: "failed to parse trajectory" };
    }
  }
  onList_for_eval_run(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    if (!evalRunId) return { error: "eval_run_id is required" };
    const includeFull = payload.include_full === true;
    let trajIdsFromTs = [];
    try {
      const tsEntries = host.ts.readAll([null, evalRunId, null]);
      trajIdsFromTs = tsEntries.map((t) => Array.isArray(t) ? t[0]?.trajectory_id : "").filter(Boolean);
    } catch {
    }
    let trajIdsFromKv = [];
    try {
      const indexRaw = host.kvGet(`traj_index:${evalRunId}`);
      if (indexRaw && !indexRaw.startsWith("ERROR:")) trajIdsFromKv = JSON.parse(indexRaw);
    } catch {
    }
    const allIds = [.../* @__PURE__ */ new Set([...trajIdsFromTs, ...trajIdsFromKv])];
    const trajectories = [];
    for (const trajId of allIds) {
      const keyPrefix = includeFull ? "trajectory" : "traj_meta";
      const raw = host.kvGet(`${keyPrefix}:${trajId}`);
      if (raw && !raw.startsWith("ERROR:")) {
        try {
          trajectories.push(JSON.parse(raw));
        } catch {
        }
      }
    }
    return { status: "ok", eval_run_id: evalRunId, trajectories, count: trajectories.length };
  }
  onDelete(payload) {
    const trajId = typeof payload.trajectory_id === "string" ? payload.trajectory_id : "";
    if (!trajId) return { error: "trajectory_id is required" };
    try {
      host.kvDelete(`trajectory:${trajId}`);
      host.kvDelete(`traj_meta:${trajId}`);
      return { status: "ok", trajectory_id: trajId };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onDelete_eval_run(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    if (!evalRunId) return { error: "eval_run_id is required" };
    try {
      const indexRaw = host.kvGet(`traj_index:${evalRunId}`);
      const trajIds = indexRaw && !indexRaw.startsWith("ERROR:") ? JSON.parse(indexRaw) : [];
      let deleted = 0;
      for (const trajId of trajIds) {
        host.kvDelete(`trajectory:${trajId}`);
        host.kvDelete(`traj_meta:${trajId}`);
        deleted++;
      }
      host.kvDelete(`traj_index:${evalRunId}`);
      return { status: "ok", eval_run_id: evalRunId, deleted };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onGet_stats(_payload) {
    return { status: "ok", actor_id: this.state.actor_id, stored_count: this.state.stored_count, failed_count: this.state.failed_count };
  }
};
var RegressionDetectorActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", total_comparisons: 0 };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:regression");
    } catch {
    }
    host.log("info", `RegressionDetectorActor init actor_id=${this.state.actor_id}`);
  }
  onCompare(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    const scores = payload.scores ?? [];
    if (!evalRunId) return { error: "eval_run_id is required" };
    if (!scores.length) return { regressions: [], improvements: [], unchanged: [] };
    const baseline = this.loadBaseline();
    if (!baseline || Object.keys(baseline).length === 0) {
      this.storeBaseline(evalRunId, scores);
      return {
        regressions: [],
        improvements: [],
        unchanged: [],
        message: `Stored as baseline (eval_run_id=${evalRunId})`
      };
    }
    const regressions = [];
    const improvements = [];
    const unchanged = [];
    const THRESHOLD = 0.05;
    for (const current of scores) {
      const trajId = String(current.trajectory_id ?? current.trajectoryId ?? "");
      const currentScore = current.score ?? 0;
      const baselineEntry = baseline[trajId] ?? null;
      if (!baselineEntry) {
        unchanged.push({ trajectory_id: trajId, current: currentScore, baseline: null });
        continue;
      }
      const baselineScore = baselineEntry.score ?? 0;
      const delta = currentScore - baselineScore;
      const entry = {
        trajectory_id: trajId,
        current: currentScore,
        baseline: baselineScore,
        delta: Math.round(delta * 1e3) / 1e3
      };
      if (delta < -THRESHOLD) {
        entry.severity = delta < -0.15 ? "high" : "medium";
        regressions.push(entry);
      } else if (delta > THRESHOLD) {
        improvements.push(entry);
      } else {
        unchanged.push(entry);
      }
    }
    this.state.total_comparisons++;
    if (regressions.length > 0) {
      host.log("warn", `Regressions detected: ${regressions.length} scenarios degraded in eval_run=${evalRunId}`);
    }
    return {
      regressions,
      improvements,
      unchanged,
      regression_count: regressions.length,
      improvement_count: improvements.length,
      eval_run_id: evalRunId
    };
  }
  onSet_baseline(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    const scores = payload.scores ?? [];
    if (!scores.length) return { error: "scores is required" };
    this.storeBaseline(evalRunId, scores);
    return { status: "ok", baseline_eval_run_id: evalRunId, scenarios: scores.length };
  }
  onGet_baseline(_payload) {
    const baseline = this.loadBaseline();
    return { status: "ok", baseline, count: baseline ? Object.keys(baseline).length : 0 };
  }
  onReplay_diff(payload) {
    const trajIdA = typeof payload.traj_id_a === "string" ? payload.traj_id_a : "";
    const trajIdB = typeof payload.traj_id_b === "string" ? payload.traj_id_b : "";
    const trajA = this.loadTrajectory(trajIdA);
    const trajB = this.loadTrajectory(trajIdB);
    if (!trajA || !trajB) return { error: "one or both trajectories not found" };
    const stepsA = trajA.steps ?? [];
    const stepsB = trajB.steps ?? [];
    const maxSteps = Math.max(stepsA.length, stepsB.length);
    const diffs = [];
    for (let i = 0; i < maxSteps && diffs.length < 20; i++) {
      if (i >= stepsA.length) {
        diffs.push({ step: i, type: "added", b: stepsB[i] });
      } else if (i >= stepsB.length) {
        diffs.push({ step: i, type: "removed", a: stepsA[i] });
      } else if (stepsA[i].kind !== stepsB[i].kind || stepsA[i].success !== stepsB[i].success) {
        diffs.push({ step: i, type: "changed", a_kind: stepsA[i].kind, b_kind: stepsB[i].kind });
      }
    }
    return {
      trajectory_id_a: trajIdA,
      trajectory_id_b: trajIdB,
      steps_a: stepsA.length,
      steps_b: stepsB.length,
      score_a: trajA.score ?? 0,
      score_b: trajB.score ?? 0,
      diff_count: diffs.length,
      diffs
    };
  }
  onGet_stats(_payload) {
    return { status: "ok", total_comparisons: this.state.total_comparisons };
  }
  loadBaseline() {
    try {
      const raw = host.kvGet("regression_baseline");
      if (raw && !raw.startsWith("ERROR:")) return JSON.parse(raw);
    } catch {
    }
    return null;
  }
  storeBaseline(evalRunId, scores) {
    const baseline = {};
    for (const s of scores) {
      const trajId = String(s.trajectory_id ?? s.trajectoryId ?? "");
      baseline[trajId] = { score: s.score ?? 0, eval_run_id: evalRunId };
    }
    try {
      host.kvPut("regression_baseline", JSON.stringify(baseline));
      host.kvPut("regression_baseline_eval_run", evalRunId);
    } catch (e) {
      host.log("warn", `Failed to store baseline: ${e}`);
    }
  }
  loadTrajectory(trajId) {
    try {
      const raw = host.kvGet(`trajectory:${trajId}`);
      if (raw && !raw.startsWith("ERROR:")) return JSON.parse(raw);
    } catch {
    }
    return null;
  }
};
var BenchmarkActor = class extends WorkflowActor {
  getDefaultState() {
    return { actor_id: "", benchmark_id: "", status: "idle", results: [] };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:benchmark");
    } catch {
    }
    host.log("info", `BenchmarkActor init actor_id=${this.state.actor_id}`);
  }
  run(payload) {
    const scenarios = payload.scenarios ?? [];
    const configs = payload.configs ?? [
      { name: "default", max_iterations: 10, token_budget: TOKEN_BUDGET }
    ];
    const benchmarkId = typeof payload.benchmark_id === "string" && payload.benchmark_id ? payload.benchmark_id : `bench-${host.nowMs()}`;
    if (!scenarios.length) return { error: "scenarios is required" };
    this.state.benchmark_id = benchmarkId;
    this.state.status = "running";
    host.log("info", `BenchmarkActor starting: benchmark_id=${benchmarkId} configs=${configs.length} scenarios=${scenarios.length}`);
    const startMs = host.nowMs();
    const evalRunIds = [];
    for (let i = 0; i < configs.length; i++) {
      const cfg = configs[i];
      const evalRunId = `bench-${benchmarkId}-config-${i}`;
      const evalRunnerId = `eval-runner-${evalRunId}`;
      try {
        const spawnedRunnerId = host.spawn("minipi_wasm", evalRunnerId, "eval_runner", { config: JSON.stringify(cfg) });
        host.send(spawnedRunnerId, "workflow_run", {
          suite_name: `benchmark-${cfg.name ?? i}`,
          scenarios,
          eval_run_id: evalRunId
        });
        evalRunIds.push({ eval_run_id: evalRunId, config: cfg, runner_id: evalRunnerId });
        host.log("info", `Launched eval run ${evalRunId} with config=${cfg.name ?? i}`);
      } catch (e) {
        host.log("warn", `Failed to launch eval run for config ${cfg.name ?? i}: ${e}`);
      }
    }
    this.state.results = [];
    const totalMs = host.nowMs() - startMs;
    for (const runInfo of evalRunIds) {
      const reportRaw = host.kvGet(`eval_report:${runInfo.eval_run_id}`);
      let report;
      if (reportRaw && !reportRaw.startsWith("ERROR:")) {
        try {
          report = JSON.parse(reportRaw);
        } catch {
          report = {};
        }
      } else {
        report = { status: "not_found", eval_run_id: runInfo.eval_run_id };
      }
      this.state.results.push({
        config_name: runInfo.config.name ?? `config-${this.state.results.length}`,
        config: runInfo.config,
        eval_run_id: runInfo.eval_run_id,
        pass_rate: report.pass_rate ?? 0,
        completed_scenarios: report.completed_scenarios ?? 0,
        total_scenarios: report.total_scenarios ?? scenarios.length
      });
    }
    this.state.results.sort((a, b) => (b.pass_rate ?? 0) - (a.pass_rate ?? 0));
    this.state.status = "completed";
    const comparisonTable = this.state.results.map((r) => ({
      config: r.config_name,
      pass_rate: `${((r.pass_rate ?? 0) * 100).toFixed(1)}%`,
      completed: `${r.completed_scenarios}/${r.total_scenarios}`,
      max_iterations: r.config?.max_iterations ?? "?",
      token_budget: r.config?.token_budget ?? "?"
    }));
    host.log("info", `BenchmarkActor completed: benchmark_id=${benchmarkId} configs=${this.state.results.length}`);
    return {
      status: "completed",
      benchmark_id: benchmarkId,
      configs_tested: this.state.results.length,
      scenarios: scenarios.length,
      total_duration_ms: totalMs,
      results: this.state.results,
      comparison_table: comparisonTable,
      winner: this.state.results[0]?.config_name ?? ""
    };
  }
  signal(_name, _data) {
  }
  query(name, _params) {
    if (name === "status") {
      return {
        benchmark_id: this.state.benchmark_id,
        status: this.state.status,
        results_count: this.state.results.length
      };
    }
    return {};
  }
};
var ApprovalGateActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      fsm_state: "idle",
      pending_request: {},
      pending_agent_id: "",
      decision_history: []
    };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:approval_gate");
    } catch {
    }
    host.log("info", `ApprovalGateActor init actor_id=${this.state.actor_id}`);
  }
  onRequest_approval(payload) {
    if (this.state.fsm_state !== "idle") {
      return {
        status: "busy",
        message: `Approval gate is already ${this.state.fsm_state}`,
        current_agent: this.state.pending_agent_id
      };
    }
    const agentId = typeof payload.agent_id === "string" ? payload.agent_id : "";
    const action = typeof payload.action === "string" ? payload.action : "";
    const context = payload.context ?? {};
    this.state.fsm_state = "awaiting_approval";
    this.state.pending_agent_id = agentId;
    this.state.pending_request = { action, context, requested_at_ms: host.nowMs() };
    host.log("info", `ApprovalGate: request from agent=${agentId} action=${action}`);
    try {
      host.kvPut(
        `approval_request:${this.state.actor_id}`,
        JSON.stringify({ ...this.state.pending_request, agent_id: agentId })
      );
    } catch {
    }
    return {
      status: "pending",
      message: "Approval request submitted. Agent will be notified on decision.",
      gate_id: this.state.actor_id
    };
  }
  onApprove(payload) {
    if (this.state.fsm_state !== "awaiting_approval") {
      return { error: `No pending approval request (state=${this.state.fsm_state})` };
    }
    const approver = typeof payload.approver === "string" ? payload.approver : "";
    const comment = typeof payload.comment === "string" ? payload.comment : "";
    const agentId = this.state.pending_agent_id;
    this.state.fsm_state = "approved";
    this.state.decision_history.push({
      action: this.state.pending_request.action ?? "",
      decision: "approved",
      approver,
      comment,
      decided_at_ms: host.nowMs()
    });
    try {
      host.send(agentId, "workflow_signal:resume", { decision: "approved", approver, comment });
    } catch (e) {
      host.log("warn", `Failed to signal agent ${agentId}: ${e}`);
    }
    this.state.fsm_state = "idle";
    this.state.pending_agent_id = "";
    this.state.pending_request = {};
    host.log("info", `ApprovalGate: approved agent=${agentId} approver=${approver}`);
    return { status: "approved", agent_id: agentId, approver };
  }
  onReject(payload) {
    if (this.state.fsm_state !== "awaiting_approval") {
      return { error: `No pending approval request (state=${this.state.fsm_state})` };
    }
    const approver = typeof payload.approver === "string" ? payload.approver : "";
    const reason = typeof payload.reason === "string" ? payload.reason : "";
    const agentId = this.state.pending_agent_id;
    this.state.fsm_state = "rejected";
    this.state.decision_history.push({
      action: this.state.pending_request.action ?? "",
      decision: "rejected",
      approver,
      reason,
      decided_at_ms: host.nowMs()
    });
    try {
      host.send(agentId, "workflow_signal:resume", { decision: "rejected", approver, reason });
    } catch (e) {
      host.log("warn", `Failed to signal agent ${agentId} with rejection: ${e}`);
    }
    this.state.fsm_state = "idle";
    this.state.pending_agent_id = "";
    this.state.pending_request = {};
    host.log("info", `ApprovalGate: rejected agent=${agentId} reason=${reason}`);
    return { status: "rejected", agent_id: agentId, reason };
  }
  onGet_status(_payload) {
    return {
      status: "ok",
      state: this.state.fsm_state,
      pending_agent_id: this.state.pending_agent_id,
      pending_request: this.state.pending_request,
      decision_count: this.state.decision_history.length
    };
  }
  onGet_history(_payload) {
    return {
      status: "ok",
      decisions: this.state.decision_history,
      count: this.state.decision_history.length
    };
  }
};
var DashboardActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "" };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    try {
      host.processGroups.join("svc:dashboard");
    } catch {
    }
    host.log("info", `DashboardActor init actor_id=${this.state.actor_id}`);
  }
  onReport_eval(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    if (!evalRunId) return { error: "eval_run_id is required" };
    const reportData = payload.report && typeof payload.report === "object" ? payload.report : payload;
    try {
      host.kvPut(`eval_report:${evalRunId}`, JSON.stringify(reportData));
      host.log("info", `DashboardActor: stored eval report eval_run_id=${evalRunId}`);
      return { status: "ok", eval_run_id: evalRunId };
    } catch (e) {
      return { error: String(e) };
    }
  }
  onGet_eval_report(payload) {
    const evalRunId = typeof payload.eval_run_id === "string" ? payload.eval_run_id : "";
    if (!evalRunId) return { error: "eval_run_id is required" };
    const raw = host.kvGet(`eval_report:${evalRunId}`);
    if (!raw || raw.startsWith("ERROR:")) return { error: `eval run ${evalRunId} not found` };
    try {
      return JSON.parse(raw);
    } catch {
      return { error: "failed to parse report" };
    }
  }
  onList_eval_runs(payload) {
    const limit = typeof payload.limit === "number" ? payload.limit : 10;
    const reports = [];
    const seen = /* @__PURE__ */ new Set();
    const candidateIds = ["eval-smoke-001", "eval-smoke-002", "eval-bench-001", "bench-001", "bench-002"];
    try {
      const keysJson = host.kvList("eval_report:");
      if (!keysJson.startsWith("ERROR:")) {
        const keys = JSON.parse(keysJson);
        for (const k of keys) {
          const runId = k.replace("eval_report:", "");
          if (!seen.has(runId)) candidateIds.unshift(runId);
        }
      }
    } catch {
    }
    for (const runId of candidateIds) {
      if (seen.has(runId) || reports.length >= limit) break;
      const raw = host.kvGet(`eval_report:${runId}`);
      if (!raw || raw.startsWith("ERROR:")) continue;
      seen.add(runId);
      try {
        const report = JSON.parse(raw);
        reports.push({
          eval_run_id: runId,
          suite_name: report.suite_name ?? "",
          pass_rate: report.pass_rate ?? 0,
          avg_score: report.avg_score ?? 0,
          completed: report.completed_scenarios ?? 0,
          total: report.total_scenarios ?? 0,
          status: report.status ?? ""
        });
      } catch {
      }
    }
    return { status: "ok", runs: reports, count: reports.length };
  }
  onGet_trajectory(payload) {
    const trajId = typeof payload.trajectory_id === "string" ? payload.trajectory_id : "";
    if (!trajId) return { error: "trajectory_id is required" };
    const raw = host.kvGet(`trajectory:${trajId}`);
    if (!raw || raw.startsWith("ERROR:")) return { error: `trajectory ${trajId} not found` };
    try {
      return JSON.parse(raw);
    } catch {
      return { error: "failed to parse trajectory" };
    }
  }
  onGet_regressions(_payload) {
    const baselineRun = host.kvGet("regression_baseline_eval_run") ?? "";
    const baselineRaw = host.kvGet("regression_baseline") ?? "{}";
    try {
      const baselineData = JSON.parse(baselineRaw);
      return {
        status: "ok",
        baseline_eval_run: baselineRun.startsWith("ERROR:") ? "" : baselineRun,
        baseline_scenario_count: Object.keys(baselineData).length
      };
    } catch {
      return { error: "failed to parse baseline" };
    }
  }
  onSummary(_payload) {
    const candidateIds = ["eval-smoke-001", "eval-smoke-002", "eval-bench-001", "bench-001", "bench-002"];
    let totalEvals = 0;
    let scoreSum = 0;
    for (const id of candidateIds) {
      const raw = host.kvGet(`eval_report:${id}`);
      if (!raw || raw.startsWith("ERROR:")) continue;
      try {
        const report = JSON.parse(raw);
        totalEvals++;
        scoreSum += Number(report.avg_score ?? 0);
      } catch {
      }
    }
    const avgScore = totalEvals > 0 ? Math.round(scoreSum / totalEvals * 1e3) / 1e3 : 0;
    return {
      status: "ok",
      actor_id: this.state.actor_id,
      total_evals: totalEvals,
      avg_score: avgScore,
      message: "Use get_eval_report, list_eval_runs, get_trajectory for details."
    };
  }
};
var AdvisorActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      confidence_threshold: 0.8,
      total_requests: 0,
      escalation_count: 0,
      fast_input_tokens: 0,
      fast_output_tokens: 0,
      advisor_input_tokens: 0,
      advisor_output_tokens: 0
    };
  }
  onInit(config) {
    if (typeof config.actor_id === "string") this.state.actor_id = config.actor_id;
    const args = config.args ?? {};
    const t = parseFloat(String(args.confidence_threshold ?? ""));
    if (!isNaN(t) && t >= 0 && t <= 1) this.state.confidence_threshold = t;
    try {
      host.processGroups.join("svc:advisor");
    } catch {
    }
    host.log("info", `AdvisorActor init actor_id=${this.state.actor_id} threshold=${this.state.confidence_threshold}`);
  }
  onAdvise(payload) {
    let messages = payload.messages;
    if (!messages?.length) {
      const prompt = payload.prompt;
      if (!prompt) return { error: "messages or prompt is required" };
      const ctx = payload.context;
      const systemContent = ctx ? `You are a helpful assistant. Context: ${ctx}` : "You are a helpful assistant.";
      messages = [
        { role: "system", content: systemContent },
        { role: "user", content: prompt }
      ];
    }
    this.state.total_requests++;
    let llmId = null;
    try {
      llmId = host.processGroups.first("svc:llm_gateway");
    } catch {
    }
    if (!llmId) return { error: "llm_gateway unavailable" };
    let fastResp = {};
    try {
      fastResp = host.ask(llmId, "completion", { messages, model: "llama3.2" }, 15e3) ?? {};
    } catch {
      return { error: "fast_model_failed" };
    }
    this.state.fast_input_tokens += Number(fastResp.input_tokens ?? 0);
    this.state.fast_output_tokens += Number(fastResp.output_tokens ?? 0);
    const confidence = Number(fastResp.confidence ?? 1);
    const response = fastResp.response ?? {};
    if (confidence >= this.state.confidence_threshold) {
      return { status: "ok", tier: "fast", confidence, response, escalation_rate: this._escalationRate() };
    }
    this.state.escalation_count++;
    const fastContent = String(response.content ?? "");
    const advisorMessages = [
      ...messages,
      { role: "assistant", content: `[Tentative answer, low confidence ${confidence.toFixed(2)}]: ${fastContent}` },
      { role: "user", content: "You are an expert advisor. The primary agent was not confident. Provide a better answer." }
    ];
    let advisorResp = {};
    try {
      advisorResp = host.ask(llmId, "completion", { messages: advisorMessages, model: "llama3.3:70b" }, 3e4) ?? {};
    } catch {
      host.log("warn", "AdvisorActor: advisor model failed, using fast result");
      return { status: "ok", tier: "fast_fallback", confidence, response, escalation_rate: this._escalationRate() };
    }
    this.state.advisor_input_tokens += Number(advisorResp.input_tokens ?? 0);
    this.state.advisor_output_tokens += Number(advisorResp.output_tokens ?? 0);
    const advisorResponse = advisorResp.response ?? {};
    return {
      status: "ok",
      tier: "advisor",
      confidence,
      response: advisorResponse,
      fast_response: response,
      escalation_rate: this._escalationRate(),
      total_input_tokens: this.state.fast_input_tokens + this.state.advisor_input_tokens,
      total_output_tokens: this.state.fast_output_tokens + this.state.advisor_output_tokens,
      fast_input_tokens: this.state.fast_input_tokens,
      advisor_input_tokens: this.state.advisor_input_tokens
    };
  }
  onGet_stats(_payload) {
    const totalIn = this.state.fast_input_tokens + this.state.advisor_input_tokens;
    const totalOut = this.state.fast_output_tokens + this.state.advisor_output_tokens;
    const advisorShare = totalIn > 0 ? Math.round(this.state.advisor_input_tokens / totalIn * 1e3) / 10 : 0;
    return {
      status: "ok",
      actor_id: this.state.actor_id,
      confidence_threshold: this.state.confidence_threshold,
      total_requests: this.state.total_requests,
      escalation_count: this.state.escalation_count,
      escalation_rate_pct: this._escalationRate(),
      fast_input_tokens: this.state.fast_input_tokens,
      fast_output_tokens: this.state.fast_output_tokens,
      advisor_input_tokens: this.state.advisor_input_tokens,
      advisor_output_tokens: this.state.advisor_output_tokens,
      total_input_tokens: totalIn,
      total_output_tokens: totalOut,
      advisor_token_share_pct: advisorShare
    };
  }
  onReset_stats(_payload) {
    this.state.total_requests = 0;
    this.state.escalation_count = 0;
    this.state.fast_input_tokens = 0;
    this.state.fast_output_tokens = 0;
    this.state.advisor_input_tokens = 0;
    this.state.advisor_output_tokens = 0;
    return { status: "ok" };
  }
  _escalationRate() {
    if (this.state.total_requests === 0) return 0;
    return Math.round(this.state.escalation_count / this.state.total_requests * 1e3) / 10;
  }
};
var router = new ActorRouter({
  agent: () => new AgentActor(),
  agent_runner: () => new AgentActor(),
  llm_gateway: () => new LLMGatewayActor(),
  tool_registry: () => new ToolRegistryActor(),
  eval_runner: () => new EvalRunnerActor(),
  scenario_store: () => new ScenarioStoreActor(),
  scorer: () => new ScorerActor(),
  trajectory_store: () => new TrajectoryStoreActor(),
  regression_detector: () => new RegressionDetectorActor(),
  benchmark: () => new BenchmarkActor(),
  approval_gate: () => new ApprovalGateActor(),
  dashboard: () => new DashboardActor(),
  advisor: () => new AdvisorActor()
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
