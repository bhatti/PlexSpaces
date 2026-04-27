// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// PlexSpaces TypeScript SDK - Multi-Actor Router
//
// Routes messages to multiple actor types within a single WASM module.
// Equivalent to Python SDK's ACTOR_ROLES dict and Go SDK's ActorRouter.
//
// Example:
//   const router = new ActorRouter({
//     "rate-limiter": () => new RateLimiterActor(),
//     "counter": () => new CounterActor(),
//   });
//   export const actor = router;

import { PlexSpacesActor } from "./actor.js";
import { decodeWitPayloadUtf8, encodeWitPayloadUtf8 } from "./wit-payload.js";

/**
 * Factory function that creates an actor instance.
 */
type ActorFactory = () => PlexSpacesActor;

function normalizeActorRole(actorId: string): string {
  if (!actorId) {
    return "";
  }
  const canonicalSep = actorId.indexOf("//");
  if (canonicalSep >= 0 && canonicalSep + 2 < actorId.length) {
    const rest = actorId.substring(canonicalSep + 2);
    const behaviorSep = rest.indexOf("::");
    if (behaviorSep >= 0) {
      return rest.substring(0, behaviorSep);
    }
    const nodeSep = rest.indexOf("@");
    return nodeSep >= 0 ? rest.substring(0, nodeSep) : rest;
  }
  const childSep = actorId.indexOf(":");
  if (childSep >= 0) {
    return actorId.substring(0, childSep);
  }
  const nodeSep = actorId.indexOf("@");
  if (nodeSep >= 0) {
    return actorId.substring(0, nodeSep);
  }
  return actorId;
}

/**
 * Routes messages to multiple actor types within a single WASM module.
 *
 * When a WASM module serves multiple actor types (via ApplicationSpec),
 * the framework passes `{"actor_id": "rate-limiter:ns@node"}` in the init config.
 * The router uses prefix matching on the actor_id to select the correct factory.
 *
 * This is the TypeScript equivalent of:
 * - Python: `ACTOR_ROLES = {"rate-limiter": RateLimiter, "counter": Counter}`
 * - Go: `router.Route("rate-limiter", func() plexspaces.Actor { ... })`
 */
export class ActorRouter {
  private factories: Record<string, ActorFactory>;
  private active: PlexSpacesActor | null = null;

  /**
   * Create an ActorRouter with prefix-to-factory mappings.
   *
   * @param routes - Map of actor_id prefix to factory function.
   *   Prefix matching: "rate-limiter" matches "rate-limiter-0", "rate-limiter-1", etc.
   *
   * Example:
   *   new ActorRouter({
   *     "parameter-server": () => new ParameterServerActor(),
   *     "data-worker": () => new DataWorkerActor(),
   *   })
   */
  constructor(routes: Record<string, ActorFactory>) {
    this.factories = routes;
  }

  /** WIT `init(config: payload) -> result<_, actor-error>` */
  init(configJson: string | Uint8Array | ArrayBuffer | ArrayBufferView): void {
    try {
      const text = decodeWitPayloadUtf8(configJson);
      const config = text.trim()
        ? JSON.parse(text) as Record<string, unknown>
        : {};

      const actorId = (config.actor_id as string) || "";
      // actor_type from init config is the authoritative dispatch key (set by ChildSpec.actor_type).
      // Fall back to extracting the type component from the canonical actor_id.
      const actorType = (config.actor_type as string) || normalizeActorRole(actorId);
      // declaration_name is the child spec name (e.g. "router", "chain") — used when multiple
      // children share the same actor_type (e.g. all use "llm_workflow_orchestrator_wasm").
      const declarationName = (config.declaration_name as string) || "";

      const findFactory = (key: string): [string, ActorFactory | null] => {
        let bestPrefix = "";
        let bestFactory: ActorFactory | null = null;
        for (const prefix of Object.keys(this.factories)) {
          if (key === prefix || key.startsWith(prefix)) {
            if (prefix.length > bestPrefix.length) {
              bestPrefix = prefix;
              bestFactory = this.factories[prefix];
            }
          }
        }
        return [bestPrefix, bestFactory];
      };

      // Try declaration_name first (exact child name match wins when present).
      // Fall back to actor_type prefix matching for single-actor-type modules.
      let [, bestFactory] = declarationName ? findFactory(declarationName) : ["", null];
      if (!bestFactory) {
        [, bestFactory] = findFactory(actorType);
      }

      if (!bestFactory) {
        throw new Error("ERROR: no actor registered for declaration_name='" + declarationName + "' actor_type='" + actorType + "'");
      }

      this.active = bestFactory();
      this.active.init(text);
    } catch (e) {
      if (e instanceof Error && e.message.startsWith("ERROR:")) {
        throw e;
      }
      throw new Error("ERROR: router init failed");
    }
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
