// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Orbit vs PlexSpaces Comparison - Discord-style Read State Tracker (TypeScript WASM)
//
// Real-world use case: Per-user message read tracking (Discord read receipts)
// - Virtual actors: One actor per user (auto-activate on first message)
// - Durability: Read states persist across deactivation/reactivation
// - Efficient: Only active users consume memory
//
// Native Orbit: Java (JVM-based virtual actor framework)
// PlexSpaces: TypeScript WASM actor using @plexspaces/sdk
//
// Architecture:
// - VirtualActorFacet: Automatic activation/deactivation (lazy activation)
// - DurabilityFacet: State persistence (read states survive crashes)
// - Actor spawning handled by framework deployment (HTTP API)
// - Metrics provided by PlexSpaces runtime (coordination vs computation tracking)

import { PlexSpacesActor, host } from "@plexspaces/sdk";

// ========================================================================
// Types
// ========================================================================

interface ReadState {
  channel_id: string;
  last_read_message_id: string;
  last_read_timestamp: number;
}

interface UserReadState {
  user_id: string;
  channels: Record<string, ReadState>;
  total_channels: number;
  total_updates: number;
  last_updated: number;
}

interface ReadStateTrackerState {
  [key: string]: unknown;
  user_id: string;
  channels: Record<string, ReadState>;
  total_channels: number;
  total_updates: number;
  last_updated: number;
  // Metrics
  total_compute_ms: number;
  total_coord_ms: number;
}

// ========================================================================
// ReadStateTracker Actor
// ========================================================================

/**
 * ReadStateTracker Actor - Orbit-style virtual actor with durability
 *
 * Demonstrates:
 * - Virtual actor lifecycle: Automatic activation/deactivation per user
 * - Durability: Read states persist across crashes and deactivation
 * - Per-user state: One actor instance per user (efficient memory usage)
 * - Read receipt tracking: Track last read message per channel per user
 *
 * Note: VirtualActorFacet and DurabilityFacet are configured
 * via app-config.toml and framework deployment, not SDK.
 */
export class ReadStateTrackerActor extends PlexSpacesActor<ReadStateTrackerState> {
  getDefaultState(): ReadStateTrackerState {
    return {
      user_id: "",
      channels: {},
      total_channels: 0,
      total_updates: 0,
      last_updated: 0,
      total_compute_ms: 0,
      total_coord_ms: 0,
    };
  }

  protected override onInit(config: Record<string, unknown>): void {
    // user_id should be provided in config when actor is activated via HTTP
    // For ApplicationSpec deployment, user_id comes from child_spec.args
    // For virtual actor activation via HTTP, user_id should be built from instance_id in get_or_activate_actor_impl
    const userId = String(config.user_id ?? "");
    if (userId) {
      this.state.user_id = userId;
    }
    this.state.channels = {};
    this.state.total_channels = 0;
    this.state.total_updates = 0;
    this.state.last_updated = host.nowMs();
    this.state.total_compute_ms = 0;
    this.state.total_coord_ms = 0;
  }

  /**
   * Mark a message as read in a channel (Orbit: updateReadState)
   * Updates the last read message ID and timestamp for the user in the channel
   */
  onMark_read(payload: Record<string, unknown>): Record<string, unknown> {
    const startMs = host.nowMs();
    try {
      const channelId = String(payload.channel_id ?? "");
      const messageId = String(payload.message_id ?? "");
      const timestamp = Number(payload.timestamp ?? host.nowMs());

      if (!channelId || !messageId) {
        return {
          status: "error",
          error: "channel_id and message_id required",
        };
      }

      // Get or create read state for this channel
      let readState = this.state.channels[channelId];
      if (!readState) {
        readState = {
          channel_id: channelId,
          last_read_message_id: messageId,
          last_read_timestamp: timestamp,
        };
        this.state.channels[channelId] = readState;
        this.state.total_channels = Object.keys(this.state.channels).length;
      } else {
        // Update existing read state (only if message is newer)
        if (timestamp >= readState.last_read_timestamp) {
          readState.last_read_message_id = messageId;
          readState.last_read_timestamp = timestamp;
        }
      }

      this.state.total_updates++;
      this.state.last_updated = host.nowMs();

      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;

      return {
        status: "ok",
        user_id: this.state.user_id,
        channel_id: channelId,
        message_id: messageId,
        timestamp: readState.last_read_timestamp,
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates,
        compute_ms: computeMs,
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e),
      };
    }
  }

  /**
   * Get read state for a specific channel (Orbit: getReadState)
   */
  onGet_read_state(payload: Record<string, unknown>): Record<string, unknown> {
    const startMs = host.nowMs();
    try {
      const channelId = String(payload.channel_id ?? "");

      if (!channelId) {
        return {
          status: "error",
          error: "channel_id required",
        };
      }

      const readState = this.state.channels[channelId];
      if (!readState) {
        return {
          status: "ok",
          user_id: this.state.user_id,
          channel_id: channelId,
          read_state: null,
        };
      }

      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;

      return {
        status: "ok",
        user_id: this.state.user_id,
        channel_id: channelId,
        read_state: {
          last_read_message_id: readState.last_read_message_id,
          last_read_timestamp: readState.last_read_timestamp,
        },
        compute_ms: computeMs,
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e),
      };
    }
  }

  /**
   * Get all read states for this user (Orbit: getAllReadStates)
   */
  onGet_all_read_states(_payload: Record<string, unknown>): Record<string, unknown> {
    const startMs = host.nowMs();
    try {
      const channels: Record<string, { last_read_message_id: string; last_read_timestamp: number }> = {};
      for (const [channelId, readState] of Object.entries(this.state.channels)) {
        channels[channelId] = {
          last_read_message_id: readState.last_read_message_id,
          last_read_timestamp: readState.last_read_timestamp,
        };
      }

      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;

      return {
        status: "ok",
        user_id: this.state.user_id,
        channels: channels,
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates,
        last_updated: this.state.last_updated,
        compute_ms: computeMs,
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e),
      };
    }
  }

  /**
   * Batch update read states (for performance testing)
   * Updates multiple channels in a single call
   */
  onBatch_mark_read(payload: Record<string, unknown>): Record<string, unknown> {
    const startMs = host.nowMs();
    try {
      const updates = payload.updates as Array<{ channel_id: string; message_id: string; timestamp?: number }> | undefined;
      if (!updates || !Array.isArray(updates)) {
        return {
          status: "error",
          error: "updates array required",
        };
      }

      const now = host.nowMs();
      let updated = 0;
      let created = 0;

      for (const update of updates) {
        const channelId = String(update.channel_id ?? "");
        const messageId = String(update.message_id ?? "");
        const timestamp = Number(update.timestamp ?? now);

        if (!channelId || !messageId) {
          continue;
        }

        let readState = this.state.channels[channelId];
        if (!readState) {
          readState = {
            channel_id: channelId,
            last_read_message_id: messageId,
            last_read_timestamp: timestamp,
          };
          this.state.channels[channelId] = readState;
          created++;
        } else {
          if (timestamp >= readState.last_read_timestamp) {
            readState.last_read_message_id = messageId;
            readState.last_read_timestamp = timestamp;
            updated++;
          }
        }
      }

      this.state.total_channels = Object.keys(this.state.channels).length;
      this.state.total_updates += updates.length;
      this.state.last_updated = host.nowMs();

      const computeMs = host.nowMs() - startMs;
      this.state.total_compute_ms += computeMs;

      return {
        status: "ok",
        user_id: this.state.user_id,
        total_updates: updates.length,
        channels_updated: updated,
        channels_created: created,
        total_channels: this.state.total_channels,
        compute_ms: computeMs,
        ops_per_sec: updates.length / (computeMs / 1000),
      };
    } catch (e) {
      return {
        status: "error",
        error: String(e),
      };
    }
  }

  /**
   * Get statistics (for metrics and testing)
   */
  onStats(_payload: Record<string, unknown>): Record<string, unknown> {
    return {
      status: "ok",
      user_id: this.state.user_id,
      total_channels: this.state.total_channels,
      total_updates: this.state.total_updates,
      last_updated: this.state.last_updated,
      total_compute_ms: this.state.total_compute_ms,
      counters: {
        total_channels: this.state.total_channels,
        total_updates: this.state.total_updates,
      },
      benchmarks: {
        total_compute_ms: this.state.total_compute_ms,
        ops_per_sec: this.state.total_updates > 0 && this.state.total_compute_ms > 0
          ? (this.state.total_updates / (this.state.total_compute_ms / 1000))
          : 0,
      },
    };
  }
}

// ========================================================================
// Main - Export actor for WASM
// ========================================================================

const actorInstance = new ReadStateTrackerActor();

export const actor = {
  init: (configJson: string) => actorInstance.init(configJson),
  handle: (from: string, msgType: string, payloadJson: string) =>
    actorInstance.handle(from, msgType, payloadJson),
  getState: () => actorInstance.getState(),
  setState: (stateJson: string) => actorInstance.setState(stateJson),
};
