// SPDX-License-Identifier: LGPL-2.1-or-later
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

  /** WIT init(config-json) -> string */
  init(configJson: string): string {
    try {
      const config = configJson && configJson.trim()
        ? JSON.parse(configJson) as Record<string, unknown>
        : {};

      const actorId = (config.actor_id as string) || "";

      const name = normalizeActorRole(actorId);

      // Find matching factory by longest prefix match
      let bestPrefix = "";
      let bestFactory: ActorFactory | null = null;

      for (const prefix of Object.keys(this.factories)) {
        if (name === prefix || name.startsWith(prefix)) {
          if (prefix.length > bestPrefix.length) {
            bestPrefix = prefix;
            bestFactory = this.factories[prefix];
          }
        }
      }

      if (!bestFactory) {
        return "ERROR: no actor registered for prefix: " + name;
      }

      this.active = bestFactory();
      return this.active.init(configJson);
    } catch {
      return "ERROR: router init failed";
    }
  }

  /** WIT handle(from-actor, msg-type, payload-json) -> string */
  handle(fromActor: string, msgType: string, payloadJson: string): string {
    if (!this.active) {
      return '{"error":"no active actor (init not called)"}';
    }
    return this.active.handle(fromActor, msgType, payloadJson);
  }

  /** WIT get-state() -> string */
  getState(): string {
    if (!this.active) {
      return "{}";
    }
    return this.active.getState();
  }

  /** WIT set-state(state-json) -> string */
  setState(stateJson: string): string {
    if (!this.active) {
      return "ERROR: no active actor";
    }
    return this.active.setState(stateJson);
  }
}
