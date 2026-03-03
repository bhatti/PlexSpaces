/**
 * Base class for PlexSpaces actors (simple-actor WIT: init, handle, get-state, set-state).
 *
 * Subclass and override:
 * - getDefaultState(): initial state
 * - onInit(config): optional, called from init() with parsed config
 * - on<Op>(payload): handler for message op (e.g. onDeposit, onBalance).
 *   Op is derived from payload.op or payload; method name is "on" + capitalized op.
 *
 * Example:
 *   class BankAccountActor extends PlexSpacesActor<BankAccountState> {
 *     getDefaultState() { return { account_id: '', balance: 0, transactions: [] }; }
 *     onDeposit(p: { amount: number }) { ...; return { status: 'ok', balance: this.state.balance }; }
 *     onBalance() { return { account: this.state.account_id, balance: this.state.balance }; }
 *   }
 */
export declare abstract class PlexSpacesActor<TState extends object = Record<string, unknown>> {
    protected state: TState;
    private cachedStateJson;
    constructor();
    /** Return initial state. Override in subclass. */
    abstract getDefaultState(): TState;
    /** Optional: called from init() with parsed config. Override to apply config to state. */
    protected onInit(_config: Record<string, unknown>): void;
    /** WIT init(config-json) -> string. Empty string = success, "ERROR:..." = failure. */
    init(configJson: string): string;
    /**
     * WIT handle(from-actor, msg-type, payload-json) -> result<string, string>.
     * Dispatches by msgType for Workflow behavior (workflow_run, workflow_signal:name, workflow_query:name),
     * then by payload.op (or payload) to on<Op>(payload). Returns JSON string.
     * Uses iterative serializer to avoid WASM recursion.
     *
     * Workflow behavior (aligned with Rust Workflow trait and Python @workflow_actor):
     * - msgType "workflow_run" -> run(payload)
     * - msgType "workflow_signal:name" -> signal(name, payload)
     * - msgType "workflow_query:name" -> query(name, payload)
     */
    handle(_fromActor: string, msgType: string, payloadJson: string): string;
    /** WIT get-state() -> string. Returns JSON-serialized state. */
    getState(): string;
    /** WIT set-state(state-json) -> string. Empty = success, "ERROR:..." = failure. */
    setState(stateJson: string): string;
    protected capitalize(s: string): string;
    /**
     * Serialize object to JSON string using fully iterative approach (zero recursion).
     *
     * jco componentize compiles JS to WASM (StarlingMonkey) with a tiny call stack.
     * Native JSON.stringify recurses per-element and per-nesting-level, hitting
     * stack limits with arrays of 2+ items. This iterative serializer uses a work
     * stack instead of recursive function calls.
     */
    protected json(obj: unknown): string;
    protected error(message: string): string;
}
/**
 * Base class for Workflow behavior actors (aligned with Rust Workflow trait and Python @workflow_actor).
 *
 * Implement run(), signal(), and query() for durable workflows (Temporal/Restate-style).
 * Message routing: framework sends msgType "workflow_run" | "workflow_signal:name" | "workflow_query:name";
 * PlexSpacesActor.handle() dispatches to these methods when present.
 *
 * Example:
 *   class OrderFulfillmentActor extends WorkflowActor<OrderState> {
 *     getDefaultState() { return { orderId: '', status: 'pending', steps: [] }; }
 *     run(payload: Record<string, unknown>) { ...; return { status: 'completed' }; }
 *     signal(name: string, data: Record<string, unknown>) { if (name === 'cancel') this.state.status = 'cancelled'; }
 *     query(name: string, params: Record<string, unknown>) { if (name === 'status') return this.state; return {}; }
 *   }
 */
export declare abstract class WorkflowActor<TState extends object = Record<string, unknown>> extends PlexSpacesActor<TState> {
    /** Main workflow execution (exclusive). Called when msgType is "workflow_run". Return result for ask/call. */
    abstract run(payload: Record<string, unknown>): Record<string, unknown> | unknown;
    /** Handle external signal (e.g. cancel). Called when msgType is "workflow_signal:name". */
    abstract signal(name: string, data: Record<string, unknown>): void;
    /** Read-only query. Called when msgType is "workflow_query:name". Return result for ask/call. */
    abstract query(name: string, params: Record<string, unknown>): Record<string, unknown> | unknown;
}
