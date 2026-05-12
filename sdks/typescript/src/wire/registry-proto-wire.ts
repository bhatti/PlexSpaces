// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Manual protobuf wire encoding/decoding for plexspaces.object_registry.v1
// request/response messages used by WIT host imports from WASM.
// Matches sdks/go/plexspaces/registry_proto_wire.go.
//
// Field numbers from proto/plexspaces/v1/registry/object_registry.proto.

import { appendVarint, appendLengthDelimited, concatBytes, readVarint, skipField } from './proto-wire-common.js';

type WireU8 = Uint8Array<ArrayBuffer>;

const enc = new TextEncoder();
const dec = new TextDecoder();

function appendStringField(buf: WireU8, fieldNum: number, s: string): WireU8 {
  if (!s) return buf;
  const encoded = enc.encode(s);
  // Copy to an ArrayBuffer-backed Uint8Array to satisfy strict TS types.
  const bytes = new Uint8Array(encoded.length) as WireU8;
  bytes.set(encoded);
  const tag = fieldNum << 3 | 2;
  let b = appendVarint(buf, tag);
  b = appendVarint(b, bytes.length);
  return concatBytes(b, bytes);
}

function appendVarintField(buf: WireU8, fieldNum: number, v: number): WireU8 {
  const tag = fieldNum << 3; // wire type 0
  let b = appendVarint(buf, tag);
  return appendVarint(b, v);
}

// ObjectType enum values
export const ObjectType = {
  UNSPECIFIED: 0,
  ACTOR: 1,
  TUPLESPACE: 2,
  SERVICE: 3,
  VM: 4,
  APPLICATION: 5,
  WORKFLOW: 6,
  NODE: 7,
  PROCESS_GROUP: 8,
} as const;

export type ObjectTypeValue = typeof ObjectType[keyof typeof ObjectType];

function objectTypeToString(n: number): string {
  switch (n) {
    case 1: return 'actor';
    case 2: return 'tuplespace';
    case 3: return 'service';
    case 4: return 'vm';
    case 5: return 'application';
    case 6: return 'workflow';
    case 7: return 'node';
    case 8: return 'process_group';
    default: return '';
  }
}

function objectTypeFromString(s: string): number {
  switch (s) {
    case 'actor': return 1;
    case 'tuplespace': return 2;
    case 'service': return 3;
    case 'vm': return 4;
    case 'application': return 5;
    case 'workflow': return 6;
    case 'node': return 7;
    case 'process_group': return 8;
    default: return 0;
  }
}

export interface ObjectRegistration {
  objectId: string;
  objectType: string;
  grpcAddress?: string;
  objectCategory?: string;
  tenantId?: string;
  namespace?: string;
  capabilities?: string[];
  labels?: string[];
  alias?: string;
  healthStatus?: string;
}

// ============================================================================
// ObjectRegistration encoder
// Field numbers: object_id=1, object_type=3, tenant_id=5, namespace=6,
//                grpc_address=8, object_category=9, capabilities=10 (repeated),
//                labels=13 (repeated), alias=18
// ============================================================================

function encodeObjectRegistration(reg: ObjectRegistration): WireU8 {
  let b: WireU8 = new Uint8Array(0) as WireU8;
  b = appendStringField(b, 1, reg.objectId);
  const ot = objectTypeFromString(reg.objectType);
  if (ot !== 0) b = appendVarintField(b, 3, ot);
  if (reg.grpcAddress) b = appendStringField(b, 8, reg.grpcAddress);
  if (reg.objectCategory) b = appendStringField(b, 9, reg.objectCategory);
  if (reg.tenantId) b = appendStringField(b, 5, reg.tenantId);
  if (reg.namespace) b = appendStringField(b, 6, reg.namespace);
  for (const cap of (reg.capabilities ?? [])) b = appendStringField(b, 10, cap);
  for (const lbl of (reg.labels ?? [])) b = appendStringField(b, 13, lbl);
  if (reg.alias) b = appendStringField(b, 18, reg.alias);
  return b;
}

// ============================================================================
// RegisterRequest: registration=1 (message)
// ============================================================================

export function encodeRegisterRequest(reg: ObjectRegistration): WireU8 {
  const inner = encodeObjectRegistration(reg);
  return appendLengthDelimited(new Uint8Array(0) as WireU8, 1, inner);
}

// ============================================================================
// UnregisterRequest: object_id=1, object_type=2, tenant_id=3, namespace=4
// ============================================================================

export function encodeUnregisterRequest(objectId: string, objectType: number, tenantId?: string, namespace?: string): WireU8 {
  let b: WireU8 = new Uint8Array(0) as WireU8;
  b = appendStringField(b, 1, objectId);
  if (objectType !== 0) b = appendVarintField(b, 2, objectType);
  if (tenantId) b = appendStringField(b, 3, tenantId);
  if (namespace) b = appendStringField(b, 4, namespace);
  return b;
}

// ============================================================================
// LookupRequest: object_id=1, object_type=2, tenant_id=3, namespace=4, alias=5
// ============================================================================

export function encodeLookupRequest(objectId: string, objectType: number, tenantId?: string, namespace?: string, alias?: string): WireU8 {
  let b: WireU8 = new Uint8Array(0) as WireU8;
  if (objectId) b = appendStringField(b, 1, objectId);
  if (objectType !== 0) b = appendVarintField(b, 2, objectType);
  if (tenantId) b = appendStringField(b, 3, tenantId);
  if (namespace) b = appendStringField(b, 4, namespace);
  if (alias) b = appendStringField(b, 5, alias);
  return b;
}

// ============================================================================
// DiscoverRequest: object_type=1, object_category=2, tenant_id=4, namespace=5,
//                  capabilities=6, labels=7, page_size=10
// ============================================================================

export function encodeDiscoverRequest(opts: {
  objectType?: number;
  objectCategory?: string;
  tenantId?: string;
  namespace?: string;
  capabilities?: string[];
  labels?: string[];
  pageSize?: number;
}): WireU8 {
  let b: WireU8 = new Uint8Array(0) as WireU8;
  if (opts.objectType) b = appendVarintField(b, 1, opts.objectType);
  if (opts.objectCategory) b = appendStringField(b, 2, opts.objectCategory);
  if (opts.tenantId) b = appendStringField(b, 4, opts.tenantId);
  if (opts.namespace) b = appendStringField(b, 5, opts.namespace);
  for (const cap of (opts.capabilities ?? [])) b = appendStringField(b, 6, cap);
  for (const lbl of (opts.labels ?? [])) b = appendStringField(b, 7, lbl);
  if (opts.pageSize && opts.pageSize > 0) b = appendVarintField(b, 10, opts.pageSize);
  return b;
}

// ============================================================================
// HeartbeatRequest: object_id=1, object_type=2, tenant_id=3, namespace=4
// ============================================================================

export function encodeHeartbeatRequest(objectId: string, objectType: number, tenantId?: string, namespace?: string): WireU8 {
  let b: WireU8 = new Uint8Array(0) as WireU8;
  b = appendStringField(b, 1, objectId);
  if (objectType !== 0) b = appendVarintField(b, 2, objectType);
  if (tenantId) b = appendStringField(b, 3, tenantId);
  if (namespace) b = appendStringField(b, 4, namespace);
  return b;
}

// ============================================================================
// ObjectRegistration decoder
// ============================================================================

function decodeObjectRegistration(data: Uint8Array): ObjectRegistration {
  const reg: ObjectRegistration = { objectId: '', objectType: '' };
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
        case 1: reg.objectId = str; break;
        case 5: reg.tenantId = str; break;
        case 6: reg.namespace = str; break;
        case 8: reg.grpcAddress = str; break;
        case 9: reg.objectCategory = str; break;
        case 10: (reg.capabilities ??= []).push(str); break;
        case 13: (reg.labels ??= []).push(str); break;
        case 18: reg.alias = str; break;
      }
    } else if (wt === 0) {
      const { value: v, n: m } = readVarint(data, pos);
      pos += m;
      if (fn_ === 3) reg.objectType = objectTypeToString(Number(v));
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return reg;
}

// ============================================================================
// LookupResponse decoder: registration=1 (message), found=2 (bool)
// ============================================================================

export function decodeLookupResponse(data: Uint8Array): ObjectRegistration | null {
  let pos = 0;
  let regBytes: Uint8Array | null = null;
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
  if (!found || !regBytes) return null;
  return decodeObjectRegistration(regBytes);
}

// ============================================================================
// DiscoverResponse decoder: registrations=1 (repeated message)
// ============================================================================

export function decodeDiscoverResponse(data: Uint8Array): ObjectRegistration[] {
  const results: ObjectRegistration[] = [];
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
