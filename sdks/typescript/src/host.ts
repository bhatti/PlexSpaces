// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Host Functions (TypeScript SDK)
//
// Provides TypeScript wrappers for WIT host imports.
// Uses virtual imports from 'plexspaces:simple-actor/host@0.1.0'.

// Virtual imports provided by jco componentize at runtime
// @ts-ignore
import {
  send as hostSend,
  ask as hostAsk,
  log as hostLog,
  nowMs as hostNowMs,
  selfId as hostSelfId,
  parentId as hostParentId,
  spawn as hostSpawn,
  stop as hostStop,
  link as hostLink,
  unlink as hostUnlink,
  monitor as hostMonitor,
  demonitor as hostDemonitor,
  sendAfter as hostSendAfter,
  cancelTimer as hostCancelTimer,
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
  // @ts-expect-error Virtual import
} from 'plexspaces:simple-actor/host@0.1.0';

/**
 * Safe call helper — returns empty string if function is undefined.
 */
function safeCall<T>(fn: ((...args: any[]) => T) | undefined, ...args: any[]): T | string {
  if (typeof fn === 'function') {
    return fn(...args);
  }
  return '';
}

/**
 * Process groups sub-API
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
    const result = safeCall(hostPgMembers, group) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    try {
      return JSON.parse(result as string) as string[];
    } catch {
      return [];
    }
  }

  /** Broadcast message to all group members */
  broadcast(group: string, msgType: string, payload?: unknown): void {
    const payloadJson = payload !== undefined ? JSON.stringify(payload) : '{}';
    const result = safeCall(hostPgBroadcast, group, msgType, payloadJson) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
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
export class Host {
  readonly processGroups = new ProcessGroups();

  // ========================================================================
  // Messaging
  // ========================================================================

  /** Send message to another actor (fire-and-forget) */
  send(to: string, msgType: string, payload?: unknown): string {
    const payloadJson = payload !== undefined ? JSON.stringify(payload) : '';
    return safeCall(hostSend, to, msgType, payloadJson) as string;
  }

  /** Send request and wait for response (request-reply) */
  ask(to: string, msgType: string, payload?: unknown, timeoutMs: number = 5000): unknown {
    const payloadJson = payload !== undefined ? JSON.stringify(payload) : '';
    const result = safeCall(hostAsk, to, msgType, payloadJson, BigInt(timeoutMs)) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
    try {
      return JSON.parse(result as string);
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

  /** Get parent/supervisor actor ID (empty string if no parent) */
  parentId(): string {
    return safeCall(hostParentId) as string;
  }

  // ========================================================================
  // Actor Lifecycle
  // ========================================================================

  /** Spawn a new actor */
  spawn(moduleRef: string, actorId: string, initConfig?: unknown): void {
    const configJson = initConfig !== undefined ? JSON.stringify(initConfig) : '{}';
    const result = safeCall(hostSpawn, moduleRef, actorId, configJson) as string;
    if (typeof result === 'string' && result.startsWith('ERROR:')) {
      throw new Error(result);
    }
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

  /** Send message to self after delay (returns timer ID) */
  sendAfter(delayMs: number, msgType: string, payload?: unknown): string {
    const payloadJson = payload !== undefined ? JSON.stringify(payload) : '{}';
    return safeCall(hostSendAfter, BigInt(delayMs), msgType, payloadJson) as string;
  }

  /** Cancel a pending timer */
  cancelTimer(timerId: string): void {
    safeCall(hostCancelTimer, timerId);
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

  kvGet(key: string): string { return safeCall(hostKvGet, key) as string; }
  kvPut(key: string, value: string): string { return safeCall(hostKvPut, key, value) as string; }
  kvDelete(key: string): string { return safeCall(hostKvDelete, key) as string; }
  kvList(prefix: string): string { return safeCall(hostKvList, prefix) as string; }

  // ========================================================================
  // TupleSpace
  // ========================================================================

  tsWrite(tupleJson: string): string { return safeCall(hostTsWrite, tupleJson) as string; }
  tsRead(patternJson: string): string { return safeCall(hostTsRead, patternJson) as string; }
  tsTake(patternJson: string): string { return safeCall(hostTsTake, patternJson) as string; }
  tsReadAll(patternJson: string): string { return safeCall(hostTsReadAll, patternJson) as string; }

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
}

/** Global host instance */
export const host = new Host();
