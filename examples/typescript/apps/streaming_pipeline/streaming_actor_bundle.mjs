// ../../../../sdks/typescript/dist/actor.js
import { log as hostLog } from "plexspaces:simple-actor/host@0.1.0";
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
      if (msgType === "workflow_run") {
        const runFn = this.run;
        if (typeof runFn === "function") {
          const result = runFn.call(this, payload);
          this.cachedStateJson = null;
          return iterativeStringify(result ?? {});
        }
      }
      if (msgType.startsWith("workflow_signal:")) {
        const name = msgType.slice("workflow_signal:".length).trim();
        const signalFn = this.signal;
        if (typeof signalFn === "function") {
          signalFn.call(this, name, payload);
          this.cachedStateJson = null;
          return "{}";
        }
      }
      if (msgType.startsWith("workflow_query:")) {
        const name = msgType.slice("workflow_query:".length).trim();
        const queryFn = this.query;
        if (typeof queryFn === "function") {
          const result = queryFn.call(this, name, payload);
          return iterativeStringify(result ?? {});
        }
      }
      const opRaw = payload.message_type ?? payload.op ?? payload.msg_type;
      const op = typeof opRaw === "string" && opRaw ? opRaw : msgType;
      const opKey = typeof op === "string" ? this.capitalize(op) : "";
      const methodName = opKey ? `on${opKey}` : "";
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
  applicationMetricsAdd as hostApplicationMetricsAdd,
  applicationGetStatus as hostApplicationGetStatus
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
   * Spawn a new actor. Delegates to ActorFactory::spawn_actor() via the host.
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

// streaming_actor.ts
var STREAM_PREFIX = "streaming-pipeline-ts";
var LeaderActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      application_id: "",
      role: "leader",
      worker_count: 8,
      batch_count: 18,
      events_per_batch: 1200,
      drop_rate: 0.08,
      enrich_fields: 6,
      total_compute_ms: 0,
      total_coord_ms: 0
    };
  }
  onInit(config) {
    const args = config.args ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = String(args.role ?? "leader");
    this.state.worker_count = intValue(args.worker_count, this.state.worker_count);
    this.state.batch_count = intValue(args.batch_count, this.state.batch_count);
    this.state.events_per_batch = intValue(args.events_per_batch, this.state.events_per_batch);
    this.state.drop_rate = floatValue(args.drop_rate, this.state.drop_rate);
    this.state.enrich_fields = intValue(args.enrich_fields, this.state.enrich_fields);
    this.state.total_compute_ms = 0;
    this.state.total_coord_ms = 0;
  }
  onRun(payload) {
    const request = this.requestFromPayload(payload);
    if (request.worker_count <= 0 || request.batch_count <= 0 || request.events_per_batch <= 0) {
      return { error: "worker_count, batch_count, and events_per_batch must be positive" };
    }
    const groupId = `${STREAM_PREFIX}-${host.nowMs()}`;
    const group = host.createShardGroup({
      group_id: groupId,
      actor_type: "worker",
      shard_count: request.worker_count,
      partition_strategy: "hash",
      rebalance_policy: "manual",
      placement: { strategy: "from_registry" },
      initial_state: {}
    });
    const shardActorIds = stringArray(group.shard_actor_ids);
    if (shardActorIds.length === 0) {
      return { error: "failed to create worker shard group" };
    }
    const leaderNodeId = actorNodeId(this.state.actor_id);
    const participantNodeIds = [leaderNodeId];
    const seenNodes = /* @__PURE__ */ new Set([leaderNodeId]);
    for (const actorId of shardActorIds) {
      const nodeId = actorNodeId(actorId);
      if (nodeId && !seenNodes.has(nodeId)) {
        seenNodes.add(nodeId);
        participantNodeIds.push(nodeId);
      }
    }
    const startStatuses = {};
    const nodeAddresses = {};
    for (const nodeId of participantNodeIds) {
      const status = host.applicationGetStatus(this.state.application_id, nodeId);
      startStatuses[nodeId] = status;
      const address = stringValue(status.node_address);
      if (address) {
        nodeAddresses[nodeId] = address;
      }
    }
    const results = [];
    const remoteNodesWithWork = /* @__PURE__ */ new Set();
    let totalErrors = 0;
    let totalWorkerLatencyMs = 0;
    let totalWorkerResponses = 0;
    let maxWorkerLatencyMs = 0;
    let totalEventCount = 0;
    let totalFilteredEvents = 0;
    let totalEnrichedEvents = 0;
    let totalTransformedEvents = 0;
    let totalDroppedEvents = 0;
    let totalBytesProcessed = 0;
    let totalTupleOperations = 0;
    for (let batchIndex = 0; batchIndex < request.batch_count; batchIndex++) {
      const runId = `${groupId}-batch-${batchIndex}`;
      const coordStart = host.nowMs();
      const response = host.scatterGather({
        group_id: groupId,
        query: {
          op: "process_batch",
          run_id: runId,
          batch_index: batchIndex,
          events_per_batch: request.events_per_batch,
          drop_rate: request.drop_rate,
          enrich_fields: request.enrich_fields
        },
        aggregation: "concat",
        min_responses: request.worker_count,
        timeout_ms: 3e4
      });
      const coordMs = host.nowMs() - coordStart;
      const candidates = [];
      const shardResponses = anyArray(response.shard_responses);
      let iterationErrors = 0;
      let iterationResponses = 0;
      let iterationLatencyMs = 0;
      let iterationMaxLatencyMs = 0;
      let iterationEvents = 0;
      let iterationFiltered = 0;
      let iterationEnriched = 0;
      let iterationTransformed = 0;
      let iterationDropped = 0;
      let iterationBytes = 0;
      let iterationTupleOps = 0;
      for (const shard of shardResponses) {
        const shardMap = recordValue(shard);
        const payloadMap = normalizeWorkerPayload(shardMap.payload);
        if (stringValue(payloadMap.status) === "ok") {
          iterationResponses += 1;
          totalWorkerResponses += 1;
          const latencyMs = intValue(payloadMap.latency_ms, 0);
          iterationLatencyMs += latencyMs;
          totalWorkerLatencyMs += latencyMs;
          iterationMaxLatencyMs = Math.max(iterationMaxLatencyMs, latencyMs);
          maxWorkerLatencyMs = Math.max(maxWorkerLatencyMs, latencyMs);
          iterationEvents += intValue(payloadMap.event_count, 0);
          iterationFiltered += intValue(payloadMap.filtered_events, 0);
          iterationEnriched += intValue(payloadMap.enriched_events, 0);
          iterationTransformed += intValue(payloadMap.transformed_events, 0);
          iterationDropped += intValue(payloadMap.dropped_events, 0);
          iterationBytes += intValue(payloadMap.bytes_processed, 0);
          iterationTupleOps += intValue(payloadMap.tuple_operations, 0);
          const nodeId = stringValue(payloadMap.node_id) || actorNodeId(stringValue(payloadMap.actor_id));
          if (nodeId && nodeId !== leaderNodeId) {
            remoteNodesWithWork.add(nodeId);
          }
          for (const streamCount of anyArray(payloadMap.top_streams)) {
            const streamMap = recordValue(streamCount);
            candidates.push({
              stream: stringValue(streamMap.stream),
              count: intValue(streamMap.count, 0)
            });
          }
        } else {
          iterationErrors += 1;
          totalErrors += 1;
        }
      }
      const stageTuples = host.ts.readAll([STREAM_PREFIX, runId, "stage_summary", null, null, null]);
      const computeStart = host.nowMs();
      const mergedTopStreams = mergeTopStreams(candidates, 5);
      let computeMs = host.nowMs() - computeStart;
      if (iterationResponses > 0 && computeMs <= 0) {
        computeMs = 1;
      }
      this.state.total_coord_ms += coordMs;
      this.state.total_compute_ms += computeMs;
      totalEventCount += iterationEvents;
      totalFilteredEvents += iterationFiltered;
      totalEnrichedEvents += iterationEnriched;
      totalTransformedEvents += iterationTransformed;
      totalDroppedEvents += iterationDropped;
      totalBytesProcessed += iterationBytes;
      totalTupleOperations += iterationTupleOps + stageTuples.length;
      results.push({
        batch_index: batchIndex,
        responses: iterationResponses,
        errors: iterationErrors,
        event_count: iterationEvents,
        filtered_events: iterationFiltered,
        enriched_events: iterationEnriched,
        transformed_events: iterationTransformed,
        dropped_events: iterationDropped,
        bytes_processed: iterationBytes,
        tuple_operations: iterationTupleOps + stageTuples.length,
        coord_ms: coordMs,
        compute_ms: computeMs,
        avg_latency_ms: iterationResponses > 0 ? iterationLatencyMs / iterationResponses : 0,
        max_latency_ms: iterationMaxLatencyMs,
        top_streams: mergedTopStreams
      });
    }
    host.applicationMetricsAdd(this.state.application_id, {
      message_count: 1,
      counter_metrics: {
        leader_messages: request.batch_count + 1,
        leader_runs: 1,
        streaming_rounds: request.batch_count,
        leader_tuple_operations: request.batch_count
      },
      latency_totals_ms: {
        leader: this.state.total_compute_ms + this.state.total_coord_ms,
        "leader.compute": this.state.total_compute_ms,
        "leader.coordination": this.state.total_coord_ms
      },
      latency_max_ms: {
        leader: this.state.total_compute_ms + this.state.total_coord_ms,
        "leader.compute": this.state.total_compute_ms,
        "leader.coordination": this.state.total_coord_ms
      },
      latency_samples: {
        leader: 1,
        "leader.compute": 1,
        "leader.coordination": 1
      }
    });
    const nodeMetrics = {};
    const roleMetrics = {};
    for (const nodeId of participantNodeIds) {
      const status = host.applicationGetStatus(this.state.application_id, nodeId);
      const address = stringValue(status.node_address);
      if (address) {
        nodeAddresses[nodeId] = address;
      }
      applyStatusDelta(nodeMetrics, roleMetrics, startStatuses[nodeId], status);
    }
    for (const [nodeId, counts] of Object.entries(computeActorCounts(leaderNodeId, shardActorIds))) {
      const node = ensureNodeMetric(nodeMetrics, nodeId);
      node.actors += counts.actors;
      node.leader_actors += counts.leader_actors;
      node.worker_actors += counts.worker_actors;
    }
    ensureRoleMetric(roleMetrics, "leader").actors = 1;
    ensureRoleMetric(roleMetrics, "worker").actors = shardActorIds.length;
    let totalMessages = 0;
    let totalComputeMs = 0;
    let totalCoordinationMs = 0;
    let workerNodeCount = 0;
    const actorCounts = [];
    for (const [nodeId, metrics] of Object.entries(nodeMetrics)) {
      totalMessages += metrics.messages;
      totalComputeMs += metrics.compute_time_ms;
      totalCoordinationMs += metrics.coordination_time_ms;
      if (nodeId !== leaderNodeId && metrics.responses > 0) {
        workerNodeCount += 1;
      }
      actorCounts.push(metrics.actors);
    }
    const actorDistributionSkew = actorCounts.length > 0 ? Math.max(...actorCounts) - Math.min(...actorCounts) : 0;
    const nodes = {};
    for (const [nodeId, metrics] of Object.entries(nodeMetrics)) {
      nodes[nodeId] = {
        actors: metrics.actors,
        leader_actors: metrics.leader_actors,
        worker_actors: metrics.worker_actors,
        messages: metrics.messages,
        leader_messages: metrics.leader_messages,
        worker_messages: metrics.worker_messages,
        event_count: metrics.event_count,
        filtered_events: metrics.filtered_events,
        enriched_events: metrics.enriched_events,
        transformed_events: metrics.transformed_events,
        dropped_events: metrics.dropped_events,
        bytes_processed: metrics.bytes_processed,
        tuple_operations: metrics.tuple_operations,
        compute_time_ms: metrics.compute_time_ms,
        coordination_time_ms: metrics.coordination_time_ms,
        avg_latency_ms: averageLatency(metrics),
        max_latency_ms: metrics.max_latency_ms,
        errors: metrics.errors
      };
    }
    const roles = {};
    for (const [role, metrics] of Object.entries(roleMetrics)) {
      roles[role] = {
        actors: metrics.actors,
        messages: metrics.messages,
        event_count: metrics.event_count,
        filtered_events: metrics.filtered_events,
        enriched_events: metrics.enriched_events,
        transformed_events: metrics.transformed_events,
        dropped_events: metrics.dropped_events,
        bytes_processed: metrics.bytes_processed,
        tuple_operations: metrics.tuple_operations,
        compute_time_ms: metrics.compute_time_ms,
        coordination_time_ms: metrics.coordination_time_ms,
        avg_latency_ms: averageLatency(metrics),
        max_latency_ms: metrics.max_latency_ms,
        errors: metrics.errors
      };
    }
    return {
      status: "ok",
      worker_count: request.worker_count,
      batch_count: request.batch_count,
      events_per_batch: request.events_per_batch,
      drop_rate: request.drop_rate,
      enrich_fields: request.enrich_fields,
      stream_rounds: request.batch_count,
      leader_node_id: leaderNodeId,
      node_addresses: nodeAddresses,
      shard_actor_ids: shardActorIds,
      node_count: Object.keys(nodeMetrics).length,
      worker_node_count: workerNodeCount,
      actor_count: shardActorIds.length + 1,
      message_count: totalMessages,
      event_count: totalEventCount,
      filtered_event_count: totalFilteredEvents,
      enriched_event_count: totalEnrichedEvents,
      transformed_event_count: totalTransformedEvents,
      dropped_event_count: totalDroppedEvents,
      bytes_processed: totalBytesProcessed,
      tuple_operation_count: totalTupleOperations,
      compute_time_ms: totalComputeMs,
      coordination_time_ms: totalCoordinationMs,
      total_time_ms: totalComputeMs + totalCoordinationMs,
      granularity_ratio: ratio(totalComputeMs, totalCoordinationMs),
      avg_worker_latency_ms: totalWorkerResponses > 0 ? totalWorkerLatencyMs / totalWorkerResponses : 0,
      max_worker_latency_ms: maxWorkerLatencyMs,
      error_count: totalErrors,
      remote_nodes_with_work: Array.from(remoteNodesWithWork).sort(),
      actor_distribution_skew: actorDistributionSkew,
      results,
      nodes,
      roles
    };
  }
  requestFromPayload(payload) {
    return {
      worker_count: intValue(payload.worker_count, this.state.worker_count),
      batch_count: intValue(payload.batch_count, this.state.batch_count),
      events_per_batch: intValue(payload.events_per_batch, this.state.events_per_batch),
      drop_rate: floatValue(payload.drop_rate, this.state.drop_rate),
      enrich_fields: intValue(payload.enrich_fields, this.state.enrich_fields)
    };
  }
};
var WorkerActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      actor_id: "",
      application_id: "",
      role: "worker"
    };
  }
  onInit(config) {
    const args = config.args ?? {};
    this.state.actor_id = String(config.actor_id ?? "");
    this.state.application_id = actorApplicationId(this.state.actor_id);
    this.state.role = String(args.role ?? "worker");
  }
  onProcess_batch(payload) {
    const runId = stringValue(payload.run_id);
    const batchIndex = intValue(payload.batch_index, 0);
    const eventsPerBatch = intValue(payload.events_per_batch, 1200);
    const dropRate = floatValue(payload.drop_rate, 0.08);
    const enrichFields = intValue(payload.enrich_fields, 6);
    const filteredEvents = Math.max(0, Math.floor(eventsPerBatch * (1 - dropRate)));
    const droppedEvents = Math.max(0, eventsPerBatch - filteredEvents);
    const enrichedEvents = filteredEvents;
    const transformedEvents = filteredEvents;
    const bytesProcessed = transformedEvents * (180 + enrichFields * 24);
    const computeMs = Math.max(1, Math.floor(eventsPerBatch / 150));
    const latencyMs = computeMs;
    const topStreams = topStreamCounts(workerSeed(this.state.actor_id), batchIndex, transformedEvents);
    host.ts.write([
      STREAM_PREFIX,
      runId,
      "stage_summary",
      batchIndex,
      actorRoleId(this.state.actor_id),
      {
        filtered_events: filteredEvents,
        enriched_events: enrichedEvents,
        transformed_events: transformedEvents,
        dropped_events: droppedEvents
      }
    ]);
    host.applicationMetricsAdd(this.state.application_id, {
      message_count: 1,
      counter_metrics: {
        worker_messages: 1,
        event_count: eventsPerBatch,
        filtered_events: filteredEvents,
        enriched_events: enrichedEvents,
        transformed_events: transformedEvents,
        dropped_events: droppedEvents,
        bytes_processed: bytesProcessed,
        worker_tuple_operations: 1
      },
      latency_totals_ms: {
        worker: latencyMs,
        "worker.compute": computeMs,
        "worker.coordination": Math.max(0, latencyMs - computeMs)
      },
      latency_max_ms: {
        worker: latencyMs,
        "worker.compute": computeMs,
        "worker.coordination": Math.max(0, latencyMs - computeMs)
      },
      latency_samples: {
        worker: 1,
        "worker.compute": 1,
        "worker.coordination": 1
      }
    });
    return {
      status: "ok",
      actor_id: this.state.actor_id,
      node_id: actorNodeId(this.state.actor_id),
      latency_ms: latencyMs,
      event_count: eventsPerBatch,
      filtered_events: filteredEvents,
      enriched_events: enrichedEvents,
      transformed_events: transformedEvents,
      dropped_events: droppedEvents,
      bytes_processed: bytesProcessed,
      tuple_operations: 1,
      top_streams: topStreams
    };
  }
};
function actorNodeId(actorId) {
  const parts = actorId.split("@");
  return parts.length > 1 ? parts[parts.length - 1] : "local";
}
function actorRoleId(actorId) {
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
  return nodeSep >= 0 ? actorId.substring(0, nodeSep) : actorId;
}
function actorApplicationId(actorId) {
  if (!actorId) {
    return "";
  }
  if (actorId.includes("//") && actorId.includes("::")) {
    const rest = actorId.split("//", 2)[1];
    const qualified = rest.split("@", 1)[0];
    const parts = qualified.split("::", 2);
    return parts.length === 2 ? parts[1] : "";
  }
  if (actorId.includes(":") && actorId.includes("@")) {
    return actorId.split(":", 2)[1].split("@", 1)[0];
  }
  return "";
}
function mergeTopStreams(values, topK) {
  const counts = {};
  for (const value of values) {
    counts[value.stream] = (counts[value.stream] ?? 0) + value.count;
  }
  return Object.entries(counts).map(([stream, count]) => ({ stream, count })).sort((a, b) => b.count - a.count).slice(0, topK);
}
function topStreamCounts(seed, batchIndex, events) {
  const streams = ["auth", "edge", "api", "dns", "audit"];
  const results = [];
  let remaining = events;
  for (let index = 0; index < streams.length; index++) {
    const divisor = streams.length - index;
    const count = index === streams.length - 1 ? remaining : Math.max(0, Math.floor((seed + batchIndex * 17 + index * 11) % 100 / 100 * remaining / divisor));
    results.push({ stream: streams[index], count });
    remaining = Math.max(0, remaining - count);
  }
  return results.sort((a, b) => intValue(b.count, 0) - intValue(a.count, 0));
}
function normalizeWorkerPayload(payload) {
  let current = recordValue(payload);
  while (current && !("status" in current)) {
    let progressed = false;
    for (const key of ["payload", "result", "response", "data"]) {
      if (recordValue(current[key])) {
        current = recordValue(current[key]);
        progressed = true;
        break;
      }
    }
    if (!progressed) {
      break;
    }
  }
  return current;
}
function ensureNodeMetric(metrics, nodeId) {
  if (!metrics[nodeId]) {
    metrics[nodeId] = {
      actors: 0,
      leader_actors: 0,
      worker_actors: 0,
      messages: 0,
      leader_messages: 0,
      worker_messages: 0,
      event_count: 0,
      filtered_events: 0,
      enriched_events: 0,
      transformed_events: 0,
      dropped_events: 0,
      bytes_processed: 0,
      tuple_operations: 0,
      compute_time_ms: 0,
      coordination_time_ms: 0,
      total_latency_ms: 0,
      max_latency_ms: 0,
      responses: 0,
      errors: 0
    };
  }
  return metrics[nodeId];
}
function ensureRoleMetric(metrics, role) {
  if (!metrics[role]) {
    metrics[role] = {
      actors: 0,
      messages: 0,
      event_count: 0,
      filtered_events: 0,
      enriched_events: 0,
      transformed_events: 0,
      dropped_events: 0,
      bytes_processed: 0,
      tuple_operations: 0,
      compute_time_ms: 0,
      coordination_time_ms: 0,
      total_latency_ms: 0,
      max_latency_ms: 0,
      responses: 0,
      errors: 0
    };
  }
  return metrics[role];
}
function applyStatusDelta(nodeMetrics, roleMetrics, startStatus, endStatus) {
  const counterDelta = saturatingMapDelta(statusMetricsMap(endStatus, "counter_metrics"), statusMetricsMap(startStatus, "counter_metrics"));
  const latencyTotalsDelta = saturatingMapDelta(statusMetricsMap(endStatus, "latency_totals_ms"), statusMetricsMap(startStatus, "latency_totals_ms"));
  const latencyMaxEnd = statusMetricsMap(endStatus, "latency_max_ms");
  const latencyMaxStart = statusMetricsMap(startStatus, "latency_max_ms");
  const latencySamplesDelta = saturatingMapDelta(statusMetricsMap(endStatus, "latency_samples"), statusMetricsMap(startStatus, "latency_samples"));
  const messageDelta = Math.max(intValue(statusMetricsValue(endStatus, "message_count"), 0) - intValue(statusMetricsValue(startStatus, "message_count"), 0), 0);
  const errorDelta = Math.max(intValue(statusMetricsValue(endStatus, "error_count"), 0) - intValue(statusMetricsValue(startStatus, "error_count"), 0), 0);
  const nodeId = stringValue(endStatus.node_id);
  const node = ensureNodeMetric(nodeMetrics, nodeId);
  node.messages += messageDelta;
  node.leader_messages += counterDelta.leader_messages ?? 0;
  node.worker_messages += counterDelta.worker_messages ?? 0;
  node.event_count += counterDelta.event_count ?? 0;
  node.filtered_events += counterDelta.filtered_events ?? 0;
  node.enriched_events += counterDelta.enriched_events ?? 0;
  node.transformed_events += counterDelta.transformed_events ?? 0;
  node.dropped_events += counterDelta.dropped_events ?? 0;
  node.bytes_processed += counterDelta.bytes_processed ?? 0;
  node.tuple_operations += (counterDelta.worker_tuple_operations ?? 0) + (counterDelta.leader_tuple_operations ?? 0);
  node.compute_time_ms += (latencyTotalsDelta["worker.compute"] ?? 0) + (latencyTotalsDelta["leader.compute"] ?? 0);
  node.coordination_time_ms += (latencyTotalsDelta["worker.coordination"] ?? 0) + (latencyTotalsDelta["leader.coordination"] ?? 0);
  node.total_latency_ms += (latencyTotalsDelta.worker ?? 0) + (latencyTotalsDelta.leader ?? 0);
  node.max_latency_ms = Math.max(
    node.max_latency_ms,
    latencyMaxEnd.worker ?? 0,
    latencyMaxStart.worker ?? 0,
    latencyMaxEnd.leader ?? 0,
    latencyMaxStart.leader ?? 0
  );
  node.responses += latencySamplesDelta.worker ?? 0;
  node.errors += errorDelta;
  const leader = ensureRoleMetric(roleMetrics, "leader");
  leader.messages += counterDelta.leader_messages ?? 0;
  leader.tuple_operations += counterDelta.leader_tuple_operations ?? 0;
  leader.compute_time_ms += latencyTotalsDelta["leader.compute"] ?? 0;
  leader.coordination_time_ms += latencyTotalsDelta["leader.coordination"] ?? 0;
  leader.total_latency_ms += latencyTotalsDelta.leader ?? 0;
  leader.max_latency_ms = Math.max(leader.max_latency_ms, latencyMaxEnd.leader ?? 0, latencyMaxStart.leader ?? 0);
  leader.responses += latencySamplesDelta.leader ?? 0;
  const worker = ensureRoleMetric(roleMetrics, "worker");
  worker.messages += counterDelta.worker_messages ?? 0;
  worker.event_count += counterDelta.event_count ?? 0;
  worker.filtered_events += counterDelta.filtered_events ?? 0;
  worker.enriched_events += counterDelta.enriched_events ?? 0;
  worker.transformed_events += counterDelta.transformed_events ?? 0;
  worker.dropped_events += counterDelta.dropped_events ?? 0;
  worker.bytes_processed += counterDelta.bytes_processed ?? 0;
  worker.tuple_operations += counterDelta.worker_tuple_operations ?? 0;
  worker.compute_time_ms += latencyTotalsDelta["worker.compute"] ?? 0;
  worker.coordination_time_ms += latencyTotalsDelta["worker.coordination"] ?? 0;
  worker.total_latency_ms += latencyTotalsDelta.worker ?? 0;
  worker.max_latency_ms = Math.max(worker.max_latency_ms, latencyMaxEnd.worker ?? 0, latencyMaxStart.worker ?? 0);
  worker.responses += latencySamplesDelta.worker ?? 0;
  worker.errors += errorDelta;
}
function statusMetricsValue(status, field) {
  const application = recordValue(status.application);
  const metrics = recordValue(application.metrics);
  return metrics[field];
}
function statusMetricsMap(status, field) {
  const value = recordValue(statusMetricsValue(status, field));
  const result = {};
  for (const [key, raw] of Object.entries(value)) {
    result[key] = intValue(raw, 0);
  }
  return result;
}
function saturatingMapDelta(end, start) {
  const result = {};
  for (const [key, endValue] of Object.entries(end)) {
    const startValue = start[key] ?? 0;
    result[key] = endValue > startValue ? endValue - startValue : 0;
  }
  for (const key of Object.keys(start)) {
    if (!(key in result)) {
      result[key] = 0;
    }
  }
  return result;
}
function computeActorCounts(leaderNodeId, shardActorIds) {
  const nodes = {
    [leaderNodeId]: {
      actors: 1,
      leader_actors: 1,
      worker_actors: 0
    }
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
function averageLatency(metric) {
  return metric.responses > 0 ? metric.total_latency_ms / metric.responses : 0;
}
function ratio(computeMs, coordinationMs) {
  return coordinationMs > 0 ? computeMs / coordinationMs : 0;
}
function workerSeed(actorId) {
  let value = 0;
  for (let index = 0; index < actorId.length; index++) {
    value = (value * 31 + actorId.charCodeAt(index)) % 1e3;
  }
  return value;
}
function intValue(value, fallback) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.trunc(value);
  }
  if (typeof value === "string") {
    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
}
function floatValue(value, fallback) {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }
  return fallback;
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
  return anyArray(value).map((item) => String(item)).filter((item) => item.length > 0);
}
var router = new ActorRouter({
  leader: () => new LeaderActor(),
  worker: () => new WorkerActor()
});
var actor = {
  init: (configJson) => router.init(configJson),
  handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson) => router.setState(stateJson)
};
export {
  actor
};
