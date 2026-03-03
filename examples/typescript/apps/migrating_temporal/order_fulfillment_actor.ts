// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Temporal → PlexSpaces: E-commerce Order Fulfillment Workflow (TypeScript WASM)
//
// Real-world use case: Multi-step order fulfillment with compensation (saga).
// - Workflow behavior: run (main execution), signal (e.g. cancel), query (e.g. status)
// - Durability: State persisted via getState/setState (DurabilityFacet in app-config)
// - Aligned with Rust Workflow trait and Python @workflow_actor
//
// Native Temporal: TypeScript SDK @temporalio/workflow (see native/temporal_order_workflow.ts)
// PlexSpaces: WorkflowActor with run/signal/query; message_type workflow_run | workflow_signal:name | workflow_query:name

import { WorkflowActor, host } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

type OrderStatus =
  | "pending"
  | "validated"
  | "inventory_reserved"
  | "payment_charged"
  | "shipped"
  | "cancelled"
  | "failed";

interface OrderStep {
  name: string;
  completed_at_ms: number;
  payload?: Record<string, unknown>;
}

interface OrderFulfillmentState {
  order_id: string;
  customer_id: string;
  status: OrderStatus;
  steps: OrderStep[];
  cancel_requested: boolean;
  total_compute_ms: number;
  total_coord_ms: number;
  created_at_ms: number;
  updated_at_ms: number;
}

// ========================================================================
// Order Fulfillment Workflow Actor
// ========================================================================

/**
 * Order Fulfillment Workflow - Temporal-style saga with run/signal/query.
 *
 * Demonstrates:
 * - Workflow behavior: run() = main execution, signal() = cancel, query() = status
 * - Saga steps: validate → reserve inventory → charge payment → ship (with compensation on failure/cancel)
 * - Durability: State checkpointed via getState/setState
 *
 * Message types (from framework): workflow_run, workflow_signal:cancel, workflow_query:status
 */
export class OrderFulfillmentActor extends WorkflowActor<OrderFulfillmentState> {
  getDefaultState(): OrderFulfillmentState {
    // Do not call host.nowMs() here: getDefaultState() runs at module load (Wizer) when host imports are unavailable.
    return {
      order_id: "",
      customer_id: "",
      status: "pending",
      steps: [],
      cancel_requested: false,
      total_compute_ms: 0,
      total_coord_ms: 0,
      created_at_ms: 0,
      updated_at_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.order_id = String(config.order_id ?? this.state.order_id);
    this.state.customer_id = String(config.customer_id ?? this.state.customer_id);
    this.state.created_at_ms = host.nowMs();
    this.state.updated_at_ms = this.state.created_at_ms;
  }

  /** Main workflow execution (exclusive). Called when msgType is "workflow_run". */
  run(payload: Record<string, unknown>): Record<string, unknown> {
    const t0 = host.nowMs();
    const orderId = String(payload.order_id ?? this.state.order_id);
    const customerId = String(payload.customer_id ?? payload.customer_id ?? this.state.customer_id);
    if (orderId) this.state.order_id = orderId;
    if (customerId) this.state.customer_id = customerId;
    this.state.updated_at_ms = host.nowMs();

    if (this.state.status !== "pending" && this.state.status !== "validated") {
      return {
        status: this.state.status,
        order_id: this.state.order_id,
        message: "Workflow already completed or cancelled",
      };
    }

    let computeMs = 0;
    try {
      // Step 1: Validate order (simulated; compute time for metrics only, no busy loop in WASM)
      if (!this.state.steps.some((s) => s.name === "validate")) {
        const stepCompute = 8;
        computeMs += stepCompute;
        this.state.steps.push({
          name: "validate",
          completed_at_ms: host.nowMs(),
          payload: { order_id: this.state.order_id },
        });
        this.state.status = "validated";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.compensate("cancel_requested");

      // Step 2: Reserve inventory
      if (!this.state.steps.some((s) => s.name === "reserve_inventory")) {
        computeMs += 12;
        this.state.steps.push({
          name: "reserve_inventory",
          completed_at_ms: host.nowMs(),
        });
        this.state.status = "inventory_reserved";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.compensate("cancel_requested");

      // Step 3: Charge payment
      if (!this.state.steps.some((s) => s.name === "charge_payment")) {
        computeMs += 10;
        this.state.steps.push({
          name: "charge_payment",
          completed_at_ms: host.nowMs(),
        });
        this.state.status = "payment_charged";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.compensate("cancel_requested");

      // Step 4: Ship
      if (!this.state.steps.some((s) => s.name === "ship")) {
        computeMs += 15;
        this.state.steps.push({
          name: "ship",
          completed_at_ms: host.nowMs(),
        });
        this.state.status = "shipped";
        this.state.updated_at_ms = host.nowMs();
      }

      const totalElapsedMs = host.nowMs() - t0;
      const computeReported = Math.min(computeMs, totalElapsedMs);
      const coordReported = Math.max(0, totalElapsedMs - computeReported);
      this.state.total_compute_ms += computeReported;
      this.state.total_coord_ms += coordReported;

      return {
        status: this.state.status,
        order_id: this.state.order_id,
        steps_completed: this.state.steps.length,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
      };
    } catch (e) {
      this.state.status = "failed";
      this.state.updated_at_ms = host.nowMs();
      return this.compensate(String(e instanceof Error ? e.message : e));
    }
  }

  /** Handle external signal (e.g. cancel). Called when msgType is "workflow_signal:name". */
  signal(name: string, _data: Record<string, unknown>): void {
    if (name === "cancel") {
      this.state.cancel_requested = true;
      this.state.updated_at_ms = host.nowMs();
    }
  }

  /** Read-only query. Called when msgType is "workflow_query:name". */
  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "status") {
      return {
        order_id: this.state.order_id,
        customer_id: this.state.customer_id,
        status: this.state.status,
        steps_count: this.state.steps.length,
        cancel_requested: this.state.cancel_requested,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        created_at_ms: this.state.created_at_ms,
        updated_at_ms: this.state.updated_at_ms,
      };
    }
    return { error: "unknown_query", name };
  }

  private compensate(reason: string): Record<string, unknown> {
    this.state.status = "cancelled";
    this.state.updated_at_ms = host.nowMs();
    return {
      status: "cancelled",
      order_id: this.state.order_id,
      reason,
      steps_rolled_back: this.state.steps.length,
    };
  }
}

// Export actor for WASM component (WIT actor-world)
const actorInstance = new OrderFulfillmentActor();
export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
