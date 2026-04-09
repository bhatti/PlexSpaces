import { WorkflowActor } from "@plexspaces/sdk";
type OrderStatus = "pending" | "validated" | "inventory_reserved" | "payment_charged" | "shipped" | "cancelled" | "failed";
interface OrderStep {
    name: string;
    completed_at_ms: number;
    payload?: Record<string, unknown>;
}
interface OrderFulfillmentState {
    order_id: string;
    customer_id: string;
    status: OrderStatus;
    steps: OrderStep[];
    cancel_requested: boolean;
    total_compute_ms: number;
    total_coord_ms: number;
    created_at_ms: number;
    updated_at_ms: number;
}
/**
 * Order Fulfillment Workflow - Temporal-style saga with run/signal/query.
 *
 * Demonstrates:
 * - Workflow behavior: run() = main execution, signal() = cancel, query() = status
 * - Saga steps: validate → reserve inventory → charge payment → ship (with compensation on failure/cancel)
 * - Durability: State checkpointed via getState/setState
 *
 * Message types (from framework): workflow_run, workflow_signal:cancel, workflow_query:status
 */
export declare class OrderFulfillmentActor extends WorkflowActor<OrderFulfillmentState> {
    getDefaultState(): OrderFulfillmentState;
    protected onInit(config: Record<string, unknown>): void;
    /** Main workflow execution (exclusive). Called when msgType is "workflow_run". */
    run(payload: Record<string, unknown>): Record<string, unknown>;
    /** Handle external signal (e.g. cancel). Called when msgType is "workflow_signal:name". */
    signal(name: string, _data: Record<string, unknown>): void;
    /** Read-only query. Called when msgType is "workflow_query:name". */
    query(name: string, _params: Record<string, unknown>): Record<string, unknown>;
    private compensate;
}
export declare const actor: {
    init: (configJson: string) => void;
    handle: (from: string, msgType: string, payloadJson: string) => Uint8Array<ArrayBufferLike>;
    getState: () => Uint8Array<ArrayBufferLike>;
    setState: (stateJson: string) => void;
};
export {};
//# sourceMappingURL=order_fulfillment_actor.d.ts.map