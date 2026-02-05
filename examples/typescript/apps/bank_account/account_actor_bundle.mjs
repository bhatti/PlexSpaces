// ../../../../sdks/typescript/dist/actor.js
var PlexSpacesActor = class {
  constructor() {
    this.state = this.getDefaultState();
  }
  /** Optional: called from init() with parsed config. Override to apply config to state. */
  onInit(_config) {
  }
  /** WIT init(config-json) -> string. Empty string = success, "ERROR:..." = failure. */
  init(configJson) {
    try {
      const config = configJson && configJson.trim() ? JSON.parse(configJson) : {};
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
  handle(_fromActor, _msgType, payloadJson) {
    try {
      const payload = payloadJson && payloadJson.trim() ? JSON.parse(payloadJson) : {};
      const op = payload.op ?? payload;
      const opKey = typeof op === "string" ? this.capitalize(op) : "";
      const methodName = opKey ? `on${opKey}` : "";
      const method = methodName && typeof this[methodName] === "function" ? this[methodName] : null;
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
  getState() {
    return this.json(this.state);
  }
  /** WIT set-state(state-json) -> string. Empty = success, "ERROR:..." = failure. */
  setState(stateJson) {
    try {
      if (stateJson && stateJson.trim()) {
        const parsed = JSON.parse(stateJson);
        this.state = parsed;
      }
      return "";
    } catch {
      return "ERROR:set_state failed";
    }
  }
  capitalize(s) {
    if (!s)
      return "";
    return s.charAt(0).toUpperCase() + s.slice(1).toLowerCase();
  }
  json(obj) {
    return JSON.stringify(obj);
  }
  error(message) {
    return "ERROR:" + message;
  }
};

// account_actor.ts
var BankAccountActor = class extends PlexSpacesActor {
  getDefaultState() {
    return { account_id: "", balance: 0, transactions: [] };
  }
  onInit(config) {
    this.state.account_id = String(config.account_id ?? "");
    this.state.balance = 0;
    this.state.transactions = [];
  }
  onBalance() {
    return { account: this.state.account_id, balance: this.state.balance };
  }
  /** Alias for balance (same as Python handler("balance", "get")). */
  onGet() {
    return this.onBalance();
  }
  onDeposit(payload) {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { error: "invalid_amount" };
    this.state.balance += amount;
    this.state.transactions.push({
      type: "deposit",
      amount,
      balance_after: this.state.balance
    });
    return { status: "ok", balance: this.state.balance };
  }
  onWithdraw(payload) {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { error: "invalid_amount" };
    if (amount > this.state.balance) {
      return { error: "insufficient_funds", balance: this.state.balance };
    }
    this.state.balance -= amount;
    this.state.transactions.push({
      type: "withdraw",
      amount,
      balance_after: this.state.balance
    });
    return { status: "ok", balance: this.state.balance };
  }
  onTx_count() {
    return { count: this.state.transactions.length };
  }
  onHistory(payload) {
    const count = Math.min(
      Number(payload.count ?? 5),
      this.state.transactions.length
    );
    const recent = count > 0 ? this.state.transactions.slice(-count) : [];
    return { transactions: recent };
  }
  onReplay() {
    let rebuilt = 0;
    for (const tx of this.state.transactions) {
      if (tx.type === "deposit") rebuilt += tx.amount;
      else if (tx.type === "withdraw") rebuilt -= tx.amount;
    }
    return {
      replayed: this.state.transactions.length,
      rebuilt_balance: rebuilt,
      current_balance: this.state.balance
    };
  }
  onSet_account(payload) {
    this.state.account_id = String(payload.account_id ?? "");
    return { status: "ok" };
  }
};
var instance = new BankAccountActor();
var actor = {
  init: (configJson) => instance.init(configJson),
  handle: (from, msgType, payloadJson) => instance.handle(from, msgType, payloadJson),
  getState: () => instance.getState(),
  setState: (stateJson) => instance.setState(stateJson)
};
export {
  BankAccountActor,
  actor
};
