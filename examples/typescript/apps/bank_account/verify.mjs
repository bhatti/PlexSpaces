#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// In-process verification of TypeScript bank_account logic (same API as Python).
// Run from this directory: node verify.mjs   OR   ./test.sh
// account_actor.js must exist (checked in, or run: npm run build).

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const actorPath = join(__dirname, "account_actor.js");

if (!existsSync(actorPath)) {
  console.error("FAIL: account_actor.js not found at", actorPath);
  console.error("  Run from this directory: npm run build   or   ./build.sh");
  process.exit(1);
}

const m = await import(actorPath);
const actor = m.actor ?? m.default ?? m;
if (!actor || typeof actor.init !== "function") {
  console.error("FAIL: account_actor.js must export { init, handle, getState, setState } or { actor }");
  process.exit(1);
}

function assert(cond, msg) {
  if (!cond) {
    console.error("FAIL:", msg);
    process.exit(1);
  }
}

function parse(r) {
  return typeof r === "string" ? JSON.parse(r) : (r instanceof Uint8Array ? JSON.parse(new TextDecoder().decode(r)) : r);
}

console.log("Bank Account (TypeScript) – in-process verification");
console.log("");

// Init with actor_type so ActorRouter can dispatch
const initConfig = JSON.stringify({ actor_id: "account-alice//bank_account_wasm::test@node", actor_type: "bank_account_wasm" });
assert(actor.init(initConfig) === "", "init returns empty string");

// Balance initially 0
let r = parse(actor.handle("", "call", '{"op":"balance"}'));
assert(r.success === true && r.balance === 0, "initial balance: " + JSON.stringify(r));

// Deposit
r = parse(actor.handle("", "call", '{"op":"deposit","amount":1000}'));
assert(r.success === true && r.balance === 1000, "deposit 1000: " + JSON.stringify(r));

// Withdraw
r = parse(actor.handle("", "call", '{"op":"withdraw","amount":200}'));
assert(r.success === true && r.balance === 800, "withdraw 200: " + JSON.stringify(r));

// Balance
r = parse(actor.handle("", "call", '{"op":"balance"}'));
assert(r.success === true && r.balance === 800, "balance 800: " + JSON.stringify(r));

// Invalid deposit
r = parse(actor.handle("", "call", '{"op":"deposit","amount":0}'));
assert(r.success === false && r.error === "invalid_amount", "invalid_amount: " + JSON.stringify(r));

// Insufficient funds
r = parse(actor.handle("", "call", '{"op":"withdraw","amount":10000}'));
assert(r.success === false && r.error === "insufficient_funds", "insufficient_funds: " + JSON.stringify(r));

// History
r = parse(actor.handle("", "call", '{"op":"history","count":5}'));
assert(r.success === true && Array.isArray(r.transactions) && r.transactions.length === 2, "history length: " + JSON.stringify(r));

// Replay
r = parse(actor.handle("", "call", '{"op":"replay"}'));
assert(r.success === true && r.replayed === 2 && r.rebuilt_balance === 800 && r.current_balance === 800, "replay: " + JSON.stringify(r));

// getState / setState (durability)
const stateJson = actor.getState();
actor.init(initConfig);
actor.setState(stateJson);
r = parse(actor.handle("", "call", '{"op":"balance"}'));
assert(r.success === true && r.balance === 800, "state restore: " + JSON.stringify(r));

// tx_count
r = parse(actor.handle("", "call", '{"op":"tx_count"}'));
assert(r.success === true && r.count === 2, "tx_count: " + JSON.stringify(r));

console.log("OK All assertions passed.");
console.log("");
