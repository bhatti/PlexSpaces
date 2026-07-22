// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WsThinClient — browser and Node.js WebSocket thin-node client.
//
// Speaks the binary protobuf WsFrame protocol defined in
// proto/plexspaces/v1/transport/websocket.proto.
//
// Usage:
//   const client = new WsThinClient({ wsUrl: 'ws://localhost:8091/ws', jwtToken });
//   const nodeId = await client.connect();
//   const myId = client.localActorId('alice', 'ChatClient', 'chat');
//   await client.tell(chatRoomId, 'send', { text: 'hello' });
//   const reply = await client.ask(chatRoomId, 'status', {});
//   client.onMessage((from, msgType, payload) => console.log(from, msgType, payload));
//   await client.disconnect();

import {
  decodeWsFrame,
  encodeWsFrameAsk,
  encodeWsFrameHeartbeat,
  encodeWsFrameNodePing,
  encodeWsFrameNodeRegister,
  encodeWsFrameTell,
} from './wire/ws-frame-wire.js';

// ─── Inline ULID generator (Crockford base32, ~20 lines, zero deps) ─────────

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';

function newUlid(): string {
  const now = Date.now();
  // 10-char timestamp component (48 bits → 10 Crockford base32 chars)
  let t = now;
  let ts = '';
  for (let i = 9; i >= 0; i--) {
    ts = CROCKFORD[t % 32]! + ts;
    t = Math.floor(t / 32);
  }
  // 16-char randomness component (80 bits → 16 Crockford base32 chars)
  // 10 bytes × 8 bits = 80 bits; bit-pack into 16 × 5-bit groups.
  const rb = new Uint8Array(10);
  if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
    crypto.getRandomValues(rb);
  } else {
    for (let i = 0; i < 10; i++) rb[i] = Math.floor(Math.random() * 256);
  }
  let rand = '';
  let acc = 0, bits = 0;
  for (let i = 0; i < 10; i++) {
    acc = (acc << 8) | rb[i]!;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      rand += CROCKFORD[(acc >>> bits) & 0x1f];
    }
  }
  // 10×8=80 bits, 80/5=16 chars exactly; bits===0 here.
  return ts + rand; // 10 + 16 = 26 chars — valid ULID
}

// ─── Public types ────────────────────────────────────────────────────────────

export interface ThinClientOptions {
  /** WebSocket URL, e.g. "ws://localhost:8091/ws" */
  wsUrl: string;
  /** JWT Bearer token. Appended as ?token=<jwt> on the WS URL if present. */
  jwtToken?: string;
  /** ULID preferred. Server assigns a new one if omitted or collision detected. */
  nodeId?: string;
  /** Placed in capabilities["tenant"] during registration. */
  tenant?: string;
  /** Placed in capabilities["namespace"] during registration. */
  namespace?: string;
}

/**
 * Resource snapshot from a PingResponse (proto fields 9–11 of PingResponse).
 * Not a proto message — TypeScript interface over decoded fields.
 */
export interface ThinNodePingResult {
  success: boolean;
  nodeId: string;
  cpuPercent: number;
  memoryAvailableMb: number;
  availableCores: number;
}

// ─── Internal pending-request maps ──────────────────────────────────────────

interface PendingAsk {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface PendingPing {
  resolve: (v: ThinNodePingResult) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

// ─── WsThinClient ────────────────────────────────────────────────────────────

export class WsThinClient {
  private ws: WebSocket | null = null;
  private assignedNodeId: string = '';
  private readonly pendingAsks = new Map<string, PendingAsk>();
  private readonly pendingPings = new Map<string, PendingPing>();
  private pendingReg: { resolve: (id: string) => void; reject: (e: Error) => void } | null = null;
  private messageHandler: ((actorId: string, msgType: string, payload: unknown) => void) | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private readonly HEARTBEAT_INTERVAL_MS = 25_000;
  private readonly DEFAULT_ASK_TIMEOUT_MS = 5_000;

  constructor(private readonly opts: ThinClientOptions) {}

  /**
   * Open the WebSocket, complete the NodeRegistration handshake, and start the
   * heartbeat loop.  Returns the server-assigned node_id.
   */
  async connect(): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      // Build URL — append token as query param if provided
      let url = this.opts.wsUrl;
      if (this.opts.jwtToken) {
        const sep = url.includes('?') ? '&' : '?';
        url = `${url}${sep}token=${encodeURIComponent(this.opts.jwtToken)}`;
      }

      this.ws = new WebSocket(url);
      this.ws.binaryType = 'arraybuffer';

      this.ws.onopen = () => {
        // Send NodeRegistration as first frame
        const nodeId = this.opts.nodeId ?? newUlid();
        const capabilities: Record<string, string> = {};
        if (this.opts.tenant) capabilities['tenant'] = this.opts.tenant;
        if (this.opts.namespace) capabilities['namespace'] = this.opts.namespace;
        // Include client resource hints if available (browser context)
        if (typeof navigator !== 'undefined') {
          capabilities['cpu_cores'] = String(navigator.hardwareConcurrency ?? 1);
        }

        // Collect browser resource hints for NodeResourceHints field 11
        const resourceHints: { cpuPercent?: number; memoryAvailableMb?: number; availableCores?: number } = {};
        if (typeof navigator !== 'undefined') {
          resourceHints.availableCores = navigator.hardwareConcurrency ?? 1;
        }

        const requestId = newUlid();
        this.pendingReg = { resolve, reject };
        const frame = encodeWsFrameNodeRegister(requestId, nodeId, '', capabilities, resourceHints);
        this.ws!.send(frame);
      };

      this.ws.onmessage = (ev: MessageEvent) => {
        const buf = ev.data instanceof ArrayBuffer
          ? new Uint8Array(ev.data)
          : new Uint8Array(ev.data as ArrayBuffer);
        this.handleFrame(buf);
      };

      this.ws.onerror = () => {
        const err = new Error('WebSocket error');
        this.rejectAllPending(err);
        reject(err);
      };

      this.ws.onclose = (ev: CloseEvent) => {
        const err = new Error(`WebSocket closed: ${ev.code} ${ev.reason}`);
        this.rejectAllPending(err);
        if (this.pendingReg) {
          this.pendingReg.reject(err);
          this.pendingReg = null;
        }
        this.stopHeartbeat();
      };
    });
  }

  /**
   * Fire-and-forget. actorId must be the canonical form:
   *   {name}//{type}::{namespace}@{nodeId}
   */
  async tell(actorId: string, msgType: string, payload: unknown): Promise<void> {
    const frame = encodeWsFrameTell(
      newUlid(),
      actorId,
      msgType,
      new TextEncoder().encode(JSON.stringify(payload)),
    );
    this.send(frame);
  }

  /**
   * Request-reply. Returns the response payload (parsed JSON).
   * Rejects with a timeout error if no response arrives within timeoutMs.
   */
  async ask(actorId: string, msgType: string, payload: unknown, timeoutMs = this.DEFAULT_ASK_TIMEOUT_MS): Promise<unknown> {
    const requestId = newUlid();
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingAsks.delete(requestId);
        reject(new Error(`ask timeout after ${timeoutMs}ms for ${actorId}`));
      }, timeoutMs);
      this.pendingAsks.set(requestId, { resolve, reject, timer });

      const frame = encodeWsFrameAsk(
        requestId,
        actorId,
        msgType,
        new TextEncoder().encode(JSON.stringify(payload)),
        timeoutMs,
      );
      this.send(frame);
    });
  }

  /**
   * Register a handler for incoming tell frames addressed to this thin node.
   * Called whenever the server routes a tell to one of this node's actor IDs.
   */
  onMessage(handler: (actorId: string, msgType: string, payload: unknown) => void): void {
    this.messageHandler = handler;
  }

  /**
   * Send a SWIM-compatible ping to a target node and return its resource hints.
   * The target nodeId must be known to the server (registered in SWIM membership).
   */
  async pingNode(targetNodeId: string, timeoutMs = 5_000): Promise<ThinNodePingResult> {
    const requestId = newUlid();
    return new Promise<ThinNodePingResult>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pendingPings.delete(requestId);
        reject(new Error(`ping timeout for ${targetNodeId}`));
      }, timeoutMs);
      this.pendingPings.set(requestId, { resolve, reject, timer });
      const frame = encodeWsFrameNodePing(requestId, this.assignedNodeId, Date.now());
      this.send(frame);
    });
  }

  /** Send a heartbeat frame to keep the WS session alive. */
  async heartbeat(): Promise<void> {
    const frame = encodeWsFrameHeartbeat(newUlid(), this.assignedNodeId);
    this.send(frame);
  }

  /**
   * Build a canonical actor ID on this thin node.
   * Format: {name}//{type}::{namespace}@{assignedNodeId}
   */
  localActorId(name: string, type: string, namespace?: string): string {
    const ns = namespace ?? this.opts.namespace ?? 'default';
    return `${name}//${type}::${ns}@${this.assignedNodeId}`;
  }

  /** The server-assigned node_id (available after connect() resolves). */
  get nodeId(): string {
    return this.assignedNodeId;
  }

  /** Generate a new ULID. Exposed so examples can use it without a dep. */
  static newUlid(): string {
    return newUlid();
  }

  /** Disconnect and clean up. */
  async disconnect(): Promise<void> {
    this.stopHeartbeat();
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.close(1000, 'client disconnect');
    }
    this.ws = null;
  }

  // ─── Private ──────────────────────────────────────────────────────────────

  private send(data: Uint8Array): void {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error('WsThinClient: not connected');
    }
    this.ws.send(data);
  }

  private handleFrame(buf: Uint8Array): void {
    const frame = decodeWsFrame(buf);
    switch (frame.type) {
      case 'node_register_ack': {
        if (this.pendingReg) {
          const reg = this.pendingReg;
          this.pendingReg = null;
          if (frame.success) {
            this.assignedNodeId = frame.assignedNodeId;
            this.startHeartbeat();
            reg.resolve(frame.assignedNodeId);
          } else {
            reg.reject(new Error(`registration failed: ${frame.errorMessage}`));
          }
        }
        break;
      }
      case 'ask_response': {
        const pending = this.pendingAsks.get(frame.requestId);
        if (pending) {
          clearTimeout(pending.timer);
          this.pendingAsks.delete(frame.requestId);
          if (frame.success) {
            pending.resolve(frame.payloadJson);
          } else {
            pending.reject(new Error(frame.errorMessage || 'ask failed'));
          }
        }
        break;
      }
      case 'tell_response':
        // Fire-and-forget — nothing to resolve
        break;
      case 'heartbeat_ack':
        // Heartbeat acknowledged — nothing to do
        break;
      case 'node_ping_response': {
        const pending = this.pendingPings.get(frame.requestId);
        if (pending) {
          clearTimeout(pending.timer);
          this.pendingPings.delete(frame.requestId);
          pending.resolve({
            success: true,
            nodeId: frame.nodeId,
            cpuPercent: frame.cpuPercent,
            memoryAvailableMb: frame.memoryAvailableMb,
            availableCores: frame.availableCores,
          });
        }
        break;
      }
      case 'incoming_tell': {
        if (this.messageHandler) {
          this.messageHandler(frame.actorId, frame.msgType, frame.payloadJson);
        }
        break;
      }
      case 'error': {
        // Reject the matching pending ask if one exists
        const pending = this.pendingAsks.get(frame.requestId);
        if (pending) {
          clearTimeout(pending.timer);
          this.pendingAsks.delete(frame.requestId);
          pending.reject(new Error(`server error ${frame.code}: ${frame.message}`));
        }
        break;
      }
      // 'unknown' — silently ignored (forward-compatible)
    }
  }

  private startHeartbeat(): void {
    this.heartbeatTimer = setInterval(() => {
      this.heartbeat().catch(() => { /* ignore heartbeat errors */ });
    }, this.HEARTBEAT_INTERVAL_MS);
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer !== null) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private rejectAllPending(err: Error): void {
    for (const [, p] of this.pendingAsks) {
      clearTimeout(p.timer);
      p.reject(err);
    }
    this.pendingAsks.clear();
    for (const [, p] of this.pendingPings) {
      clearTimeout(p.timer);
      p.reject(err);
    }
    this.pendingPings.clear();
  }
}
