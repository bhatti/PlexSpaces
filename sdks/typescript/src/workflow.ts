// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Workflow retry helpers aligned with proto RetryConfig (plexspaces.workflow.v1.RetryConfig).
// Single run-with-config semantics: use defaultRetryConfig() for one attempt; pass config with
// max_attempts > 1 for retries.

/**
 * Retry configuration aligned with proto RetryConfig.
 * Unset max_attempts is treated as 1 (single attempt) at runtime.
 */
export interface RetryConfig {
  /** Maximum attempts (1 = no retries; unset/0 treated as 1). */
  max_attempts?: number;
  /** Initial retry interval ms. Default 100. */
  initial_interval_ms?: number;
  /** Backoff rate (e.g. 2 = doubling). Default 2. */
  backoff_rate?: number;
  /** Max interval ms. Default 30000. */
  max_interval_ms?: number;
  /** Error types to retry (empty = all). */
  retryable_errors?: string[];
}

/** Default retry config: single attempt (no retries). Aligns with proto when fields unset. */
export function defaultRetryConfig(): RetryConfig {
  return {
    max_attempts: 1,
    initial_interval_ms: 100,
    backoff_rate: 2,
    max_interval_ms: 30000,
    retryable_errors: [],
  };
}

/** Effective max attempts: 0 or unset means 1. */
function effectiveMaxAttempts(c: RetryConfig): number {
  const n = c.max_attempts ?? 0;
  return n === 0 ? 1 : n;
}

/**
 * Execute a step with retries. Uses RetryConfig. When config is omitted or max_attempts unset, 3 attempts.
 * When config.max_attempts is 0 or 1, single attempt. Aligns with Rust run(name, retry, op) and proto RetryConfig.
 *
 * @param fn - Function to execute. Must return a value or throw.
 * @param config - Optional RetryConfig (omitted => 3 attempts; max_attempts: 1 => one attempt).
 * @returns The result of fn() after a successful attempt.
 * @throws The last error if all attempts fail.
 */
export function withRetry<T>(fn: () => T, config: RetryConfig = {}): T {
  const fromConfig = config.max_attempts !== undefined ? effectiveMaxAttempts(config) : 0;
  const maxAttempts = fromConfig > 0 ? fromConfig : 3;
  let lastError: unknown;
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      return fn();
    } catch (e) {
      lastError = e;
      if (attempt === maxAttempts) {
        throw e;
      }
    }
  }
  throw lastError;
}
