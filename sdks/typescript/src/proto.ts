// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Proto-shaped SDK types.
//
// The repository can generate richer TypeScript models via `make proto-typescript`,
// but the SDK itself must still build in environments where those generated files
// are absent. These interfaces preserve the canonical proto field names so
// application code stays proto-first at the SDK boundary.

export interface RetryConfig {
  max_attempts?: number;
  initial_interval_ms?: number;
  backoff_rate?: number;
  max_interval_ms?: number;
  retryable_errors?: string[];
}

export interface Facet {
  type: string;
  config?: Record<string, string>;
  priority?: number;
}

export interface Message {
  id?: string;
  message_type?: string;
  payload?: Uint8Array | string;
}

export interface RequestContext {
  tenant_id?: string;
  namespace?: string;
}

export interface ErrorDetail {
  code?: string;
  message?: string;
  details?: Record<string, unknown>;
}

export interface ActorConfig {
  max_mailbox_size?: number;
  enable_persistence?: boolean;
  actor_groups?: string[];
  properties?: Record<string, unknown>;
  config_schema_version?: number;
}
