// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Azure Durable Functions → PlexSpaces: Document Processing Workflow (TypeScript WASM)
//
// Real-world use case: OCR → classify → extract → store (fan-out/fan-in).
// Fault tolerance: Step Functions / Durable Functions style retry (max attempts, then catch).
// - Workflow behavior: run (main execution), signal (cancel), query (status)
// - Virtual actor: one workflow per job (document-processing:job-123)
// - Durability: state persisted via getState/setState
// - Retry: withRetry() for steps that can fail transiently; catch sets status "failed".

import { WorkflowActor, host, withRetry } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

type DocStatus =
  | "pending"
  | "ocr_done"
  | "classified"
  | "extracted"
  | "stored"
  | "cancelled"
  | "failed";

interface DocStep {
  name: string;
  completed_at_ms: number;
  retry_count?: number;
  payload?: Record<string, unknown>;
}

interface DocumentProcessingState {
  job_id: string;
  status: DocStatus;
  steps: DocStep[];
  ocr_results: { page: number; text_len: number }[];
  cancel_requested: boolean;
  total_compute_ms: number;
  total_coord_ms: number;
  created_at_ms: number;
  updated_at_ms: number;
  /** Total retries used across steps (for metrics). */
  total_retry_count: number;
}

const MAX_RETRIES_PER_STEP = 3;

const OCR_PAGES = 4;
const OCR_MS_PER_PAGE = 20;
const CLASSIFY_MS = 30;
const EXTRACT_MS = 25;
const STORE_MS = 15;

/** Simulate transient failure on first attempt for ~1/3 of jobs (deterministic by job_id). */
function simulateTransientFailure(jobId: string, attemptNumber: number): boolean {
  if (attemptNumber > 1) return false;
  let h = 0;
  for (let i = 0; i < jobId.length; i++) h = (h * 31 + jobId.charCodeAt(i)) >>> 0;
  return h % 3 === 0;
}

// ========================================================================
// Document Processing Workflow Actor
// ========================================================================

/**
 * Document Processing Workflow - Azure Durable Functions style.
 * Steps: OCR (fan-out N pages) → classify → extract → store.
 */
export class DocumentProcessingActor extends WorkflowActor<DocumentProcessingState> {
  getDefaultState(): DocumentProcessingState {
    return {
      job_id: "",
      status: "pending",
      steps: [],
      ocr_results: [],
      cancel_requested: false,
      total_compute_ms: 0,
      total_coord_ms: 0,
      created_at_ms: 0,
      updated_at_ms: 0,
      total_retry_count: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    this.state.job_id = String(config.job_id ?? this.state.job_id);
    this.state.created_at_ms = host.nowMs();
    this.state.updated_at_ms = this.state.created_at_ms;
  }

  run(payload: Record<string, unknown>): Record<string, unknown> {
    const t0 = host.nowMs();
    const jobId = String(payload.job_id ?? this.state.job_id);
    if (jobId) this.state.job_id = jobId;
    this.state.updated_at_ms = host.nowMs();

    if (
      this.state.status !== "pending" &&
      this.state.status !== "ocr_done" &&
      this.state.status !== "classified" &&
      this.state.status !== "extracted"
    ) {
      return {
        status: this.state.status,
        job_id: this.state.job_id,
        message: "Workflow already completed or cancelled",
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
      };
    }

    let computeMs = 0;
    const t0Capture = t0;
    try {
      const stepNames = this.state.steps.map((s) => s.name);

      // Step 1: OCR (fan-out: N pages in parallel, then fan-in)
      if (!stepNames.includes("ocr")) {
        for (let page = 0; page < OCR_PAGES; page++) {
          computeMs += OCR_MS_PER_PAGE;
          this.state.ocr_results.push({ page, text_len: 500 });
        }
        this.state.steps.push({
          name: "ocr",
          completed_at_ms: host.nowMs(),
          payload: { pages: OCR_PAGES },
        });
        this.state.status = "ocr_done";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.finishCancelled(computeMs, t0Capture);

      // Step 2: Classify (with retry - Step Functions / Durable Functions style)
      if (!stepNames.includes("classify")) {
        let attempts = 0;
        let stepRetries = 0;
        try {
          withRetry(
            () => {
              attempts += 1;
              computeMs += CLASSIFY_MS;
              if (simulateTransientFailure(this.state.job_id, attempts)) {
                stepRetries += 1;
                this.state.total_retry_count += 1;
                throw new Error("ClassifyServiceUnavailable");
              }
            },
            { max_attempts: MAX_RETRIES_PER_STEP }
          );
        } catch (e) {
          this.state.status = "failed";
          this.state.updated_at_ms = host.nowMs();
          return this.finishFailed(computeMs, t0Capture, "classify", String(e instanceof Error ? e.message : e));
        }
        this.state.steps.push({
          name: "classify",
          completed_at_ms: host.nowMs(),
          retry_count: stepRetries,
        });
        this.state.status = "classified";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.finishCancelled(computeMs, t0Capture);

      // Step 3: Extract
      if (!stepNames.includes("extract")) {
        computeMs += EXTRACT_MS;
        this.state.steps.push({ name: "extract", completed_at_ms: host.nowMs() });
        this.state.status = "extracted";
        this.state.updated_at_ms = host.nowMs();
      }
      if (this.state.cancel_requested) return this.finishCancelled(computeMs, t0Capture);

      // Step 4: Store
      if (!stepNames.includes("store")) {
        computeMs += STORE_MS;
        this.state.steps.push({ name: "store", completed_at_ms: host.nowMs() });
        this.state.status = "stored";
        this.state.updated_at_ms = host.nowMs();
      }

      const totalElapsedMs = host.nowMs() - t0;
      const computeReported = Math.min(computeMs, totalElapsedMs);
      const coordReported = Math.max(0, totalElapsedMs - computeReported);
      this.state.total_compute_ms += computeReported;
      this.state.total_coord_ms += coordReported;

      return {
        status: this.state.status,
        job_id: this.state.job_id,
        steps_completed: this.state.steps.length,
        total_retry_count: this.state.total_retry_count,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
      };
    } catch (e) {
      this.state.status = "failed";
      this.state.updated_at_ms = host.nowMs();
      return this.finishCancelled(computeMs, t0Capture);
    }
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
        job_id: this.state.job_id,
        status: this.state.status,
        steps_count: this.state.steps.length,
        cancel_requested: this.state.cancel_requested,
        total_compute_ms: this.state.total_compute_ms,
        total_coord_ms: this.state.total_coord_ms,
        total_retry_count: this.state.total_retry_count,
        created_at_ms: this.state.created_at_ms,
        updated_at_ms: this.state.updated_at_ms,
      };
    }
    return { error: "unknown_query", name };
  }

  /** Catch path: step failed after retries exhausted (Step Functions Catch / Durable Functions). */
  private finishFailed(
    computeMs: number,
    t0: number,
    failed_step: string,
    error_message: string
  ): Record<string, unknown> {
    this.state.status = "failed";
    this.state.updated_at_ms = host.nowMs();
    const totalElapsedMs = host.nowMs() - t0;
    const computeReported = Math.min(computeMs, totalElapsedMs);
    const coordReported = Math.max(0, totalElapsedMs - computeReported);
    this.state.total_compute_ms += computeReported;
    this.state.total_coord_ms += coordReported;
    return {
      status: "failed",
      job_id: this.state.job_id,
      failed_step,
      error: error_message,
      total_retry_count: this.state.total_retry_count,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    };
  }

  private finishCancelled(computeMs: number, t0: number): Record<string, unknown> {
    this.state.status = "cancelled";
    this.state.updated_at_ms = host.nowMs();
    const totalElapsedMs = host.nowMs() - t0;
    const computeReported = Math.min(computeMs, totalElapsedMs);
    const coordReported = Math.max(0, totalElapsedMs - computeReported);
    this.state.total_compute_ms += computeReported;
    this.state.total_coord_ms += coordReported;
    return {
      status: "cancelled",
      job_id: this.state.job_id,
      reason: "cancel_requested",
      steps_rolled_back: this.state.steps.length,
      total_compute_ms: this.state.total_compute_ms,
      total_coord_ms: this.state.total_coord_ms,
    };
  }
}

const actorInstance = new DocumentProcessingActor();
export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
