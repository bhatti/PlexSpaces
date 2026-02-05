// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Bank Account Actor - Durable State Example (TypeScript WASM)
//
// Uses @plexspaces/sdk (inheritance-based). Same API as Python bank_account:
// balance, deposit, withdraw, history, replay. Real-world: banking, wallets, ledgers.
import { PlexSpacesActor } from "@plexspaces/sdk";
export class BankAccountActor extends PlexSpacesActor {
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
        if (amount <= 0)
            return { error: "invalid_amount" };
        this.state.balance += amount;
        this.state.transactions.push({
            type: "deposit",
            amount,
            balance_after: this.state.balance,
        });
        return { status: "ok", balance: this.state.balance };
    }
    onWithdraw(payload) {
        const amount = Number(payload.amount ?? 0);
        if (amount <= 0)
            return { error: "invalid_amount" };
        if (amount > this.state.balance) {
            return { error: "insufficient_funds", balance: this.state.balance };
        }
        this.state.balance -= amount;
        this.state.transactions.push({
            type: "withdraw",
            amount,
            balance_after: this.state.balance,
        });
        return { status: "ok", balance: this.state.balance };
    }
    onTx_count() {
        return { count: this.state.transactions.length };
    }
    onHistory(payload) {
        const count = Math.min(Number(payload.count ?? 5), this.state.transactions.length);
        const recent = count > 0 ? this.state.transactions.slice(-count) : [];
        return { transactions: recent };
    }
    onReplay() {
        let rebuilt = 0;
        for (const tx of this.state.transactions) {
            if (tx.type === "deposit")
                rebuilt += tx.amount;
            else if (tx.type === "withdraw")
                rebuilt -= tx.amount;
        }
        return {
            replayed: this.state.transactions.length,
            rebuilt_balance: rebuilt,
            current_balance: this.state.balance,
        };
    }
    onSet_account(payload) {
        this.state.account_id = String(payload.account_id ?? "");
        return { status: "ok" };
    }
}
// WIT actor export (used by component entry and verify)
const instance = new BankAccountActor();
export const actor = {
    init: (configJson) => instance.init(configJson),
    handle: (from, msgType, payloadJson) => instance.handle(from, msgType, payloadJson),
    getState: () => instance.getState(),
    setState: (stateJson) => instance.setState(stateJson),
};
