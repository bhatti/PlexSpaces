// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces Actor base class (TypeScript SDK)
//
// Use inheritance to define actors with minimal boilerplate. Override
// getDefaultState() and onOpName(payload) methods; the base class handles
// init/handle/getState/setState and dispatch by payload.op.

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
export abstract class PlexSpacesActor<TState extends object = Record<string, unknown>> {
  protected state: TState;

  constructor() {
    this.state = this.getDefaultState();
  }

  /** Return initial state. Override in subclass. */
  abstract getDefaultState(): TState;

  /** Optional: called from init() with parsed config. Override to apply config to state. */
  protected onInit(_config: Record<string, unknown>): void {
    // default: no-op; subclass can set state from config
  }

  /** WIT init(config-json) -> string. Empty string = success, "ERROR:..." = failure. */
  init(configJson: string): string {
    try {
      const config = configJson && configJson.trim() ? (JSON.parse(configJson) as Record<string, unknown>) : {};
      this.onInit(config);
      return "";
    } catch {
      return "ERROR:init failed";
    }
  }

  /**
   * WIT handle(from-actor, msg-type, payload-json) -> string.
   * Dispatches by payload.op (or payload) to on<Op>(payload). Returns JSON or "ERROR:...".
   */
  handle(_fromActor: string, _msgType: string, payloadJson: string): string {
    try {
      const payload = payloadJson && payloadJson.trim() ? (JSON.parse(payloadJson) as Record<string, unknown>) : {};
      const op = (payload.op ?? payload) as string;
      const opKey = typeof op === "string" ? this.capitalize(op) : "";
      const methodName = opKey ? `on${opKey}` : "";
      const method = methodName && typeof (this as unknown as Record<string, unknown>)[methodName] === "function"
        ? (this as unknown as Record<string, (p: Record<string, unknown>) => unknown>)[methodName]
        : null;
      if (method) {
        const result = method.call(this, payload);
        return this.json(result);
      }
      return this.json({ error: "unknown_op", op: String(op) });
    } catch (e) {
      return this.error(e instanceof Error ? e.message : String(e));
    }
  }

  /** WIT get-state() -> string. Returns JSON-serialized state. */
  getState(): string {
    return this.json(this.state);
  }

  /** WIT set-state(state-json) -> string. Empty = success, "ERROR:..." = failure. */
  setState(stateJson: string): string {
    try {
      if (stateJson && stateJson.trim()) {
        const parsed = JSON.parse(stateJson) as TState;
        this.state = parsed;
      }
      return "";
    } catch {
      return "ERROR:set_state failed";
    }
  }

  protected capitalize(s: string): string {
    if (!s) return "";
    return s.charAt(0).toUpperCase() + s.slice(1).toLowerCase();
  }

  protected json(obj: unknown): string {
    return JSON.stringify(obj);
  }

  protected error(message: string): string {
    return "ERROR:" + message;
  }
}
