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
    constructor();
    /** Return initial state. Override in subclass. */
    abstract getDefaultState(): TState;
    /** Optional: called from init() with parsed config. Override to apply config to state. */
    protected onInit(_config: Record<string, unknown>): void;
    /** WIT init(config-json) -> string. Empty string = success, "ERROR:..." = failure. */
    init(configJson: string): string;
    /**
     * WIT handle(from-actor, msg-type, payload-json) -> string.
     * Dispatches by payload.op (or payload) to on<Op>(payload). Returns JSON or "ERROR:...".
     */
    handle(_fromActor: string, _msgType: string, payloadJson: string): string;
    /** WIT get-state() -> string. Returns JSON-serialized state. */
    getState(): string;
    /** WIT set-state(state-json) -> string. Empty = success, "ERROR:..." = failure. */
    setState(stateJson: string): string;
    protected capitalize(s: string): string;
    protected json(obj: unknown): string;
    protected error(message: string): string;
}
