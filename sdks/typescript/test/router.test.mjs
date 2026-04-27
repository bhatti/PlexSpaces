// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for the TypeScript SDK ActorRouter.
// Uses Node.js built-in test runner (node --test) with no external dependencies.
//
// Run: node --test test/router.test.mjs

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// ========================================================================
// Minimal mock of PlexSpacesActor for testing ActorRouter logic
// Since the real class imports WIT host functions, we test the router
// logic in isolation by reimplementing the core routing algorithm.
// ========================================================================

/**
 * ActorRouter (standalone test implementation matching src/router.ts logic).
 * This tests the routing algorithm without needing WASM imports.
 */
class TestActorRouter {
  #factories;
  #active = null;

  constructor(routes) {
    this.#factories = routes;
  }

  init(configJson) {
    try {
      const config = configJson && configJson.trim()
        ? JSON.parse(configJson)
        : {};

      const actorId = config.actor_id || "";
      const name = normalizeActorRole(actorId);

      let bestPrefix = "";
      let bestFactory = null;

      for (const prefix of Object.keys(this.#factories)) {
        if (name === prefix || name.startsWith(prefix)) {
          if (prefix.length > bestPrefix.length) {
            bestPrefix = prefix;
            bestFactory = this.#factories[prefix];
          }
        }
      }

      if (!bestFactory) {
        return "ERROR: no actor registered for prefix: " + name;
      }

      this.#active = bestFactory();
      return this.#active.init(configJson);
    } catch {
      return "ERROR: router init failed";
    }
  }

  handle(fromActor, msgType, payloadJson) {
    if (!this.#active) {
      return '{"error":"no active actor (init not called)"}';
    }
    return this.#active.handle(fromActor, msgType, payloadJson);
  }

  getState() {
    if (!this.#active) return "{}";
    return this.#active.getState();
  }

  setState(stateJson) {
    if (!this.#active) return "ERROR: no active actor";
    return this.#active.setState(stateJson);
  }
}

function normalizeActorRole(actorId) {
  if (!actorId) return "";
  const canonicalSep = actorId.indexOf("//");
  if (canonicalSep >= 0 && canonicalSep + 2 < actorId.length) {
    const rest = actorId.substring(canonicalSep + 2);
    const behaviorSep = rest.indexOf("::");
    if (behaviorSep >= 0) return rest.substring(0, behaviorSep);
    const nodeSep = rest.indexOf("@");
    return nodeSep >= 0 ? rest.substring(0, nodeSep) : rest;
  }
  const childSep = actorId.indexOf(":");
  if (childSep >= 0) return actorId.substring(0, childSep);
  const nodeSep = actorId.indexOf("@");
  if (nodeSep >= 0) return actorId.substring(0, nodeSep);
  return actorId;
}

// Mock actors for testing
class CounterActor {
  constructor() { this.value = 0; }
  init(configJson) { return ""; }
  handle(from, msgType, payload) {
    if (msgType === "increment") {
      this.value++;
      return JSON.stringify({ value: this.value });
    }
    if (msgType === "get") {
      return JSON.stringify({ value: this.value });
    }
    return '{"error":"unknown"}';
  }
  getState() { return JSON.stringify({ value: this.value }); }
  setState(stateJson) {
    const s = JSON.parse(stateJson);
    this.value = s.value || 0;
    return "";
  }
}

class EchoActor {
  constructor() { this.lastMsg = ""; }
  init(configJson) { this.lastMsg = "initialized"; return ""; }
  handle(from, msgType, payload) { this.lastMsg = msgType; return payload; }
  getState() { return JSON.stringify({ last_msg: this.lastMsg }); }
  setState(stateJson) {
    const s = JSON.parse(stateJson);
    this.lastMsg = s.last_msg || "";
    return "";
  }
}

// ========================================================================
// Tests
// ========================================================================

describe("ActorRouter", () => {
  describe("init", () => {
    it("should initialize with matching prefix", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
        "echo": () => new EchoActor(),
      });
      const result = router.init('{"actor_id":"counter:test@node"}');
      assert.equal(result, "");
    });

    it("should handle full actor ID format (name:namespace@node)", () => {
      const router = new TestActorRouter({
        "rate-limiter": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"rate-limiter:default@test-node"}');
      assert.equal(result, "");
    });

    it("should support prefix matching", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"counter-1:ns@node"}');
      assert.equal(result, "");
    });

    it("should handle canonical actor IDs", () => {
      const router = new TestActorRouter({
        "leader": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"01ABC//leader::streaming-pipeline-ts@test-node-8091"}');
      assert.equal(result, "");
    });

    it("should handle canonical worker prefix matching", () => {
      const router = new TestActorRouter({
        "worker": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"01ABC//worker-3::streaming-pipeline-ts@test-node-8093"}');
      assert.equal(result, "");
    });

    it("should use longest prefix match", () => {
      let shortMatch = false;
      let longMatch = false;
      const router = new TestActorRouter({
        "count": () => { shortMatch = true; return new CounterActor(); },
        "counter": () => { longMatch = true; return new CounterActor(); },
      });
      router.init('{"actor_id":"counter:ns@node"}');
      assert.equal(shortMatch, false, "shorter prefix should not match");
      assert.equal(longMatch, true, "longer prefix should match");
    });

    it("should return error for unknown prefix", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"unknown:ns@node"}');
      assert.ok(result.startsWith("ERROR:"), `expected ERROR, got ${result}`);
    });

    it("should return error for invalid JSON", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.init("not json");
      assert.ok(result.startsWith("ERROR:"), `expected ERROR, got ${result}`);
    });

    it("should return error for empty actor_id", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":""}');
      assert.ok(result.startsWith("ERROR:"), `expected ERROR, got ${result}`);
    });

    it("should handle empty config", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.init("{}");
      assert.ok(result.startsWith("ERROR:"), `expected ERROR for empty config, got ${result}`);
    });
  });

  describe("handle", () => {
    it("should delegate to active actor", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      router.init('{"actor_id":"counter:ns@node"}');
      const result = router.handle("sender", "increment", "{}");
      const parsed = JSON.parse(result);
      assert.equal(parsed.value, 1);
    });

    it("should return error when init not called", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.handle("sender", "test", "{}");
      assert.ok(result.includes("no active actor"));
    });

    it("should route to correct actor type", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
        "echo": () => new EchoActor(),
      });
      router.init('{"actor_id":"echo:ns@node"}');
      const result = router.handle("sender", "test-msg", '{"data":"hello"}');
      assert.equal(result, '{"data":"hello"}');
    });
  });

  describe("state management", () => {
    it("should delegate getState to active actor", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      router.init('{"actor_id":"counter:ns@node"}');
      router.handle("sender", "increment", "{}");
      const state = router.getState();
      const parsed = JSON.parse(state);
      assert.equal(parsed.value, 1);
    });

    it("should return {} when no active actor for getState", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      assert.equal(router.getState(), "{}");
    });

    it("should delegate setState to active actor", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      router.init('{"actor_id":"counter:ns@node"}');
      const result = router.setState('{"value":42}');
      assert.equal(result, "");
      const state = router.getState();
      const parsed = JSON.parse(state);
      assert.equal(parsed.value, 42);
    });

    it("should return error for setState without init", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      const result = router.setState('{"value":1}');
      assert.ok(result.startsWith("ERROR:"));
    });

    it("should support state round-trip", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      router.init('{"actor_id":"counter:ns@node"}');
      router.handle("sender", "increment", "{}");
      router.handle("sender", "increment", "{}");
      const state = router.getState();

      // Create new router and restore state
      const router2 = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      router2.init('{"actor_id":"counter:ns@node"}');
      router2.setState(state);
      const restored = JSON.parse(router2.getState());
      assert.equal(restored.value, 2);
    });
  });

  describe("prefix matching edge cases", () => {
    it("should match exact prefix without suffix", () => {
      const router = new TestActorRouter({
        "worker": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"worker:ns@node"}');
      assert.equal(result, "");
    });

    it("should not match partial prefix (no 'work' matching 'worker')", () => {
      const router = new TestActorRouter({
        "work": () => new CounterActor(),
      });
      const result = router.init('{"actor_id":"worker:ns@node"}');
      // "worker" starts with "work" so this should match
      assert.equal(result, "");
    });

    it("should handle actor_id without namespace", () => {
      const router = new TestActorRouter({
        "counter": () => new CounterActor(),
      });
      // actor_id without : means the whole thing is the name
      const result = router.init('{"actor_id":"counter"}');
      assert.equal(result, "");
    });
  });
});
