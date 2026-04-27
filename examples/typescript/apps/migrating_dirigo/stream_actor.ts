// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Dirigo → PlexSpaces: Real-time analytics stream (TypeScript WASM)
//
// Real-world use case: Kafka-style windowed aggregation (clickstream).
// Workflow run: process event batch, maintain window, emit windowed aggregates.
// Handlers: ingest (single event), window_flush (emit aggregate for current window).
//
// Native Dirigo: virtual actors for stream operators (map, filter, reduce, window).
// PlexSpaces: WorkflowActor + virtual_actor + durability; window in state.

import { WorkflowActor, host } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

interface StreamEvent {
  event_id?: string;
  value?: number;
  ts?: number;
  [key: string]: unknown;
}

interface WindowedStreamState {
  stream_id: string;
  window_size: number;
  window: StreamEvent[];
  processed_count: number;
  windows_emitted: number;
  status: string;
  total_compute_ms: number;
  total_coord_ms: number;
  created_at_ms: number;
  updated_at_ms: number;
  cancel_requested: boolean;
}

const DEFAULT_WINDOW_SIZE = 10;
const COMPUTE_MS_PER_EVENT = 0.2;

// ========================================================================
// Windowed Stream Actor
// ========================================================================

/**
 * Windowed stream aggregator - Dirigo-style real-time analytics.
 * run(): process batch of events, push to window, emit aggregate when window full.
 * onIngest: add single event. onWindow_flush: emit aggregate for current window.
 */
export class WindowedStreamActor extends WorkflowActor<WindowedStreamState> {
  getDefaultState(): WindowedStreamState {
    return {
      stream_id: "",
      window_size: DEFAULT_WINDOW_SIZE,
      window: [],
      processed_count: 0,
      windows_emitted: 0,
      status: "idle",
      total_compute_ms: 0,
      total_coord_ms: 0,
      created_at_ms: 0,
      updated_at_ms: 0,
      cancel_requested: false,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.stream_id = String(config.stream_id ?? this.state.stream_id);
    const ws = config.window_size;
    if (typeof ws === "number" && ws > 0) this.state.window_size = ws;
    this.state.created_at_ms = host.nowMs();
    this.state.updated_at_ms = this.state.created_at_ms;
  }

  /** Main workflow: process event batch, window aggregation, return metrics. */
  run(payload: Record<string, unknown>): Record<string, unknown> {
    const t0 = host.nowMs();
    this.state.stream_id = String(payload.stream_id ?? this.state.stream_id);
    const windowSize = Number(payload.window_size) || this.state.window_size;
    if (windowSize > 0) this.state.window_size = windowSize;
    this.state.updated_at_ms = host.nowMs();

    if (this.state.cancel_requested) {
      this.state.status = "cancelled";
      return this.finish(t0, 0, "cancelled");
    }

    const events = (payload.events as StreamEvent[] | undefined) ?? [];
    if (events.length === 0) {
      return this.finish(t0, 0, this.state.status || "idle");
    }

    this.state.status = "running";
    let computeMs = 0;

    for (const ev of events) {
      if (this.state.cancel_requested) {
        this.state.status = "cancelled";
        return this.finish(t0, computeMs, "cancelled");
      }
      this.state.window.push(ev);
      this.state.processed_count += 1;
      computeMs += COMPUTE_MS_PER_EVENT;
      if (this.state.window.length >= this.state.window_size) {
        const agg = this.emitWindow();
        this.state.windows_emitted += 1;
        (agg as Record<string, unknown>).window_index = this.state.windows_emitted;
      }
    }

    this.state.updated_at_ms = host.nowMs();
    this.state.status = "idle";
    return this.finish(t0, computeMs, "idle");
  }

  signal(name: string, _data: Record<string, unknown>): void {
    if (name === "cancel") {
      this.state.cancel_requested = true;
      this.state.updated_at_ms = host.nowMs();
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "status") {
      return {
        stream_id: this.state.stream_id,
        status: this.state.status,
        window_size: this.state.window_size,
        window_count: this.state.window.length,
        processed_count: this.state.processed_count,
        windows_emitted: this.state.windows_emitted,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        created_at_ms: this.state.created_at_ms,
        updated_at_ms: this.state.updated_at_ms,
        cancel_requested: this.state.cancel_requested,
      };
    }
    return { error: "unknown_query", name };
  }

  /** Add single event (handler). */
  onIngest(payload: Record<string, unknown>): Record<string, unknown> {
    const ev: StreamEvent = {
      event_id: String(payload.event_id ?? host.nowMs()),
      value: Number(payload.value ?? 0),
      ts: Number(payload.ts ?? host.nowMs()),
      ...payload,
    };
    this.state.window.push(ev);
    this.state.processed_count += 1;
    this.state.updated_at_ms = host.nowMs();
    return { ok: true, window_count: this.state.window.length };
  }

  /** Emit aggregate for current window and clear (handler). */
  onWindow_flush(_payload: Record<string, unknown>): Record<string, unknown> {
    const agg = this.emitWindow();
    this.state.windows_emitted += 1;
    this.state.updated_at_ms = host.nowMs();
    return { ok: true, aggregate: agg, windows_emitted: this.state.windows_emitted };
  }

  private emitWindow(): Record<string, unknown> {
    const w = this.state.window;
    const values = w.map((e) => (typeof e.value === "number" ? e.value : 0));
    const sum = values.reduce((a, b) => a + b, 0);
    const count = values.length;
    const avg = count > 0 ? sum / count : 0;
    const min = count > 0 ? Math.min(...values) : 0;
    const max = count > 0 ? Math.max(...values) : 0;
    this.state.window = [];
    return { count, sum, avg, min, max };
  }

  private finish(
    t0: number,
    computeMs: number,
    status: string
  ): Record<string, unknown> {
    const elapsed = host.nowMs() - t0;
    const coordMs = Math.max(0, elapsed - computeMs);
    this.state.total_compute_ms += computeMs;
    this.state.total_coord_ms += coordMs;
    this.state.status = status;
    this.state.updated_at_ms = host.nowMs();
    return {
      status: this.state.status,
      stream_id: this.state.stream_id,
      processed_count: this.state.processed_count,
      windows_emitted: this.state.windows_emitted,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    };
  }
}

const actorInstance = new WindowedStreamActor();
export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
