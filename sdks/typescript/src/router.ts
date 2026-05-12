// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces TypeScript SDK - Multi-Actor Router
//
// Routes messages to multiple actor types within a single WASM module.
//
// Dispatch rule (matches Python and Go SDKs):
//   1. config.actor_type — exact lookup (the normal case; always set by the framework).
//   2. config.role       — exact lookup (same-actor_type multi-instance only,
//                          e.g. "ephemeral"/"channel" both map to AbstractionsActor).
//   3. Error — no silent fallback; a missing registration is always a bug.

import { PlexSpacesActor } from "./actor.js";
import { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from "./wit-payload.js";

/** Factory function that creates an actor instance. */
type ActorFactory = () => PlexSpacesActor;

/**
 * Routes messages to multiple actor types within a single WASM module.
 *
 * Register each actor type under its exact class name, which matches what
 * the framework places in ``config.actor_type``:
 *
 *   const router = new ActorRouter({
 *     "ChatRoomActor":  () => new ChatRoomActor(),
 *     "RateLimiterActor": () => new RateLimiterActor(),
 *   });
 *   export const actor = router;
 *
 * For same-class multi-instance dispatch, register under the role name:
 *
 *   const router = new ActorRouter({
 *     "AbstractionsActor": () => new AbstractionsActor(),
 *     "ephemeral":         () => new AbstractionsActor(),
 *     "channel":           () => new AbstractionsActor(),
 *   });
 */
export class ActorRouter {
  private readonly factories: Record<string, ActorFactory>;
  private active: PlexSpacesActor | null = null;

  constructor(routes: Record<string, ActorFactory>) {
    this.factories = routes;
  }

  /** WIT `init(config: payload) -> result<_, actor-error>` */
  init(configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView): void {
    const text = decodeWitPayloadUtf8(configJson);
    const config = text.trim() ? JSON.parse(text) as Record<string, unknown> : {};

    const actorType = (config.actor_type as string) || "";
    const role = (config.role as string) || "";

    // 1. actor_type — exact match (primary dispatch key set by framework)
    let factory = actorType ? this.factories[actorType] : undefined;

    // 2. role — exact match (same-actor_type multi-instance only)
    if (!factory && role) {
      factory = this.factories[role];
    }

    if (!factory) {
      throw new Error(
        `ERROR: no actor registered for actor_type='${actorType}' role='${role}'`,
      );
    }

    this.active = factory();
    this.active.init(text);
  }

  /** WIT `handle(...) -> result<payload, actor-error>` */
  handle(
    fromActor: string,
    msgType: string,
    payloadJson: string | Uint8Array | ArrayBuffer | ArrayBufferView,
  ): Uint8Array {
    if (!this.active) {
      return encodeWitPayloadUtf8('{"error":"no active actor (init not called)"}');
    }
    return this.active.handle(fromActor, msgType, payloadJson);
  }

  /** WIT `get-state() -> result<payload, actor-error>` */
  getState(): Uint8Array {
    if (!this.active) {
      return encodeWitPayloadUtf8("{}");
    }
    return this.active.getState();
  }

  /** WIT `set-state(state: payload) -> result<_, actor-error>` */
  setState(stateJson: string | Uint8Array | ArrayBuffer | ArrayBufferView): void {
    if (!this.active) {
      throw new Error("ERROR: no active actor");
    }
    this.active.setState(stateJson);
  }
}
