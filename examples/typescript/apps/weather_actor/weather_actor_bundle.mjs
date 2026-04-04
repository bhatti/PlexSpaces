// ../../../../sdks/typescript/dist/actor.js
import { log as hostLog } from "plexspaces:simple-actor/host@0.1.0";

// ../../../../sdks/typescript/dist/decorators.js
var ACTOR_METADATA = Symbol.for("plexspaces.actor.metadata");
function getActorDefinition(target) {
  const ctor = typeof target === "function" ? target : target.constructor;
  return Reflect.get(ctor, ACTOR_METADATA);
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
  /** WIT init(config-json) -> string. Empty string = success, "ERROR:..." = failure. */
  init(configJson) {
    try {
      const config = configJson && configJson.trim() ? JSON.parse(configJson) : {};
      this.onInit(config);
      this.cachedStateJson = null;
      return "";
    } catch {
      return "ERROR:init failed";
    }
  }
  /**
   * WIT handle(from-actor, msg-type, payload-json) -> result<string, string>.
   * Dispatches by msgType for Workflow behavior (workflow_run, workflow_signal:name, workflow_query:name),
   * then by payload.op (or payload) to on<Op>(payload). Returns JSON string.
   * Uses iterative serializer to avoid WASM recursion.
   *
   * Workflow behavior (aligned with Rust Workflow trait and Python @workflow_actor):
   * - msgType "workflow_run" -> run(payload)
   * - msgType "workflow_signal:name" -> signal(name, payload)
   * - msgType "workflow_query:name" -> query(name, payload)
   */
  handle(_fromActor, msgType, payloadJson) {
    try {
      const payload = payloadJson && payloadJson.trim() ? JSON.parse(payloadJson) : {};
      const definition = getActorDefinition(this);
      if (msgType === "workflow_run") {
        const runMethod = definition?.runHandler;
        const runFn = runMethod ? this[runMethod] : this.run;
        if (typeof runFn === "function") {
          const result = runFn.call(this, payload);
          this.cachedStateJson = null;
          return iterativeStringify(result ?? {});
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
          return "{}";
        }
      }
      if (msgType.startsWith("workflow_query:")) {
        const name = msgType.slice("workflow_query:".length).trim();
        const queryMethod = definition?.queryHandlers?.[name];
        const queryFn = queryMethod ? this[queryMethod] : this.query;
        if (typeof queryFn === "function") {
          const result = queryMethod ? queryFn.call(this, payload) : queryFn.call(this, name, payload);
          return iterativeStringify(result ?? {});
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
          return "ERROR:" + errorMsg;
        }
        this.cachedStateJson = null;
        try {
          return iterativeStringify(result);
        } catch (jsonError) {
          const errorMsg = jsonError instanceof Error ? jsonError.message : String(jsonError);
          actorLog("error", "actor.ts:handle", "JSON serialization failed", errorMsg);
          return "ERROR:JSON serialization failed: " + errorMsg;
        }
      }
      actorLog("warn", "actor.ts:handle", "Unknown operation", String(op));
      return iterativeStringify({ error: "unknown_op", op: String(op) });
    } catch (e) {
      const errorMsg = e instanceof Error ? e.message : String(e);
      actorLog("error", "actor.ts:handle", "Handle failed", errorMsg);
      return "ERROR:" + errorMsg;
    }
  }
  /** WIT get-state() -> string. Returns JSON-serialized state. */
  getState() {
    if (this.cachedStateJson !== null) {
      return this.cachedStateJson;
    }
    try {
      const serialized = iterativeStringify(this.state);
      this.cachedStateJson = serialized;
      return serialized;
    } catch {
      return "{}";
    }
  }
  /** WIT set-state(state-json) -> string. Empty = success, "ERROR:..." = failure. */
  setState(stateJson) {
    try {
      if (stateJson && stateJson.trim()) {
        this.state = JSON.parse(stateJson);
        this.cachedStateJson = null;
      }
      return "";
    } catch {
      return "ERROR:set_state failed";
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
} from "plexspaces:simple-actor/host@0.1.0";
function safeCall(fn, ...args) {
  if (typeof fn === "function") {
    return fn(...args);
  }
  return "";
}
var TupleSpace = class {
  constructor(host2) {
    this.host = host2;
  }
  /** Write a tuple. Elements must be JSON-serializable. Returns empty on success, "ERROR:..." on failure. */
  write(tuple) {
    const json = JSON.stringify(tuple);
    return this.host.tsWrite(json);
  }
  /** Take one matching tuple (destructive). Returns tuple as array or null if no match/error. */
  take(pattern) {
    const json = JSON.stringify(pattern);
    const raw = this.host.tsTake(json);
    if (raw === "" || raw.startsWith("ERROR"))
      return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  /** Read one matching tuple (non-destructive). Returns tuple as array or null if no match/error. */
  read(pattern) {
    const json = JSON.stringify(pattern);
    const raw = this.host.tsRead(json);
    if (raw === "" || raw.startsWith("ERROR"))
      return null;
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }
  /** Read all matching tuples (non-destructive). Returns array of tuples (each tuple is an array). */
  readAll(pattern) {
    const json = JSON.stringify(pattern);
    const raw = this.host.tsReadAll(json);
    if (raw === "" || raw.startsWith("ERROR"))
      return [];
    try {
      const out = JSON.parse(raw);
      return Array.isArray(out) ? out : [];
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
    return safeCall(hostSend, to, msgType, payloadJson);
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
   */
  sendAfter(delayMs, msgType, payload) {
    const payloadJson = payload !== void 0 ? JSON.stringify(payload) : "{}";
    return safeCall(hostSendAfter, BigInt(delayMs), msgType, payloadJson);
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
  // TupleSpace (low-level string API; prefer host.ts for list-in/list-out)
  // ========================================================================
  tsWrite(tupleJson) {
    return safeCall(hostTsWrite, tupleJson);
  }
  tsRead(patternJson) {
    return safeCall(hostTsRead, patternJson);
  }
  tsTake(patternJson) {
    return safeCall(hostTsTake, patternJson);
  }
  tsReadAll(patternJson) {
    return safeCall(hostTsReadAll, patternJson);
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
    const headersJson = JSON.stringify(headers ?? {});
    const bodyStr = body ?? "";
    const result = safeCall(hostHttpFetch, linkName, method, pathAndQuery, headersJson, bodyStr);
    if (typeof result === "string" && result.startsWith("ERROR:")) {
      throw new Error(result);
    }
    return JSON.parse(result);
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

// ../../../../sdks/typescript/dist/router.js
function normalizeActorRole(actorId) {
  if (!actorId) {
    return "";
  }
  const canonicalSep = actorId.indexOf("//");
  if (canonicalSep >= 0 && canonicalSep + 2 < actorId.length) {
    const rest = actorId.substring(canonicalSep + 2);
    const behaviorSep = rest.indexOf("::");
    if (behaviorSep >= 0) {
      return rest.substring(0, behaviorSep);
    }
    const nodeSep2 = rest.indexOf("@");
    return nodeSep2 >= 0 ? rest.substring(0, nodeSep2) : rest;
  }
  const childSep = actorId.indexOf(":");
  if (childSep >= 0) {
    return actorId.substring(0, childSep);
  }
  const nodeSep = actorId.indexOf("@");
  if (nodeSep >= 0) {
    return actorId.substring(0, nodeSep);
  }
  return actorId;
}
var ActorRouter = class {
  /**
   * Create an ActorRouter with prefix-to-factory mappings.
   *
   * @param routes - Map of actor_id prefix to factory function.
   *   Prefix matching: "rate-limiter" matches "rate-limiter-0", "rate-limiter-1", etc.
   *
   * Example:
   *   new ActorRouter({
   *     "parameter-server": () => new ParameterServerActor(),
   *     "data-worker": () => new DataWorkerActor(),
   *   })
   */
  constructor(routes) {
    this.active = null;
    this.factories = routes;
  }
  /** WIT init(config-json) -> string */
  init(configJson) {
    try {
      const config = configJson && configJson.trim() ? JSON.parse(configJson) : {};
      const actorId = config.actor_id || "";
      const name = normalizeActorRole(actorId);
      let bestPrefix = "";
      let bestFactory = null;
      for (const prefix of Object.keys(this.factories)) {
        if (name === prefix || name.startsWith(prefix)) {
          if (prefix.length > bestPrefix.length) {
            bestPrefix = prefix;
            bestFactory = this.factories[prefix];
          }
        }
      }
      if (!bestFactory) {
        return "ERROR: no actor registered for prefix: " + name;
      }
      this.active = bestFactory();
      return this.active.init(configJson);
    } catch {
      return "ERROR: router init failed";
    }
  }
  /** WIT handle(from-actor, msg-type, payload-json) -> string */
  handle(fromActor, msgType, payloadJson) {
    if (!this.active) {
      return '{"error":"no active actor (init not called)"}';
    }
    return this.active.handle(fromActor, msgType, payloadJson);
  }
  /** WIT get-state() -> string */
  getState() {
    if (!this.active) {
      return "{}";
    }
    return this.active.getState();
  }
  /** WIT set-state(state-json) -> string */
  setState(stateJson) {
    if (!this.active) {
      return "ERROR: no active actor";
    }
    return this.active.setState(stateJson);
  }
};

// weather_actor.ts
var LINK_NAME = "weather-api";
var CACHE_TTL_MS = 5 * 60 * 1e3;
function parseWeatherBody(bodyStr) {
  if (!bodyStr) return {};
  try {
    return JSON.parse(bodyStr);
  } catch {
    try {
      const decoded = decodeBase64Standard(bodyStr);
      return JSON.parse(decoded);
    } catch {
      return {};
    }
  }
}
function decodeBase64Standard(input) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const cleaned = input.replace(/\s+/g, "").replace(/=+$/, "");
  const bytes = [];
  let buffer = 0;
  let bits = 0;
  for (let index = 0; index < cleaned.length; index++) {
    const value = alphabet.indexOf(cleaned.charAt(index));
    if (value < 0) {
      throw new Error("invalid base64 payload");
    }
    buffer = buffer << 6 | value;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      bytes.push(buffer >> bits & 255);
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}
var WeatherActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { actor_id: "", cache_hits: 0, cache_misses: 0 };
  }
  init(configJSON) {
    try {
      const cfg = JSON.parse(configJSON);
      if (cfg.actor_id) this.state.actor_id = cfg.actor_id;
    } catch {
    }
    host.log("info", `WeatherActor initialized: ${this.state.actor_id}`);
    return "";
  }
  handle(_from, msgType, payloadJSON) {
    switch (msgType) {
      case "get_weather":
        return this.handleGetWeather(payloadJSON);
      case "cache_stats":
        return JSON.stringify({ hits: this.state.cache_hits, misses: this.state.cache_misses });
      case "clear_cache":
        this.state.cache_hits = 0;
        this.state.cache_misses = 0;
        return JSON.stringify({ cleared: true });
      default:
        return JSON.stringify({ error: `unknown message type: ${msgType}` });
    }
  }
  handleGetWeather(payloadJSON) {
    let city = "London";
    try {
      const req = JSON.parse(payloadJSON);
      if (req.city) city = req.city;
    } catch {
    }
    const cacheKey = `weather:${city}`;
    const cached = host.kvGet(cacheKey);
    if (cached.startsWith("ERROR:")) {
      host.log("warn", `Cache read failed for ${city}: ${cached}`);
    } else if (cached) {
      try {
        const data = JSON.parse(cached);
        const fetchedAt = data["fetched_at_ms"] ?? 0;
        if (host.nowMs() - fetchedAt < CACHE_TTL_MS) {
          this.state.cache_hits++;
          host.log("debug", `Cache HIT for ${city}`);
          return JSON.stringify({ ...data, city, source: "cache" });
        }
      } catch {
      }
    }
    this.state.cache_misses++;
    host.log("info", `Cache MISS for ${city} \u2014 calling ${LINK_NAME}`);
    try {
      const http = new ServiceHttpClient(LINK_NAME);
      const resp = http.get(
        `/v1/forecast?latitude=51.5&longitude=-0.12&current=temperature_2m,wind_speed_10m&city=${encodeURIComponent(city)}`
      );
      const bodyStr = resp.body ?? "";
      const weatherData = parseWeatherBody(bodyStr);
      const current = weatherData["current"] ?? {};
      const result = {
        temp_c: current["temperature_2m"] ?? 0,
        wind_kph: current["wind_speed_10m"] ?? 0,
        fetched_at_ms: host.nowMs()
      };
      const cacheWrite = host.kvPut(cacheKey, JSON.stringify(result));
      if (cacheWrite.startsWith("ERROR:")) {
        host.log("warn", `Cache write failed for ${city}: ${cacheWrite}`);
      }
      return JSON.stringify({ ...result, city, source: "api" });
    } catch (err) {
      host.log("error", `Weather API call failed: ${err}`);
      return JSON.stringify({ city, error: String(err), source: "api" });
    }
  }
};
var router = new ActorRouter({
  weather: () => new WeatherActor()
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
