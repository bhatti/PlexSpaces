import { WorkflowActor } from "@plexspaces/sdk";
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
/**
 * Windowed stream aggregator - Dirigo-style real-time analytics.
 * run(): process batch of events, push to window, emit aggregate when window full.
 * onIngest: add single event. onWindow_flush: emit aggregate for current window.
 */
export declare class WindowedStreamActor extends WorkflowActor<WindowedStreamState> {
    getDefaultState(): WindowedStreamState;
    protected onInit(config: Record<string, unknown>): void;
    /** Main workflow: process event batch, window aggregation, return metrics. */
    run(payload: Record<string, unknown>): Record<string, unknown>;
    signal(name: string, _data: Record<string, unknown>): void;
    query(name: string, _params: Record<string, unknown>): Record<string, unknown>;
    /** Add single event (handler). */
    onIngest(payload: Record<string, unknown>): Record<string, unknown>;
    /** Emit aggregate for current window and clear (handler). */
    onWindow_flush(_payload: Record<string, unknown>): Record<string, unknown>;
    private emitWindow;
    private finish;
}
export declare const actor: {
    init: (configJson: string) => void;
    handle: (from: string, msgType: string, payloadJson: string) => Uint8Array<ArrayBufferLike>;
    getState: () => Uint8Array<ArrayBufferLike>;
    setState: (stateJson: string) => void;
};
export {};
//# sourceMappingURL=stream_actor.d.ts.map