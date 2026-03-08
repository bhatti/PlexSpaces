import { PlexSpacesActor } from "@plexspaces/sdk";
interface LogProcessorState {
    processor_id: string;
    processed_count: number;
    batches_received: number;
    total_bytes: number;
    by_level: Record<string, number>;
    start_ms: number;
    total_compute_ms: number;
    total_coord_ms: number;
}
/**
 * High-throughput log processor - V8 isolate-style batching.
 * process_batch: accept array of log lines, parse level, aggregate counts.
 * status: return throughput and level breakdown.
 */
export declare class LogProcessorActor extends PlexSpacesActor<LogProcessorState> {
    getDefaultState(): LogProcessorState;
    protected onInit(config: Record<string, unknown>): void;
    /** Process a batch of log lines; parse level, aggregate. */
    onProcess_batch(payload: Record<string, unknown>): Record<string, unknown>;
    /** Return throughput and level breakdown. */
    onStatus(_payload: Record<string, unknown>): Record<string, unknown>;
    private parseLevel;
}
export declare const actor: {
    init: (configJson: string) => string;
    handle: (from: string, msgType: string, payloadJson: string) => string;
    getState: () => string;
    setState: (stateJson: string) => string;
};
export {};
//# sourceMappingURL=log_processor_actor.d.ts.map