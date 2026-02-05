#!/usr/bin/env node
// SPDX-License-Identifier: LGPL-2.1-or-later
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

function assertJsonEq(got, expected, msg) {
  const g = typeof got === "string" ? JSON.parse(got) : got;
  const e = typeof expected === "string" ? JSON.parse(expected) : expected;
  const ok = JSON.stringify(g) === JSON.stringify(e);
  if (!ok) {
    console.error("FAIL:", msg, "\n  got:", JSON.stringify(g), "\n  expected:", JSON.stringify(e));
    process.exit(1);
  }
}

console.log("Bank Account (TypeScript) – in-process verification");
console.log("");

// Reset state
actor.init("{}");
assert(actor.init("{}") === "", "init returns empty string");

// Balance initially 0
let r = actor.handle("", "call", '{"op":"balance"}');
assertJsonEq(r, { account: "", balance: 0 }, "initial balance");

// Deposit
r = actor.handle("", "call", '{"op":"deposit","amount":1000}');
assertJsonEq(r, { status: "ok", balance: 1000 }, "deposit 1000");

// Withdraw
r = actor.handle("", "call", '{"op":"withdraw","amount":200}');
assertJsonEq(r, { status: "ok", balance: 800 }, "withdraw 200");

// Balance
r = actor.handle("", "call", '{"op":"balance"}');
assertJsonEq(r, { account: "", balance: 800 }, "balance 800");

// Invalid deposit
r = actor.handle("", "call", '{"op":"deposit","amount":0}');
assert(JSON.parse(r).error === "invalid_amount", "invalid_amount");

// Insufficient funds
r = actor.handle("", "call", '{"op":"withdraw","amount":10000}');
assert(JSON.parse(r).error === "insufficient_funds", "insufficient_funds");

// History
r = actor.handle("", "call", '{"op":"history","count":5}');
const hist = JSON.parse(r);
assert(Array.isArray(hist.transactions) && hist.transactions.length === 2, "history length");

// Replay
r = actor.handle("", "call", '{"op":"replay"}');
const replay = JSON.parse(r);
assert(replay.replayed === 2 && replay.rebuilt_balance === 800 && replay.current_balance === 800, "replay");

// getState / setState (durability)
const stateJson = actor.getState();
actor.init("{}");
actor.setState(stateJson);
r = actor.handle("", "call", '{"op":"balance"}');
assertJsonEq(r, { account: "", balance: 800 }, "state restore");

// set_account
actor.handle("", "call", '{"op":"set_account","account_id":"alice"}');
r = actor.handle("", "call", '{"op":"balance"}');
assert(JSON.parse(r).account === "alice", "set_account");

console.log("OK All assertions passed.");
console.log("");
