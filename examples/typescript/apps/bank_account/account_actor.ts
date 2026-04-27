// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Bank Account Actor - Named Virtual Actor Example (TypeScript WASM)
//
// Uses @plexspaces/sdk (inheritance-based). Same API as Python bank_account:
// balance, deposit, withdraw, history, replay. Real-world: banking, wallets, ledgers.

import { ActorRouter, PlexSpacesActor } from "@plexspaces/sdk";

interface Transaction {
  type: string;
  amount: number;
  balance_after: number;
}

interface BankAccountState extends Record<string, unknown> {
  actor_id: string;
  balance: number;
  transactions: Transaction[];
}

class BankAccountActor extends PlexSpacesActor<BankAccountState> {
  getDefaultState(): BankAccountState {
    return { actor_id: "", balance: 0, transactions: [] };
  }

  protected override onInit(config: Record<string, unknown>): void {
    if (typeof config.actor_id === "string" && config.actor_id) {
      this.state.actor_id = config.actor_id;
    }
    this.state.balance = 0;
    this.state.transactions = [];
  }

  protected onBalance(): Record<string, unknown> {
    return { success: true, account: this.state.actor_id, balance: this.state.balance };
  }

  /** Alias for balance. */
  protected onGet(): Record<string, unknown> {
    return this.onBalance();
  }

  protected onDeposit(payload: Record<string, unknown>): Record<string, unknown> {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { success: false, error: "invalid_amount" };
    this.state.balance += amount;
    this.state.transactions.push({ type: "deposit", amount, balance_after: this.state.balance });
    return { success: true, balance: this.state.balance };
  }

  protected onWithdraw(payload: Record<string, unknown>): Record<string, unknown> {
    const amount = Number(payload.amount ?? 0);
    if (amount <= 0) return { success: false, error: "invalid_amount" };
    if (amount > this.state.balance) {
      return { success: false, error: "insufficient_funds", balance: this.state.balance };
    }
    this.state.balance -= amount;
    this.state.transactions.push({ type: "withdraw", amount, balance_after: this.state.balance });
    return { success: true, balance: this.state.balance };
  }

  protected onTx_count(): Record<string, unknown> {
    return { success: true, count: this.state.transactions.length };
  }

  protected onHistory(payload: Record<string, unknown>): Record<string, unknown> {
    const count = Math.min(Number(payload.count ?? 5), this.state.transactions.length);
    const recent = count > 0 ? this.state.transactions.slice(-count) : [];
    return { success: true, transactions: recent };
  }

  protected onReplay(): Record<string, unknown> {
    let rebuilt = 0;
    for (const tx of this.state.transactions) {
      if (tx.type === "deposit") rebuilt += tx.amount;
      else if (tx.type === "withdraw") rebuilt -= tx.amount;
    }
    return {
      success: true,
      replayed: this.state.transactions.length,
      rebuilt_balance: rebuilt,
      current_balance: this.state.balance,
    };
  }
}

const router = new ActorRouter({
  bank_account_wasm: () => new BankAccountActor(),
});

export const actor = {
  init: (configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.init(configJson),
  handle: (
    from: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView
  ) => router.handle(from, msgType, payloadJson),
  getState: () => router.getState(),
  setState: (stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView) => router.setState(stateJson),
};
