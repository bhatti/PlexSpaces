import { WorkflowActor } from "@plexspaces/sdk";
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
/**
 * Payment actor with idempotency and durability (Restate-style).
 * Duplicate requests with same idempotency_key return cached result; steps are journaled via checkpoint.
 */
export declare class PaymentActor extends WorkflowActor<PaymentActorState> {
    getDefaultState(): PaymentActorState;
    protected onInit(_config: Record<string, unknown>): void;
    run(payload: Record<string, unknown>): Record<string, unknown>;
    signal(_name: string, _data: Record<string, unknown>): void;
    query(name: string, _params: Record<string, unknown>): Record<string, unknown>;
    private finish;
}
export declare const actor: {
    init: (configJson: string) => void;
    handle: (from: string, msgType: string, payloadJson: string) => Uint8Array<ArrayBufferLike>;
    getState: () => Uint8Array<ArrayBufferLike>;
    setState: (stateJson: string) => void;
};
export {};
//# sourceMappingURL=payment_actor.d.ts.map