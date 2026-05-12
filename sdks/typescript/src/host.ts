// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Host Functions (TypeScript SDK)
//
// Provides TypeScript wrappers for WIT host imports.
// Uses virtual imports from 'plexspaces:actor/host@0.1.0'.

// Protobuf wire for WIT `payload` fields (matches Go WASM hostWire* / tuplespace_proto_wire).
import {
  decodeReadResponseAllTuples,
  decodeReadResponseFirstTuple,
  encodeReadRequest,
  encodeWriteRequest,
} from './wire/tuplespace-proto-wire.js';
import { decodeHttpFetchResponseWire, encodeHttpFetchRequestWire } from './wire/http-fetch-proto-wire.js';
import {
  decodeApplicationMetrics,
  decodeCreateShardGroupResponse,
  decodeGetApplicationStatusResponse,
  decodeScatterGatherResponse,
  encodeApplicationMetrics,
  encodeCreateShardGroupRequest,
  encodeScatterGatherRequest,
} from './wire/shard-group-proto-wire.js';
import { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from './wit-payload.js';
import {
  encodeRegisterRequest,
  encodeUnregisterRequest,
  encodeLookupRequest,
  encodeDiscoverRequest,
  encodeHeartbeatRequest,
  decodeLookupResponse,
  decodeDiscoverResponse,
  ObjectType as RegistryObjectType,
} from './wire/registry-proto-wire.js';
import { firstGroupMember, firstGroupMemberOrThrow } from './process_groups.js';

// Virtual imports provided by jco componentize at runtime.
// These map 1:1 to the WIT host interface functions.
// @ts-ignore
import {
  send as hostSend,
  ask as hostAsk,
  log as hostLog,
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
  httpFetch as hostHttpFetch,
  // @ts-expect-error Virtual import
} from 'plexspaces:actor/host@0.1.0';

// @ts-ignore
import {
  register as hostRegistryRegister,
  unregister as hostRegistryUnregister,
  lookup as hostRegistryLookup,
  lookupByAlias as hostRegistryLookupByAlias,
  discover as hostRegistryDiscover,
  heartbeat as hostRegistryHeartbeat,
  // @ts-expect-error Virtual import
} from 'plexspaces:actor/registry@0.1.0';

/**
 * Safe call helper — returns empty string if function is undefined.
 */
function safeCall<T>(fn: ((...args: any[]) => T) | undefined, ...args: any[]): T | string {
  if (typeof fn === 'function') {
    return fn(...args);
  }
  return '';
}

/** Host list&lt;u8&gt; / payload: accept Uint8Array; some bindings surface Latin-1 string bytes. */
function hostPayloadToBytes(result: unknown): Uint8Array {
  if (result instanceof Uint8Array) return result;
  // Cross-realm ArrayBufferView check: jco componentize-js returns list<u8> as a Uint8Array
  // from a different JS realm inside the WASM component, so instanceof fails. Use isView() instead.
  if (ArrayBuffer.isView(result)) {
    const v = result as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  if (result instanceof ArrayBuffer) {
    return new Uint8Array(result);
  }
  if (typeof result === 'string') {
    const out = new Uint8Array(result.length);
    for (let i = 0; i < result.length; i++) out[i] = result.charCodeAt(i) & 0xff;
    return out;
  }
  return new Uint8Array(0);
}

function hostErrorPrefixBytes(raw: Uint8Array): boolean {
  const prefix = 'ERROR:';
  if (raw.length < prefix.length) return false;
  for (let i = 0; i < prefix.length; i++) {
    if (raw[i] !== prefix.charCodeAt(i)) return false;
  }
  return true;
}

/**
 * Tuple space helper: list-in, list-out API.
 * Use null in patterns for wildcards. Consistent with Python host.ts and Go host.TS().
 */
export class TupleSpace {
  constructor(private host: Host) {}

  /**
   * Write a tuple. Values are encoded as plexspaces.tuplespace.v1 WriteRequest protobuf wire
   * (same as Go `TupleSpace.Write` / Rust simple_component_host).
   */
  write(tuple: unknown[]): string {
    try {
      const wire = encodeWriteRequest(tuple);
      return this.host.tsWritePayload(wire);
    } catch (e) {
      return `ERROR: ${e instanceof Error ? e.message : String(e)}`;
    }
  }

  /** Take one matching tuple (destructive). */
  take(pattern: unknown[]): unknown[] | null {
    try {
      const wire = encodeReadRequest(pattern, true, 1);
      const raw = this.host.tsTakePayload(wire);
      if (raw.length === 0 || hostErrorPrefixBytes(raw)) return null;
      return decodeReadResponseFirstTuple(raw);
    } catch {
      return null;
    }
  }

  /** Read one matching tuple (non-destructive). */
  read(pattern: unknown[]): unknown[] | null {
    try {
      const wire = encodeReadRequest(pattern, false, 1);
      const raw = this.host.tsReadPayload(wire);
      if (raw.length === 0 || hostErrorPrefixBytes(raw)) return null;
      return decodeReadResponseFirstTuple(raw);
    } catch {
      return null;
    }
  }

  /** Read all matching tuples (non-destructive). */
  readAll(pattern: unknown[]): unknown[][] {
    try {
      const wire = encodeReadRequest(pattern, false, 1024);
      const raw = this.host.tsReadAllPayload(wire);
      if (raw.length === 0 || hostErrorPrefixBytes(raw)) return [];
      return decodeReadResponseAllTuples(raw);
    } catch {
      return [];
    }
  }
}

/**
 * Process groups sub-API.
 * broadcast(group, msgType, payload): msgType is used by the host for routing; payload can be data-only.
 */
export class ProcessGroups {
  /** Join a named process group */
  join(group: string): void {
    const result = safeCall(hostPgJoin, group) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /** Leave a named process group */
  leave(group: string): void {
    const result = safeCall(hostPgLeave, group) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /** Get members of a process group */
  members(group: string): string[] {
    const raw = safeCall(hostPgMembers, group);
    const result = decodeWitPayloadUtf8(raw as (string | Uint8Array | ArrayBuffer | ArrayBufferView));
    if (result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    try {
      return JSON.parse(result) as string[];
    } catch {
      return [];
    }
  }

  /** Broadcast to all group members. msgType is used for routing so payload can be data-only. */
  broadcast(group: string, msgType: string, payload?: unknown): void {
    const payloadBytes = encodeWitPayloadUtf8(payload !== undefined ? JSON.stringify(payload) : '{}');
    const result = safeCall(hostPgBroadcast, group, msgType, payloadBytes) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /** Return the first member of a process group, or null if empty. */
  first(group: string): string | null {
    return firstGroupMember(this.members(group));
  }

  /** Return the first member of a process group, throwing if empty. */
  firstOrThrow(group: string): string {
    return firstGroupMemberOrThrow(group, this.members(group));
  }
}

/**
 * PlexSpaces host function interface.
 *
 * Provides typed access to all WIT host capabilities.
 *
 * Usage:
 *   import { host } from '@plexspaces/sdk';
 *
 *   host.send('other-actor', 'ping', { data: 'hello' });
 *   const response = host.ask('other-actor', 'get_balance', {}, 5000);
 *   const myId = host.selfId();
 */
/** Object registration data from the registry. */
export interface ObjectRegistration {
  objectId: string;
  objectType: string;
  grpcAddress?: string;
  objectCategory?: string;
  tenantId?: string;
  namespace?: string;
  capabilities?: string[];
  labels?: string[];
  healthStatus?: string;
  createdAt?: number;
  updatedAt?: number;
  lastHeartbeat?: number;
  alias?: string;
}

/**
 * Object Registry host functions for registration and discovery.
 *
 * All calls encode request/response as proto wire bytes matching
 * plexspaces.object_registry.v1 messages.
 *
 * @example
 * ```typescript
 * // Register this actor
 * host.registry.register({ objectId: myId, objectType: 'actor', objectCategory: 'GenServer' });
 *
 * // Look up by alias (Orleans grain directory pattern)
 * const reg = host.registry.lookupByAlias('Counter:my-counter:default:tenant1');
 *
 * // Discover all actors of a given category
 * const actors = host.registry.discover({ objectType: RegistryObjectType.ACTOR, objectCategory: 'GenServer' });
 * ```
 */
export class Registry {
  /**
   * Register an object in the registry.
   */
  register(reg: ObjectRegistration): void {
    const reqBytes = encodeRegisterRequest(reg);
    const result = safeCall(hostRegistryRegister, reqBytes) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /**
   * Unregister an object from the registry.
   */
  unregister(objectId: string, objectType: number, tenantId?: string, namespace?: string): void {
    const reqBytes = encodeUnregisterRequest(objectId, objectType, tenantId, namespace);
    const result = safeCall(hostRegistryUnregister, reqBytes) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /**
   * Look up an object by ID. Returns null if not found, throws on storage errors.
   */
  lookup(objectId: string, objectType: number = 0, tenantId?: string, namespace?: string): ObjectRegistration | null {
    const reqBytes = encodeLookupRequest(objectId, objectType, tenantId, namespace);
    const raw = safeCall(hostRegistryLookup, reqBytes);
    if (typeof raw === 'string' && raw.startsWith('ERROR:')) {
      throw new Error(raw);
    }
    if (!raw) return null;
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0) return null;
    return decodeLookupResponse(bytes);
  }

  /**
   * Look up an object by alias (Orleans grain directory pattern).
   * Alias format: "{actor_type}:{name}:{namespace}:{tenant_id}"
   * Returns null if not found, throws on storage errors.
   */
  lookupByAlias(alias: string): ObjectRegistration | null {
    const raw = safeCall(hostRegistryLookupByAlias, alias);
    if (typeof raw === 'string' && raw.startsWith('ERROR:')) {
      throw new Error(raw);
    }
    if (!raw) return null;
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0) return null;
    return decodeLookupResponse(bytes);
  }

  /**
   * Discover objects with optional filtering.
   */
  discover(options: {
    objectType?: number;
    objectCategory?: string;
    tenantId?: string;
    namespace?: string;
    capabilities?: string[];
    labels?: string[];
    pageSize?: number;
  } = {}): ObjectRegistration[] {
    const reqBytes = encodeDiscoverRequest(options);
    const raw = safeCall(hostRegistryDiscover, reqBytes);
    if (!raw) return [];
    if (typeof raw === 'string' && raw.startsWith('ERROR:')) {
      throw new Error(raw);
    }
    const bytes = raw instanceof Uint8Array ? raw : new Uint8Array(0);
    if (bytes.length === 0) return [];
    return decodeDiscoverResponse(bytes);
  }

  /**
   * Update the heartbeat for a registered object.
   */
  heartbeat(objectId: string, objectType: number = 0, tenantId?: string, namespace?: string): void {
    const reqBytes = encodeHeartbeatRequest(objectId, objectType, tenantId, namespace);
    const result = safeCall(hostRegistryHeartbeat, reqBytes) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }
}

export { RegistryObjectType };

export class Host {
  readonly processGroups = new ProcessGroups();
  /** Tuple space: list-in, list-out. Use null in patterns for wildcards. */
  readonly ts = new TupleSpace(this);
  /** Object registry: register, discover, and look up actors and services. */
  readonly registry = new Registry();

  // ========================================================================
  // Messaging
  // ========================================================================

  /** Send message to another actor (fire-and-forget) */
  send(to: string, msgType: string, payload?: unknown): string {
    const payloadBytes = encodeWitPayloadUtf8(payload !== undefined ? JSON.stringify(payload) : '');
    const raw = safeCall(hostSend, to, msgType, payloadBytes);
    // WIT `result<_, actor-error>` ok is unit; jco may yield `undefined` on success.
    if (typeof raw !== 'string') {
      return '';
    }
    return raw;
  }

  /** Send request and wait for response (request-reply) */
  ask(to: string, msgType: string, payload?: unknown, timeoutMs: number = 5000): unknown {
    const payloadBytes = encodeWitPayloadUtf8(payload !== undefined ? JSON.stringify(payload) : '');
    const raw = safeCall(hostAsk, to, msgType, payloadBytes, BigInt(timeoutMs));
    // jco returns result<list<u8>, actor-error>: OK value is Uint8Array, error is a string.
    const result = decodeWitPayloadUtf8(raw as (string | Uint8Array | ArrayBuffer | ArrayBufferView));
    if (result.startsWith('ERROR:')) {
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
  selfId(): string {
    return safeCall(hostSelfId) as string;
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
  spawn(moduleRef: string, actorId: string = '', initConfig?: unknown): string {
    const configBytes = encodeWitPayloadUtf8(initConfig !== undefined ? JSON.stringify(initConfig) : '{}');
    const result = safeCall(hostSpawn, moduleRef, actorId, configBytes) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return result as string;
  }

  /** Stop an actor gracefully */
  stop(actorId: string): void {
    const result = safeCall(hostStop, actorId) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  // ========================================================================
  // Actor Linking & Monitoring
  // ========================================================================

  /** Bidirectional link */
  link(actorId: string): void {
    const result = safeCall(hostLink, actorId) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /** Remove bidirectional link */
  unlink(actorId: string): void {
    const result = safeCall(hostUnlink, actorId) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /** Monitor an actor (returns monitor reference) */
  monitor(actorId: string): string {
    const result = safeCall(hostMonitor, actorId) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return result as string;
  }

  /** Cancel a monitor */
  demonitor(monitorRef: string): void {
    const result = safeCall(hostDemonitor, monitorRef) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
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
  sendAfter(delayMs: number, msgType: string, payload?: unknown): string {
    const text = payload !== undefined ? JSON.stringify(payload) : '{}';
    const payloadBytes = new TextEncoder().encode(text);
    const raw = safeCall(hostSendAfter, BigInt(delayMs), msgType, payloadBytes);
    if (typeof raw === 'string') {
      return raw;
    }
    if (raw && typeof raw === 'object') {
      const o = raw as { tag?: string | number; val?: unknown };
      if (o.tag === 'ok' || o.tag === 0) {
        return typeof o.val === 'string' ? o.val : '';
      }
      if (o.tag === 'err' || o.tag === 1) {
        return `ERROR:${String(o.val ?? 'send-after failed')}`;
      }
    }
    return '';
  }

  // ========================================================================
  // Logging & Time
  // ========================================================================

  /** Log a message */
  log(level: string, message: string): void {
    safeCall(hostLog, level, message);
  }

  debug(message: string): void { this.log('debug', message); }
  info(message: string): void { this.log('info', message); }
  warn(message: string): void { this.log('warn', message); }
  error(message: string): void { this.log('error', message); }

  /** Get current timestamp in milliseconds */
  nowMs(): number {
    const result = safeCall(hostNowMs);
    return typeof result === 'bigint' ? Number(result) : (typeof result === 'number' ? result : 0);
  }

  // ========================================================================
  // Key-Value Store
  // ========================================================================

  kvGet(key: string): string {
    if (typeof hostKvGet !== 'function') return '';
    try { return decodeWitPayloadUtf8(hostKvGet(key)); } catch (e) { return `ERROR:${e}`; }
  }
  kvPut(key: string, value: string): string {
    if (typeof hostKvPut !== 'function') return '';
    try { hostKvPut(key, encodeWitPayloadUtf8(value)); return ''; } catch (e) { return `ERROR:${e}`; }
  }
  kvDelete(key: string): string {
    if (typeof hostKvDelete !== 'function') return '';
    try { hostKvDelete(key); return ''; } catch (e) { return `ERROR:${e}`; }
  }
  kvList(prefix: string): string {
    if (typeof hostKvList !== 'function') return '[]';
    try { return JSON.stringify(hostKvList(prefix)); } catch (e) { return `ERROR:${e}`; }
  }

  /** Retrieve a JSON value by key. Returns parsed object or null if not found. */
  kvGetJson<T = unknown>(key: string): T | null {
    const raw = this.kvGet(key);
    if (!raw || raw.startsWith('ERROR:')) return null;
    try { return JSON.parse(raw) as T; } catch { return null; }
  }

  /** Serialize value to JSON and store under key. Throws on write failure. */
  kvPutJson(key: string, value: unknown): void {
    const serialized = JSON.stringify(value);
    const result = this.kvPut(key, serialized);
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(`kvPutJson(${key}): ${result}`);
    }
  }

  /** Increment a single named application metric counter by 1. Errors are swallowed. */
  incrCounter(applicationId: string, name: string): void {
    this.incrCounters(applicationId, { [name]: 1 });
  }

  /** Increment one or more named application metric counters. Errors are swallowed. */
  incrCounters(applicationId: string, counters: Record<string, number>): void {
    try {
      this.applicationMetricsAdd(applicationId, {
        message_count: Object.keys(counters).length,
        counter_metrics: counters,
      });
    } catch (e) {
      this.warn(`incrCounters: metrics update failed: ${e}`);
    }
  }

  // ========================================================================
  // TupleSpace (protobuf WriteRequest / ReadRequest / ReadResponse wire bytes)
  // ========================================================================

  /** @internal TupleSpace — plexspaces.tuplespace.v1 wire bytes. */
  tsWritePayload(data: Uint8Array): string {
    const r = safeCall(hostTsWrite, data) as unknown;
    return typeof r === 'string' ? r : '';
  }

  /** @internal */
  tsReadPayload(data: Uint8Array): Uint8Array {
    return hostPayloadToBytes(safeCall(hostTsRead, data));
  }

  /** @internal */
  tsTakePayload(data: Uint8Array): Uint8Array {
    return hostPayloadToBytes(safeCall(hostTsTake, data));
  }

  /** @internal */
  tsReadAllPayload(data: Uint8Array): Uint8Array {
    return hostPayloadToBytes(safeCall(hostTsReadAll, data));
  }

  // ========================================================================
  // Distributed Locks
  // ========================================================================

  lockAcquire(tenantId: string, namespace: string, holderId: string, lockName: string, leaseDurationSecs: number = 30, timeoutMs: number = 0): string {
    return safeCall(hostLockAcquire, tenantId, namespace, holderId, lockName, leaseDurationSecs, BigInt(timeoutMs)) as string;
  }
  lockRelease(lockId: string, tenantId: string, namespace: string, holderId: string, lockVersion: string): string {
    return safeCall(hostLockRelease, lockId, tenantId, namespace, holderId, lockVersion) as string;
  }
  lockRenew(lockId: string, tenantId: string, namespace: string, holderId: string, lockVersion: string, leaseDurationSecs: number = 30): string {
    return safeCall(hostLockRenew, lockId, tenantId, namespace, holderId, lockVersion, leaseDurationSecs) as string;
  }

  // ========================================================================
  // Blob Storage
  // ========================================================================

  blobUpload(blobId: string, data: string, contentType: string = 'application/octet-stream'): string {
    return safeCall(hostBlobUpload, blobId, data, contentType) as string;
  }
  blobDownload(blobId: string): string { return safeCall(hostBlobDownload, blobId) as string; }
  blobDelete(blobId: string): string { return safeCall(hostBlobDelete, blobId) as string; }
  blobList(prefix: string): string { return safeCall(hostBlobList, prefix) as string; }

  // ========================================================================
  // Elastic pool (checkout/checkin)
  // ========================================================================

  /**
   * Checkout an actor from a named pool. Returns handle { actor_id, pool_name, checkout_id } or null on failure.
   */
  poolCheckout(poolName: string, timeoutMs: number = 5000): { actor_id: string; pool_name: string; checkout_id: string } | null {
    const result = safeCall(hostPoolCheckout, poolName, BigInt(timeoutMs)) as string;
    if (typeof result !== 'string' || result === '' || result.startsWith('ERROR:')) return null;
    try {
      return JSON.parse(result) as { actor_id: string; pool_name: string; checkout_id: string };
    } catch {
      return null;
    }
  }

  /**
   * Checkin an actor to the pool. Pass actor_id and checkout_id from the handle returned by poolCheckout.
   */
  poolCheckin(poolName: string, actorId: string, checkoutId: string, healthy: boolean): void {
    const result = safeCall(hostPoolCheckin, poolName, actorId, checkoutId, healthy) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
  }

  /**
   * Get pool metrics (total_actors, available_actors, busy_actors, current_load, etc.). Returns null if not available.
   */
  poolGetMetrics(poolName: string): Record<string, unknown> | null {
    const result = safeCall(hostPoolGetMetrics, poolName) as string;
    if (typeof result !== 'string' || result === '' || result.startsWith('ERROR:')) return null;
    try {
      return JSON.parse(result) as Record<string, unknown>;
    } catch {
      return null;
    }
  }

  createShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const reqBytes = encodeCreateShardGroupRequest(request);
    const result = safeCall(hostCreateShardGroup, reqBytes) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) return { shard_actor_ids: [] };
    const decoded = decodeCreateShardGroupResponse(bytes);
    // Flatten: expose group fields at top level for actor convenience
    const group = decoded.group as Record<string, unknown> ?? {};
    return { ...group, ...decoded };
  }

  bulkUpdateShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostBulkUpdateShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  mapShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostMapShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  scatterGather(request: Record<string, unknown>): Record<string, unknown> {
    const reqBytes = encodeScatterGatherRequest(request);
    const result = safeCall(hostScatterGather, reqBytes) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) return { shard_responses: [] };
    return decodeScatterGatherResponse(bytes);
  }

  broadcastShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostBroadcastShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  reduceShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostReduceShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  allReduceShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostAllReduceShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  barrierShardGroup(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostBarrierShardGroup, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  spawnActors(request: Record<string, unknown>): Record<string, unknown> {
    const result = safeCall(hostSpawnActors, JSON.stringify(request)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    return JSON.parse(result as string) as Record<string, unknown>;
  }

  applicationMetricsAdd(applicationId: string, metrics: Record<string, unknown>): Record<string, unknown> {
    const metricsBytes = encodeApplicationMetrics(metrics);
    const result = safeCall(hostApplicationMetricsAdd, applicationId, metricsBytes) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) return {};
    try { return JSON.parse(new TextDecoder().decode(bytes)) as Record<string, unknown>; } catch { return {}; }
  }

  applicationGetMetrics(applicationId: string, nodeId: string): Record<string, unknown> {
    const result = safeCall(hostApplicationGetMetrics, applicationId, nodeId) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) return {};
    return decodeApplicationMetrics(bytes);
  }

  applicationGetStatus(applicationId: string, nodeId: string): Record<string, unknown> {
    const result = safeCall(hostApplicationGetStatus, applicationId, nodeId) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) return { node_id: nodeId, node_address: '', application: null };
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
  httpFetch(
    linkName: string,
    method: string,
    pathAndQuery: string,
    headers?: Record<string, string>,
    body?: string,
  ): { status: number; headers: Record<string, string>; body: string } {
    const bodyBytes = body !== undefined && body.length > 0 ? new TextEncoder().encode(body) : new Uint8Array(0);
    const reqWire = encodeHttpFetchRequestWire(headers ?? {}, bodyBytes);
    const result = safeCall(hostHttpFetch, linkName, method, pathAndQuery, reqWire) as unknown;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    const bytes = hostPayloadToBytes(result);
    if (bytes.length === 0) {
      return { status: 0, headers: {}, body: '' };
    }
    if (hostErrorPrefixBytes(bytes)) {
      throw new Error(new TextDecoder('utf-8', { fatal: false }).decode(bytes));
    }
    const asText = new TextDecoder('utf-8', { fatal: false }).decode(bytes);
    try {
      return JSON.parse(asText) as { status: number; headers: Record<string, string>; body: string };
    } catch {
      return decodeHttpFetchResponseWire(bytes);
    }
  }
}

/**
 * Two-cursor monotonic append-only log backed by KV.
 *
 * Embed in actor state (serialize with `JSON.stringify`). Each consumer
 * tracks its own read cursor so they advance independently.
 *
 * @example
 * ```typescript
 * const log = new EventLog();
 * const seq = log.append(host, 'audit:', { action: 'login' });
 * const [events, newCursor] = log.poll(host, 'audit:', 'consumer-1', 20);
 * ```
 */
export class EventLog {
  watermark: number = 0;

  constructor(watermark = 0) {
    this.watermark = watermark;
  }

  /** Append an entry to the log. Returns the assigned sequence number. */
  append(h: Host, prefix: string, entry: unknown): number {
    this.watermark++;
    const key = `${prefix}seq:${this.watermark}`;
    try {
      h.kvPutJson(key, entry);
    } catch (e) {
      this.watermark--;
      throw new Error(`EventLog.append: ${e}`);
    }
    return this.watermark;
  }

  /**
   * Return up to `limit` events for `consumerId` that arrived after its last cursor.
   * Returns `[events, newCursor]`. The new cursor is persisted in KV.
   */
  poll(h: Host, prefix: string, consumerId: string, limit = 100): [unknown[], number] {
    const cursorKey = `${prefix}cursor:${consumerId}`;
    const rawCursor = h.kvGet(cursorKey);
    const cursor = rawCursor ? parseInt(rawCursor, 10) || 0 : 0;

    const events: unknown[] = [];
    let newCursor = cursor;
    for (let seq = cursor + 1; seq <= this.watermark && events.length < limit; seq++) {
      const entry = h.kvGetJson(`${prefix}seq:${seq}`);
      if (entry !== null) {
        events.push(entry);
        newCursor = seq;
      }
    }

    if (newCursor !== cursor) {
      h.kvPut(cursorKey, String(newCursor));
    }
    return [events, newCursor];
  }
}

/**
 * Ergonomic outbound HTTP client backed by a named service link.
 *
 * The link must be pre-configured in RuntimeConfig.service_links.
 * The host handles retries, circuit breaking, and auth injection.
 *
 * @example
 * ```typescript
 * const http = new ServiceHttpClient("payments-api");
 * const balance = http.get("/v1/balance?account=123");
 * const result = http.post("/v1/transfer", { amount: 100 });
 * ```
 */
export class ServiceHttpClient {
  constructor(private readonly linkName: string) {}

  /** GET request. Returns response object with status, headers, body. */
  get(
    pathAndQuery: string,
    headers?: Record<string, string>,
  ): { status: number; headers: Record<string, string>; body: string } {
    return host.httpFetch(this.linkName, 'GET', pathAndQuery, headers);
  }

  /** POST JSON request. body is serialized to JSON. */
  post(
    pathAndQuery: string,
    body?: unknown,
    headers?: Record<string, string>,
  ): { status: number; headers: Record<string, string>; body: string } {
    const bodyStr = body !== undefined ? JSON.stringify(body) : '';
    return host.httpFetch(this.linkName, 'POST', pathAndQuery, headers, bodyStr);
  }

  /** PUT JSON request. */
  put(
    pathAndQuery: string,
    body?: unknown,
    headers?: Record<string, string>,
  ): { status: number; headers: Record<string, string>; body: string } {
    const bodyStr = body !== undefined ? JSON.stringify(body) : '';
    return host.httpFetch(this.linkName, 'PUT', pathAndQuery, headers, bodyStr);
  }

  /** DELETE request. */
  delete(
    pathAndQuery: string,
    headers?: Record<string, string>,
  ): { status: number; headers: Record<string, string>; body: string } {
    return host.httpFetch(this.linkName, 'DELETE', pathAndQuery, headers);
  }
}

/** Global host instance */
export const host = new Host();

/** Return the first member of a process group, or null if empty. */
export function pgFirst(group: string): string | null {
  return host.processGroups.first(group);
}

/** Return the first member of a process group, throwing if empty. */
export function pgFirstOrThrow(group: string): string {
  return host.processGroups.firstOrThrow(group);
}
