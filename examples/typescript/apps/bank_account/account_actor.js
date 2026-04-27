// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Bank Account Actor - Named Virtual Actor Example (TypeScript WASM)
//
// Uses @plexspaces/sdk (inheritance-based). Same API as Python bank_account:
// balance, deposit, withdraw, history, replay. Real-world: banking, wallets, ledgers.
import { ActorRouter, PlexSpacesActor } from "@plexspaces/sdk";
class BankAccountActor extends PlexSpacesActor {
    getDefaultState() {
        return { actor_id: "", balance: 0, transactions: [] };
    }
    onInit(config) {
        if (typeof config.actor_id === "string" && config.actor_id) {
            this.state.actor_id = config.actor_id;
        }
        this.state.balance = 0;
        this.state.transactions = [];
    }
    onBalance() {
        return { success: true, account: this.state.actor_id, balance: this.state.balance };
    }
    /** Alias for balance. */
    onGet() {
        return this.onBalance();
    }
    onDeposit(payload) {
        const amount = Number(payload.amount ?? 0);
        if (amount <= 0)
            return { success: false, error: "invalid_amount" };
        this.state.balance += amount;
        this.state.transactions.push({ type: "deposit", amount, balance_after: this.state.balance });
        return { success: true, balance: this.state.balance };
    }
    onWithdraw(payload) {
        const amount = Number(payload.amount ?? 0);
        if (amount <= 0)
            return { success: false, error: "invalid_amount" };
        if (amount > this.state.balance) {
            return { success: false, error: "insufficient_funds", balance: this.state.balance };
        }
        this.state.balance -= amount;
        this.state.transactions.push({ type: "withdraw", amount, balance_after: this.state.balance });
        return { success: true, balance: this.state.balance };
    }
    onTx_count() {
        return { success: true, count: this.state.transactions.length };
    }
    onHistory(payload) {
        const count = Math.min(Number(payload.count ?? 5), this.state.transactions.length);
        const recent = count > 0 ? this.state.transactions.slice(-count) : [];
        return { success: true, transactions: recent };
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
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
