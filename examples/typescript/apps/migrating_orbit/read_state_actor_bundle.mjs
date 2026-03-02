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
   * Dispatches by payload.op (or payload) to on<Op>(payload). Returns JSON string.
   * Uses iterative serializer to avoid WASM recursion.
   */
  handle(_fromActor, _msgType, payloadJson) {
    try {
      const payload = payloadJson && payloadJson.trim() ? JSON.parse(payloadJson) : {};
      const op = payload.op ?? payload;
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
  pgBroadcast as hostPgBroadcast
} from "plexspaces:simple-actor/host@0.1.0";
function safeCall(fn, ...args) {
  if (typeof fn === "function") {
    return fn(...args);
  }
  return "";
}
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
  /** Broadcast message to all group members */
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
  // TupleSpace
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
};
var host = new Host();

// read_state_actor.ts
var ReadStateTrackerActor = class extends PlexSpacesActor {
  getDefaultState() {
    return {
      user_id: "",
      channels: {},
      total_channels: 0,
      total_updates: 0,
      last_updated: 0,
      total_compute_ms: 0,
      total_coord_ms: 0
    };
  }
  onInit(config) {
    const userId = String(config.user_id ?? "");
    if (userId) {
      this.state.user_id = userId;
    }
    this.state.channels = {};
    this.state.total_channels = 0;
    this.state.total_updates = 0;
    this.state.last_updated = host.nowMs();
    this.state.total_compute_ms = 0;
    this.state.total_coord_ms = 0;
  }
  /**
   * Mark a message as read in a channel (Orbit: updateReadState)
   * Updates the last read message ID and timestamp for the user in the channel
   */
  onMark_read(payload) {
    const startMs = host.nowMs();
    try {
      const channelId = String(payload.channel_id ?? "");
      const messageId = String(payload.message_id ?? "");
      const timestamp = Number(payload.timestamp ?? host.nowMs());
      if (!channelId || !messageId) {
        return {
          status: "error",
          error: "channel_id and message_id required"
        };
      }
      let readState = this.state.channels[channelId];
      if (!readState) {
        readState = {
          channel_id: channelId,
          last_read_message_id: messageId,
          last_read_timestamp: timestamp
        };
        this.state.channels[channelId] = readState;
        this.state.total_channels = Object.keys(this.state.channels).length;
      } else {
        if (timestamp >= readState.last_read_timestamp) {
          readState.last_read_message_id = messageId;
          readState.last_read_timestamp = timestamp;
        }
      }
      this.state.total_updates++;
      this.state.last_updated = host.nowMs();
      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;
      return {
        status: "ok",
        user_id: this.state.user_id,
        channel_id: channelId,
        message_id: messageId,
        timestamp: readState.last_read_timestamp,
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates,
        compute_ms: computeMs
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e)
      };
    }
  }
  /**
   * Get read state for a specific channel (Orbit: getReadState)
   */
  onGet_read_state(payload) {
    const startMs = host.nowMs();
    try {
      const channelId = String(payload.channel_id ?? "");
      if (!channelId) {
        return {
          status: "error",
          error: "channel_id required"
        };
      }
      const readState = this.state.channels[channelId];
      if (!readState) {
        return {
          status: "ok",
          user_id: this.state.user_id,
          channel_id: channelId,
          read_state: null
        };
      }
      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;
      return {
        status: "ok",
        user_id: this.state.user_id,
        channel_id: channelId,
        read_state: {
          last_read_message_id: readState.last_read_message_id,
          last_read_timestamp: readState.last_read_timestamp
        },
        compute_ms: computeMs
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e)
      };
    }
  }
  /**
   * Get all read states for this user (Orbit: getAllReadStates)
   */
  onGet_all_read_states(_payload) {
    const startMs = host.nowMs();
    try {
      const channels = {};
      for (const [channelId, readState] of Object.entries(this.state.channels)) {
        channels[channelId] = {
          last_read_message_id: readState.last_read_message_id,
          last_read_timestamp: readState.last_read_timestamp
        };
      }
      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;
      return {
        status: "ok",
        user_id: this.state.user_id,
        channels,
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates,
        last_updated: this.state.last_updated,
        compute_ms: computeMs
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e)
      };
    }
  }
  /**
   * Batch update read states (for performance testing)
   * Updates multiple channels in a single call
   */
  onBatch_mark_read(payload) {
    const startMs = host.nowMs();
    try {
      const updates = payload.updates;
      if (!updates || !Array.isArray(updates)) {
        return {
          status: "error",
          error: "updates array required"
        };
      }
      const now = host.nowMs();
      let updated = 0;
      let created = 0;
      for (const update of updates) {
        const channelId = String(update.channel_id ?? "");
        const messageId = String(update.message_id ?? "");
        const timestamp = Number(update.timestamp ?? now);
        if (!channelId || !messageId) {
          continue;
        }
        let readState = this.state.channels[channelId];
        if (!readState) {
          readState = {
            channel_id: channelId,
            last_read_message_id: messageId,
            last_read_timestamp: timestamp
          };
          this.state.channels[channelId] = readState;
          created++;
        } else {
          if (timestamp >= readState.last_read_timestamp) {
            readState.last_read_message_id = messageId;
            readState.last_read_timestamp = timestamp;
            updated++;
          }
        }
      }
      this.state.total_channels = Object.keys(this.state.channels).length;
      this.state.total_updates += updates.length;
      this.state.last_updated = host.nowMs();
      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;
      return {
        status: "ok",
        user_id: this.state.user_id,
        total_updates: updates.length,
        channels_updated: updated,
        channels_created: created,
        total_channels: this.state.total_channels,
        compute_ms: computeMs,
        ops_per_sec: updates.length / (computeMs / 1e3)
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e)
      };
    }
  }
  /**
   * Get statistics (for metrics and testing)
   */
  onStats(_payload) {
    return {
      status: "ok",
      user_id: this.state.user_id,
      total_channels: this.state.total_channels,
      total_updates: this.state.total_updates,
      last_updated: this.state.last_updated,
      total_compute_ms: this.state.total_compute_ms,
      counters: {
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates
      },
      benchmarks: {
        total_compute_ms: this.state.total_compute_ms,
        ops_per_sec: this.state.total_updates > 0 && this.state.total_compute_ms > 0 ? this.state.total_updates / (this.state.total_compute_ms / 1e3) : 0
      }
    };
  }
};
var actorInstance = new ReadStateTrackerActor();
var actor = {
  init: (configJson) => actorInstance.init(configJson),
  handle: (from, msgType, payloadJson) => actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson) => actorInstance.setState(stateJson)
};
export {
  ReadStateTrackerActor,
  actor
};
