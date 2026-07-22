// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Protobuf wire encoding/decoding for plexspaces.transport.ws.v1.WsFrame.
// Covers the subset used by WsThinClient (tell, ask, register, heartbeat, ping).
// Byte-compatible with the prost-encoded WsFrame in crates/node/src/http_routes/ws_routes.rs.
//
// Field numbers from:
//   proto/plexspaces/v1/transport/websocket.proto  — WsFrame, WsNodeRegisterAck, WsError
//   proto/plexspaces/v1/actors/actor_runtime.proto — SendMessageRequest, AskReplyRequest, AskReplyResponse
//   proto/plexspaces/v1/node/node.proto            — NodeRegistration, PingRequest, PingResponse,
//                                                    SendHeartbeatRequest

import {
  appendLengthDelimited,
  appendVarint,
  concatBytes,
  readLengthDelimited,
  readVarint,
  skipField,
} from './proto-wire-common.js';
import { ActorID } from '../actor_id.js';

// ─── helpers ────────────────────────────────────────────────────────────────

const textEnc = new TextEncoder();
const textDec = new TextDecoder('utf-8', { fatal: false });

type WireU8 = Uint8Array<ArrayBuffer>;

function str(buf: Uint8Array, fieldNum: number, s: string): WireU8 {
  if (!s) return buf as WireU8;
  return appendLengthDelimited(buf, fieldNum, new Uint8Array(textEnc.encode(s))) as WireU8;
}

function bytes(buf: Uint8Array, fieldNum: number, data: Uint8Array): WireU8 {
  if (data.length === 0) return buf as WireU8;
  return appendLengthDelimited(buf, fieldNum, data) as WireU8;
}

function uint32(buf: Uint8Array, fieldNum: number, v: number): WireU8 {
  if (v === 0) return buf as WireU8;
  let b = appendVarint(buf, (fieldNum << 3) | 0) as WireU8; // wire type 0 = varint
  return appendVarint(b, v >>> 0) as WireU8;
}

function uint64(buf: Uint8Array, fieldNum: number, v: number): WireU8 {
  if (v === 0) return buf as WireU8;
  let b = appendVarint(buf, (fieldNum << 3) | 0) as WireU8;
  return appendVarint(b, v) as WireU8;
}

// proto float (wire type 5, fixed32 little-endian)
function float32(buf: Uint8Array, fieldNum: number, v: number): WireU8 {
  if (v === 0) return buf as WireU8;
  const tag = appendVarint(buf, (fieldNum << 3) | 5) as WireU8;
  const out = new Uint8Array(tag.length + 4) as WireU8;
  out.set(tag, 0);
  new DataView(out.buffer, tag.length, 4).setFloat32(0, v, true);
  return out;
}

/** Encode a map<string,string> field (proto: repeated embedded message{key=1,value=2}). */
function mapStringString(buf: Uint8Array, fieldNum: number, m: Record<string, string>): WireU8 {
  let out = buf as WireU8;
  for (const [k, v] of Object.entries(m)) {
    let entry: WireU8 = new Uint8Array(0) as WireU8;
    entry = str(entry, 1, k);
    entry = str(entry, 2, v);
    out = appendLengthDelimited(out, fieldNum, entry) as WireU8;
  }
  return out;
}

function readStr(data: Uint8Array, pos: number): { value: string; nextPos: number } {
  const { slice, nextPos } = readLengthDelimited(data, pos);
  return { value: textDec.decode(slice), nextPos };
}

function readBool(data: Uint8Array, pos: number): { value: boolean; nextPos: number } {
  const { value, n } = readVarint(data, pos);
  return { value: value !== 0n, nextPos: pos + n };
}

function readFloat32(data: Uint8Array, pos: number): { value: number; nextPos: number } {
  if (pos + 4 > data.length) throw new Error('float32 underflow');
  const value = new DataView(data.buffer, data.byteOffset + pos, 4).getFloat32(0, true);
  return { value, nextPos: pos + 4 };
}

function readUint64(data: Uint8Array, pos: number): { value: number; nextPos: number } {
  const { value, n } = readVarint(data, pos);
  return { value: Number(value), nextPos: pos + n };
}

// ─── WsFrame field numbers (from websocket.proto) ───────────────────────────
// WsFrame: request_id=1, tell=10, tell_response=11, ask=12, ask_response=13
//   node_register=20, node_register_ack=21, node_ping=22, node_ping_response=23
//   heartbeat=24, heartbeat_ack=25, metrics_request=26, metrics_response=27, error=30

// SendMessageRequest field numbers (actor_runtime.proto):
//   request_id=1, namespace=2, actor_type=3, payload=5, sender_id=10, message_type=11, actor_name=21

// AskReplyRequest field numbers (actor_runtime.proto):
//   request_id=1, namespace=2, actor_type=3, payload=5, sender_id=10, message_type=11, timeout=15, actor_name=21

// AskReplyResponse field numbers (actor_runtime.proto):
//   request_id=1, success=2, payload=3, actor_id=5, error_message=6

// NodeRegistration (node.proto):
//   node_id=1, node_address=2, capabilities=3(map), node_role=10

// WsNodeRegisterAck (websocket.proto):
//   success=1, assigned_node_id=2, error_message=3

// PingRequest (node.proto):
//   request_id=1, source_node_id=2, sequence_number=3

// PingResponse (node.proto):
//   request_id=1, node_id=2, sequence_number=3, incarnation=4, cluster_name=6,
//   node_address=7, resources=9 (NodeResourceHints)
//
// NodeResourceHints (node.proto):
//   cpu_percent=1 (float), memory_available_mb=2 (uint64), available_cores=3 (uint32)

// SendHeartbeatRequest (node.proto):
//   request_id=1, node_id=2

// WsError (websocket.proto):
//   request_id=1, code=2, message=3

// SendMessageResponse (actor_runtime.proto):
//   request_id=1, success=2, error_message=5

// ─── Encode (client → server) ───────────────────────────────────────────────

/**
 * Encode a tell (fire-and-forget) frame.
 * actor_type carries the full canonical actor ID when it contains '@'.
 */
export function encodeWsFrameTell(
  requestId: string,
  actorId: string,
  msgType: string,
  payloadBytes: Uint8Array,
): WireU8 {
  const parsed = ActorID.parse(actorId);
  // Build SendMessageRequest (field 10 inside WsFrame)
  let inner: WireU8 = new Uint8Array(0) as WireU8;
  inner = str(inner, 2, parsed.namespace);  // namespace (required by server)
  inner = str(inner, 3, actorId);           // actor_type carries full canonical ID
  if (msgType) inner = str(inner, 11, msgType);
  inner = bytes(inner, 5, payloadBytes);

  // Build WsFrame
  let frame: WireU8 = new Uint8Array(0) as WireU8;
  frame = str(frame, 1, requestId);
  frame = appendLengthDelimited(frame, 10, inner) as WireU8;
  return frame;
}

/**
 * Encode an ask (request-reply) frame.
 * timeout field 15 in AskReplyRequest is a google.protobuf.Duration: message{seconds=1, nanos=2}.
 */
export function encodeWsFrameAsk(
  requestId: string,
  actorId: string,
  msgType: string,
  payloadBytes: Uint8Array,
  timeoutMs: number,
): WireU8 {
  const parsed = ActorID.parse(actorId);
  // Build AskReplyRequest (field 12 inside WsFrame)
  let inner: WireU8 = new Uint8Array(0) as WireU8;
  inner = str(inner, 1, requestId);         // request_id inside AskReplyRequest
  inner = str(inner, 2, parsed.namespace);  // namespace (required by server)
  inner = str(inner, 3, actorId);           // actor_type carries full canonical ID
  inner = str(inner, 4, 'POST');            // http_method — must be POST so server passes payload bytes through unchanged
  if (msgType) inner = str(inner, 11, msgType);
  inner = bytes(inner, 5, payloadBytes);

  // Encode timeout as Duration message {seconds: field 1, nanos: field 2}
  if (timeoutMs > 0) {
    const secs = Math.floor(timeoutMs / 1000);
    const nanos = (timeoutMs % 1000) * 1_000_000;
    let dur: WireU8 = new Uint8Array(0) as WireU8;
    if (secs > 0) dur = uint64(dur, 1, secs);
    if (nanos > 0) dur = uint32(dur, 2, nanos);
    inner = appendLengthDelimited(inner, 15, dur) as WireU8;
  }

  // Build WsFrame
  let frame: WireU8 = new Uint8Array(0) as WireU8;
  frame = str(frame, 1, requestId);
  frame = appendLengthDelimited(frame, 12, inner) as WireU8;
  return frame;
}

/**
 * Encode a node registration handshake frame (must be first frame after WS upgrade).
 * resourceHints advertises browser capabilities as NodeResourceHints (field 11).
 */
export function encodeWsFrameNodeRegister(
  requestId: string,
  nodeId: string,
  nodeAddress: string,
  capabilities: Record<string, string>,
  resourceHints?: { cpuPercent?: number; memoryAvailableMb?: number; availableCores?: number },
): WireU8 {
  // Build NodeRegistration (field 20 inside WsFrame)
  // node_role = NODE_ROLE_THIN = 2
  let inner: WireU8 = new Uint8Array(0) as WireU8;
  inner = str(inner, 1, nodeId);
  inner = str(inner, 2, nodeAddress);
  inner = mapStringString(inner, 3, capabilities);
  inner = uint32(inner, 10, 2); // node_role = NODE_ROLE_THIN

  // Encode NodeResourceHints sub-message as field 11 if present
  if (resourceHints) {
    let hints: WireU8 = new Uint8Array(0) as WireU8;
    if (resourceHints.cpuPercent) hints = float32(hints, 1, resourceHints.cpuPercent);
    if (resourceHints.memoryAvailableMb) hints = uint64(hints, 2, resourceHints.memoryAvailableMb);
    if (resourceHints.availableCores) hints = uint32(hints, 3, resourceHints.availableCores);
    if (hints.length > 0) inner = appendLengthDelimited(inner, 11, hints) as WireU8;
  }

  // Build WsFrame
  let frame: WireU8 = new Uint8Array(0) as WireU8;
  frame = str(frame, 1, requestId);
  frame = appendLengthDelimited(frame, 20, inner) as WireU8;
  return frame;
}

/**
 * Encode a heartbeat frame to keep the WS session alive.
 */
export function encodeWsFrameHeartbeat(requestId: string, nodeId: string): WireU8 {
  // Build SendHeartbeatRequest (field 24 inside WsFrame)
  let inner: WireU8 = new Uint8Array(0) as WireU8;
  inner = str(inner, 1, requestId);
  inner = str(inner, 2, nodeId);

  let frame: WireU8 = new Uint8Array(0) as WireU8;
  frame = str(frame, 1, requestId);
  frame = appendLengthDelimited(frame, 24, inner) as WireU8;
  return frame;
}

/**
 * Encode a node ping frame (SWIM-compatible, carries resource hints in the response).
 */
export function encodeWsFrameNodePing(
  requestId: string,
  sourceNodeId: string,
  sequenceNumber: number,
): WireU8 {
  // Build PingRequest (field 22 inside WsFrame)
  let inner: WireU8 = new Uint8Array(0) as WireU8;
  inner = str(inner, 1, requestId);
  inner = str(inner, 2, sourceNodeId);
  if (sequenceNumber) inner = uint64(inner, 3, sequenceNumber);

  let frame: WireU8 = new Uint8Array(0) as WireU8;
  frame = str(frame, 1, requestId);
  frame = appendLengthDelimited(frame, 22, inner) as WireU8;
  return frame;
}

// ─── Decoded frame discriminated union ──────────────────────────────────────

export type WsFrameDecoded =
  | { type: 'node_register_ack'; requestId: string; success: boolean; assignedNodeId: string; errorMessage: string }
  | { type: 'ask_response'; requestId: string; success: boolean; payloadJson: unknown; errorMessage: string }
  | { type: 'tell_response'; requestId: string; success: boolean; errorMessage: string }
  | { type: 'heartbeat_ack'; requestId: string }
  | { type: 'node_ping_response'; requestId: string; nodeId: string; cpuPercent: number; memoryAvailableMb: number; availableCores: number }
  | { type: 'incoming_tell'; requestId: string; actorId: string; msgType: string; payloadJson: unknown }
  | { type: 'error'; requestId: string; code: number; message: string }
  | { type: 'unknown' };

// ─── Decode helpers ──────────────────────────────────────────────────────────

function parseMessage(data: Uint8Array): Map<number, unknown[]> {
  const fields = new Map<number, unknown[]>();
  const push = (fn: number, v: unknown) => {
    const arr = fields.get(fn) ?? [];
    arr.push(v);
    fields.set(fn, arr);
  };
  let pos = 0;
  while (pos < data.length) {
    const { value: tag, n: tn } = readVarint(data, pos);
    pos += tn;
    const fn = Number(tag >> 3n);
    const wt = Number(tag & 7n);
    if (wt === 0) {
      const { value, n } = readVarint(data, pos);
      pos += n;
      push(fn, value);
    } else if (wt === 2) {
      const { slice, nextPos } = readLengthDelimited(data, pos);
      pos = nextPos;
      push(fn, slice);
    } else if (wt === 5) {
      // fixed32 (float)
      if (pos + 4 > data.length) throw new Error('fixed32 underflow');
      const v = new DataView(data.buffer, data.byteOffset + pos, 4).getFloat32(0, true);
      pos += 4;
      push(fn, v);
    } else {
      pos = skipField(data, pos, wt);
    }
  }
  return fields;
}

function getStr(fields: Map<number, unknown[]>, fn: number): string {
  const arr = fields.get(fn);
  if (!arr || arr.length === 0) return '';
  const v = arr[0];
  if (v instanceof Uint8Array) return textDec.decode(v);
  return '';
}

function getU64(fields: Map<number, unknown[]>, fn: number): number {
  const arr = fields.get(fn);
  if (!arr || arr.length === 0) return 0;
  const v = arr[0];
  if (typeof v === 'bigint') return Number(v);
  return 0;
}

function getBool(fields: Map<number, unknown[]>, fn: number): boolean {
  const arr = fields.get(fn);
  if (!arr || arr.length === 0) return false;
  const v = arr[0];
  if (typeof v === 'bigint') return v !== 0n;
  return false;
}

function getFloat(fields: Map<number, unknown[]>, fn: number): number {
  const arr = fields.get(fn);
  if (!arr || arr.length === 0) return 0;
  const v = arr[0];
  if (typeof v === 'number') return v;
  return 0;
}

function getBytes(fields: Map<number, unknown[]>, fn: number): Uint8Array {
  const arr = fields.get(fn);
  if (!arr || arr.length === 0) return new Uint8Array(0);
  const v = arr[0];
  if (v instanceof Uint8Array) return v;
  return new Uint8Array(0);
}

function parseJson(raw: Uint8Array): unknown {
  if (raw.length === 0) return null;
  try {
    return JSON.parse(textDec.decode(raw));
  } catch {
    return textDec.decode(raw);
  }
}

function parseAskResponse(data: Uint8Array): { requestId: string; success: boolean; payloadJson: unknown; errorMessage: string } {
  const f = parseMessage(data);
  return {
    requestId: getStr(f, 1),
    success: getBool(f, 2),
    payloadJson: parseJson(getBytes(f, 3)),
    errorMessage: getStr(f, 6),
  };
}

function parseTellResponse(data: Uint8Array): { requestId: string; success: boolean; errorMessage: string } {
  const f = parseMessage(data);
  return { requestId: getStr(f, 1), success: getBool(f, 2), errorMessage: getStr(f, 5) };
}

function parseRegisterAck(data: Uint8Array): { success: boolean; assignedNodeId: string; errorMessage: string } {
  const f = parseMessage(data);
  return { success: getBool(f, 1), assignedNodeId: getStr(f, 2), errorMessage: getStr(f, 3) };
}

function parseNodeResourceHints(data: Uint8Array): { cpuPercent: number; memoryAvailableMb: number; availableCores: number } {
  const f = parseMessage(data);
  return {
    cpuPercent: getFloat(f, 1),
    memoryAvailableMb: getU64(f, 2),
    availableCores: getU64(f, 3),
  };
}

function parsePingResponse(data: Uint8Array): { requestId: string; nodeId: string; cpuPercent: number; memoryAvailableMb: number; availableCores: number } {
  const f = parseMessage(data);
  // resources is field 9 (length-delimited NodeResourceHints sub-message)
  const resourcesBytes = getBytes(f, 9);
  const hints = resourcesBytes.length > 0
    ? parseNodeResourceHints(resourcesBytes)
    : { cpuPercent: 0, memoryAvailableMb: 0, availableCores: 0 };
  return {
    requestId: getStr(f, 1),
    nodeId: getStr(f, 2),
    cpuPercent: hints.cpuPercent,
    memoryAvailableMb: hints.memoryAvailableMb,
    availableCores: hints.availableCores,
  };
}

function parseIncomingTell(requestId: string, data: Uint8Array): { actorId: string; msgType: string; payloadJson: unknown } {
  // SendMessageRequest: actor_type=3, message_type=11, payload=5
  const f = parseMessage(data);
  return {
    actorId: getStr(f, 3),
    msgType: getStr(f, 11),
    payloadJson: parseJson(getBytes(f, 5)),
  };
}

function parseError(data: Uint8Array): { requestId: string; code: number; message: string } {
  const f = parseMessage(data);
  return { requestId: getStr(f, 1), code: getU64(f, 2), message: getStr(f, 3) };
}

// ─── Top-level decode ────────────────────────────────────────────────────────

/**
 * Decode a binary WebSocket frame into a typed discriminated union.
 * Unknown fields are skipped (forward-compatible).
 */
export function decodeWsFrame(bytes: Uint8Array): WsFrameDecoded {
  try {
    const top = parseMessage(bytes);

    // Extract top-level request_id (field 1)
    const requestId = getStr(top, 1);

    // Check which oneof payload field is set (fields 10–30)
    for (const [fn, arr] of top.entries()) {
      if (!(arr[0] instanceof Uint8Array)) continue;
      const payload = arr[0];

      switch (fn) {
        case 10: { // tell (incoming from server — server routing a tell back to thin node)
          const t = parseIncomingTell(requestId, payload);
          return { type: 'incoming_tell', requestId, actorId: t.actorId, msgType: t.msgType, payloadJson: t.payloadJson };
        }
        case 11: { // tell_response
          const t = parseTellResponse(payload);
          return { type: 'tell_response', requestId: t.requestId || requestId, success: t.success, errorMessage: t.errorMessage };
        }
        case 13: { // ask_response
          const a = parseAskResponse(payload);
          return { type: 'ask_response', requestId: a.requestId || requestId, success: a.success, payloadJson: a.payloadJson, errorMessage: a.errorMessage };
        }
        case 21: { // node_register_ack
          const r = parseRegisterAck(payload);
          return { type: 'node_register_ack', requestId, success: r.success, assignedNodeId: r.assignedNodeId, errorMessage: r.errorMessage };
        }
        case 23: { // node_ping_response
          const p = parsePingResponse(payload);
          return { type: 'node_ping_response', requestId: p.requestId || requestId, nodeId: p.nodeId, cpuPercent: p.cpuPercent, memoryAvailableMb: p.memoryAvailableMb, availableCores: p.availableCores };
        }
        case 25: // heartbeat_ack
          return { type: 'heartbeat_ack', requestId };
        case 30: { // error
          const e = parseError(payload);
          return { type: 'error', requestId: e.requestId || requestId, code: e.code, message: e.message };
        }
      }
    }

    return { type: 'unknown' };
  } catch {
    return { type: 'unknown' };
  }
}
