// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Restate → PlexSpaces: Exactly-Once Payment (TypeScript WASM)
//
// Real-world use case: Idempotent payment with journaling replay (Restate-style).
// - Workflow + durability: state checkpointed so replay does not double-execute.
// - Idempotency: same idempotency_key returns cached result (exactly-once).
// - Steps: validate → debit → credit → confirm (simulated); result stored keyed by idempotency_key.

import { WorkflowActor, host } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

type PaymentStatus = "pending" | "validated" | "debited" | "credited" | "confirmed" | "cancelled" | "failed";

interface IdempotentResult {
  status: PaymentStatus;
  amount_cents: number;
  from_account: string;
  to_account: string;
  completed_at_ms: number;
  steps: string[];
}

interface PaymentActorState {
  /** Cached results by idempotency_key for exactly-once semantics */
  idempotency_results: Record<string, IdempotentResult>;
  cancel_requested: boolean;
  total_compute_ms: number;
  total_coord_ms: number;
  updated_at_ms: number;
}

const VALIDATE_MS = 15;
const DEBIT_MS = 25;
const CREDIT_MS = 25;
const CONFIRM_MS = 20;

// ========================================================================
// Exactly-Once Payment Actor (Restate-style)
// ========================================================================

/**
 * Payment actor with idempotency and durability (Restate-style).
 * Duplicate requests with same idempotency_key return cached result; steps are journaled via checkpoint.
 */
export class PaymentActor extends WorkflowActor<PaymentActorState> {
  getDefaultState(): PaymentActorState {
    return {
      idempotency_results: {},
      cancel_requested: false,
      total_compute_ms: 0,
      total_coord_ms: 0,
      updated_at_ms: 0,
    };
  }

  protected override onInit(_config: Record<string, unknown>): void {
    this.state.updated_at_ms = host.nowMs();
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const t0 = host.nowMs();
    const idempotencyKey = String(payload.idempotency_key ?? payload.idempotencyKey ?? "");
    const amountCents = Number(payload.amount_cents ?? payload.amountCents ?? 0);
    const fromAccount = String(payload.from_account ?? payload.fromAccount ?? "account-a");
    const toAccount = String(payload.to_account ?? payload.toAccount ?? "account-b");

    this.state.updated_at_ms = host.nowMs();

    if (this.state.cancel_requested) {
      return this.finish(t0, 0, {
        status: "cancelled",
        idempotency_key: idempotencyKey,
        message: "cancel_requested",
      });
    }

    // Exactly-once: return cached result if we already completed this idempotency_key
    const cached = this.state.idempotency_results[idempotencyKey];
    if (cached) {
      return this.finish(t0, 0, {
        status: cached.status,
        idempotency_key: idempotencyKey,
        amount_cents: cached.amount_cents,
        from_account: cached.from_account,
        to_account: cached.to_account,
        message: "already_completed",
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
      });
    }

    if (!idempotencyKey) {
      return this.finish(t0, 0, {
        status: "failed",
        error: "idempotency_key required",
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
      });
    }

    // Journaled steps (durability checkpoint after run; replay restores state so we don't double-execute)
    let computeMs = 0;
    const steps: string[] = [];

    computeMs += VALIDATE_MS;
    steps.push("validate");
    this.state.updated_at_ms = host.nowMs();

    if (this.state.cancel_requested) {
      return this.finish(t0, computeMs, { status: "cancelled", idempotency_key: idempotencyKey });
    }

    computeMs += DEBIT_MS;
    steps.push("debit");
    this.state.updated_at_ms = host.nowMs();

    computeMs += CREDIT_MS;
    steps.push("credit");
    this.state.updated_at_ms = host.nowMs();

    computeMs += CONFIRM_MS;
    steps.push("confirm");
    const result: IdempotentResult = {
      status: "confirmed",
      amount_cents: amountCents,
      from_account: fromAccount,
      to_account: toAccount,
      completed_at_ms: host.nowMs(),
      steps,
    };
    this.state.idempotency_results = { ...this.state.idempotency_results, [idempotencyKey]: result };
    this.state.updated_at_ms = result.completed_at_ms;

    return this.finish(t0, computeMs, {
      status: "confirmed",
      idempotency_key: idempotencyKey,
      amount_cents: amountCents,
      from_account: fromAccount,
      to_account: toAccount,
      steps_completed: steps.length,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    });
  }

  signal(_name: string, _data: Record<string, unknown>): void {
    if (_name === "cancel") {
      this.state.cancel_requested = true;
      this.state.updated_at_ms = host.nowMs();
    }
  }

  query(name: string, _params: Record<string, unknown>): Record<string, unknown> {
    if (name === "status") {
      return {
        idempotency_results_count: Object.keys(this.state.idempotency_results).length,
        cancel_requested: this.state.cancel_requested,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        updated_at_ms: this.state.updated_at_ms,
      };
    }
    return { error: "unknown_query", name };
  }

  private finish(
    t0: number,
    computeMs: number,
    out: Record<string, unknown>
  ): Record<string, unknown> {
    const elapsed = host.nowMs() - t0;
    const coordMs = Math.max(0, elapsed - computeMs);
    this.state.total_compute_ms += computeMs;
    this.state.total_coord_ms += coordMs;
    return {
      ...out,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    };
  }
}

// Export for WIT actor-world (init, handle, get-state, set-state)
const actorInstance = new PaymentActor();
export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
