import { WorkflowActor } from "@plexspaces/sdk";
type DocStatus = "pending" | "ocr_done" | "classified" | "extracted" | "stored" | "cancelled" | "failed";
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
    ocr_results: {
        page: number;
        text_len: number;
    }[];
    cancel_requested: boolean;
    total_compute_ms: number;
    total_coord_ms: number;
    created_at_ms: number;
    updated_at_ms: number;
    /** Total retries used across steps (for metrics). */
    total_retry_count: number;
}
/**
 * Document Processing Workflow - Azure Durable Functions style.
 * Steps: OCR (fan-out N pages) → classify → extract → store.
 */
export declare class DocumentProcessingActor extends WorkflowActor<DocumentProcessingState> {
    getDefaultState(): DocumentProcessingState;
    protected onInit(config: Record<string, unknown>): void;
    run(payload: Record<string, unknown>): Record<string, unknown>;
    signal(name: string, _data: Record<string, unknown>): void;
    query(name: string, _params: Record<string, unknown>): Record<string, unknown>;
    /** Catch path: step failed after retries exhausted (Step Functions Catch / Durable Functions). */
    private finishFailed;
    private finishCancelled;
}
export declare const actor: {
    init: (configJson: string) => void;
    handle: (from: string, msgType: string, payloadJson: string) => Uint8Array<ArrayBufferLike>;
    getState: () => Uint8Array<ArrayBufferLike>;
    setState: (stateJson: string) => void;
};
export {};
//# sourceMappingURL=document_processing_actor.d.ts.map