// SPDX-License-Identifier: LGPL-2.1-or-later
// Minimal test to reproduce WASM recursion issue
// This test can be run standalone to quickly iterate on serialization fixes

import { PlexSpacesActor } from "@plexspaces/sdk";

interface TestState {
  count: number;
}

class MinimalTestActor extends PlexSpacesActor<TestState> {
  getDefaultState(): TestState {
    return { count: 0 };
  }

  onTest(payload: Record<string, unknown>): Record<string, unknown> {
    // Return minimal object with array to test serialization
    return {
      status: "ok",
      items: [
        { id: "1", value: 10 },
        { id: "2", value: 20 },
      ],
    };
  }
}

// Test serialization directly (without WASM)
const actor = new MinimalTestActor();
const result = actor.onTest({});

// This will call json() which calls safeStringify
try {
  const serialized = (actor as any).json(result);
  console.log("Serialization succeeded:", serialized.substring(0, 100));
} catch (e) {
  console.error("Serialization failed:", e);
  process.exit(1);
}
