#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// In-process verification of guild_chat_actor (no server needed).
// Imports compiled JS and exercises ChatRoom + RateLimiter via ActorRouter.

import { actor } from "./guild_chat_actor.js";

function check(label, condition, actual) {
  if (condition) {
    console.log(`  ✓ ${label}`);
  } else {
    console.error(`  ✗ ${label} — got: ${JSON.stringify(actual)}`);
    process.exitCode = 1;
  }
}

function call(op, payload = {}) {
  const result = actor.handle("test", "message", JSON.stringify({ op, ...payload }));
  return JSON.parse(result);
}

console.log("Guild Chat Actor — In-Process Verification");
console.log("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
console.log("");

// Initialize ChatRoom actor
console.log("1. Initialize ChatRoom");
const initResult = actor.init(JSON.stringify({
  actor_id: "chat-room:test@node",
  args: { max_history: "50" },
}));
check("init succeeds", !initResult.startsWith("ERROR"), initResult);
console.log("");

// Join users
console.log("2. Join users");
let r = call("join", { user_id: "alice" });
check("alice joins", r.action === "joined", r);

r = call("join", { user_id: "bob" });
check("bob joins", r.action === "joined" && r.members === 2, r);

r = call("join", { user_id: "alice" });
check("alice re-join → already_joined", r.action === "already_joined", r);
console.log("");

// Send messages
console.log("3. Send messages");
r = call("send_message", { user_id: "alice", content: "Hello everyone!" });
check("alice sends (fan_out=1)", r.status === "ok" && r.fan_out === 1, r);

r = call("send_message", { user_id: "bob", content: "Hi alice!" });
check("bob sends (fan_out=1)", r.status === "ok" && r.fan_out === 1, r);

r = call("send_message", { user_id: "unknown", content: "test" });
check("non-member blocked", r.error === "not a member of this room", r);
console.log("");

// Get history
console.log("4. Get history");
r = call("get_history", { limit: 10 });
check("history count >= 4", r.count >= 4, r);  // 2 join system msgs + 2 user msgs
console.log("");

// Get members
console.log("5. Get members");
r = call("get_members");
check("2 members", r.count === 2, r);
console.log("");

// Leave
console.log("6. Leave");
r = call("leave", { user_id: "bob" });
check("bob leaves", r.action === "left" && r.members === 1, r);
console.log("");

// Stats
console.log("7. Stats");
r = call("stats");
check("total_messages >= 2", r.counters?.total_messages >= 2, r);
check("total_joins >= 2", r.counters?.total_joins >= 2, r);
check("active_members === 1", r.counters?.active_members === 1, r);
console.log("");

// State round-trip
console.log("8. State persistence (getState → setState)");
const savedState = actor.getState();
check("getState returns JSON", savedState.length > 10, savedState);

// Re-init with different actor to test state restoration
const initResult2 = actor.init(JSON.stringify({
  actor_id: "rate-limiter:test@node",
  args: { max_tokens: "3", refill_rate_ms: "500" },
}));
check("rate-limiter init", !initResult2.startsWith("ERROR"), initResult2);

// Check rate limiter
console.log("");
console.log("9. RateLimiter");
r = call("check_rate", { user_id: "user-1" });
check("first check allowed", r.allowed === true && r.remaining === 2, r);

r = call("check_rate", { user_id: "user-1" });
r = call("check_rate", { user_id: "user-1" });
check("3rd check allowed (last token)", r.allowed === true && r.remaining === 0, r);

r = call("check_rate", { user_id: "user-1" });
check("4th check denied", r.allowed === false, r);

r = call("stats");
check("RL stats: 4 checks", r.counters?.total_checks === 4, r);
check("RL stats: 3 allowed", r.counters?.total_allowed === 3, r);
check("RL stats: 1 denied", r.counters?.total_denied === 1, r);
console.log("");

// Batch benchmark
console.log("10. Batch benchmark");
r = call("check_rate_batch", { user_id: "bench", count: 1000 });
check("batch total >= 1000", r.total_requests >= 1000, r);
check("batch has allowed+denied", r.allowed > 0 && r.denied > 0, r);
console.log(`    ${r.total_requests} checks in ${r.duration_ms}ms → ${Math.round(r.ops_per_sec)} ops/sec`);
console.log("");

console.log("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
console.log("✅ All verifications passed.");
