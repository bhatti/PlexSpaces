// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// V8 Isolates → PlexSpaces: High-throughput log processor (TypeScript WASM)
//
// Real-world use case: Process batches of log lines (parse level/source, route by level).
// One actor handles batch requests; simulates V8 isolate-style per-batch processing.
//
// Native: V8 isolates for multi-tenant log processing with isolated heaps.
// PlexSpaces: GenServer-style actor with process_batch (batch of lines) and status query.
import { PlexSpacesActor, host } from "@plexspaces/sdk";
const LEVELS = ["DEBUG", "INFO", "WARN", "ERROR"];
const PARSE_MS_PER_LINE = 0.05;
// ========================================================================
// Log Processor Actor (GenServer-style)
// ========================================================================
/**
 * High-throughput log processor - V8 isolate-style batching.
 * process_batch: accept array of log lines, parse level, aggregate counts.
 * status: return throughput and level breakdown.
 */
export class LogProcessorActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            processor_id: "",
            processed_count: 0,
            batches_received: 0,
            total_bytes: 0,
            by_level: { DEBUG: 0, INFO: 0, WARN: 0, ERROR: 0 },
            start_ms: 0,
            total_compute_ms: 0,
            total_coord_ms: 0,
        };
    }
    onInit(config) {
        this.state.processor_id = String(config.processor_id ?? this.state.processor_id);
        this.state.start_ms = host.nowMs();
    }
    /** Process a batch of log lines; parse level, aggregate. */
    onProcess_batch(payload) {
        const t0 = host.nowMs();
        const lines = payload.lines ?? payload.batch ?? [];
        let bytes = 0;
        const levelCounts = { DEBUG: 0, INFO: 0, WARN: 0, ERROR: 0 };
        for (const line of lines) {
            const s = String(line);
            bytes += s.length;
            const level = this.parseLevel(s);
            levelCounts[level] = (levelCounts[level] ?? 0) + 1;
        }
        const computeMs = lines.length * PARSE_MS_PER_LINE;
        const elapsed = host.nowMs() - t0;
        const coordMs = Math.max(0, elapsed - computeMs);
        this.state.processed_count += lines.length;
        this.state.batches_received += 1;
        this.state.total_bytes += bytes;
        this.state.total_compute_ms += computeMs;
        this.state.total_coord_ms += coordMs;
        for (const k of LEVELS) {
            this.state.by_level[k] = (this.state.by_level[k] ?? 0) + (levelCounts[k] ?? 0);
        }
        return {
            ok: true,
            lines: lines.length,
            bytes,
            by_level: levelCounts,
            processed_count: this.state.processed_count,
            batches_received: this.state.batches_received,
            total_compute_ms: this.state.total_compute_ms,
            total_coord_ms: this.state.total_coord_ms,
        };
    }
    /** Return throughput and level breakdown. */
    onStatus(_payload) {
        const elapsed = host.nowMs() - this.state.start_ms;
        const elapsedSec = elapsed / 1000;
        const eventsPerSec = elapsedSec > 0 ? this.state.processed_count / elapsedSec : 0;
        return {
            processor_id: this.state.processor_id,
            processed_count: this.state.processed_count,
            batches_received: this.state.batches_received,
            total_bytes: this.state.total_bytes,
            by_level: { ...this.state.by_level },
            total_compute_ms: this.state.total_compute_ms,
            total_coord_ms: this.state.total_coord_ms,
            elapsed_ms: elapsed,
            events_per_sec: Math.round(eventsPerSec * 10) / 10,
        };
    }
    parseLevel(line) {
        const upper = line.toUpperCase();
        for (const level of LEVELS) {
            if (upper.startsWith(level) || upper.includes("\t" + level) || upper.includes(" " + level)) {
                return level;
            }
        }
        return "INFO";
    }
}
const actorInstance = new LogProcessorActor();
export const actor = {
    init: (configJson) => actorInstance.init(configJson),
    handle: (from, msgType, payloadJson) => actorInstance.handle(from, msgType, payloadJson),
    getState: () => actorInstance.getState(),
    setState: (stateJson) => actorInstance.setState(stateJson),
};
//# sourceMappingURL=log_processor_actor.js.map