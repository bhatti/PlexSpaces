import { WorkflowActor } from "@plexspaces/sdk";
interface EnsembleState {
    ensemble_id: string;
    num_tasks: number;
    num_completed: number;
    status: string;
    total_compute_ms: number;
    total_coord_ms: number;
    created_at_ms: number;
    updated_at_ms: number;
    cancel_requested: boolean;
    worker_joined: boolean;
}
export declare class EnsembleActor extends WorkflowActor<EnsembleState> {
    getDefaultState(): EnsembleState;
    run(payload: Record<string, unknown>): Record<string, unknown>;
    private finish;
    signal(name: string, _data: Record<string, unknown>): void;
    query(name: string, _params: Record<string, unknown>): Record<string, unknown>;
    /** Worker: called when process group broadcast "tasks_ready" is received (msgType used for routing). */
    onTasks_ready(payload: Record<string, unknown>): Record<string, unknown>;
}
export declare const actor: {
    init: (configJson: string) => void;
    handle: (from: string, msgType: string, payloadJson: string) => Uint8Array<ArrayBufferLike>;
    getState: () => Uint8Array<ArrayBufferLike>;
    setState: (stateJson: string) => void;
};
export {};
