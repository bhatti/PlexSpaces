// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ActorID — parsed representation of a canonical PlexSpaces actor ID.
//
// Canonical format: {name}//{actor_type}::{namespace}@{node_id}
//
// The name is typically a stable role name for supervisor-spawned actors,
// or a ULID for dynamically spawned actors.
//
// Usage — peer/sibling discovery from within a WASM actor:
//
//   const selfID = ActorID.parse(host.selfId());
//   const budgetID = selfID.sibling("budget_manager");
//   const reply = await host.ask(budgetID.toString(), payload, 5000);

/**
 * Parsed form of a canonical PlexSpaces actor ID.
 *
 * Canonical string format: {name}//{actor_type}::{namespace}@{node_id}
 */
export class ActorID {
  /** Unique instance identifier (role name or ULID). */
  readonly name: string;
  /** Behavior type registered in the application (e.g. 'budget_manager'). */
  readonly actorType: string;
  /** Application namespace (e.g. 'go-resource-aware-inference'). */
  readonly namespace: string;
  /** Node hosting the actor (e.g. 'test-node-8091'). Empty string if absent. */
  readonly nodeId: string;

  constructor(name: string, actorType: string, namespace: string, nodeId: string) {
    this.name = name;
    this.actorType = actorType;
    this.namespace = namespace;
    this.nodeId = nodeId;
  }

  /**
   * Parse a canonical actor ID string into an ActorID.
   *
   * Expected format: {name}//{actor_type}::{namespace}@{node_id}
   * Throws if the string does not contain the expected separators.
   */
  static parse(id: string): ActorID {
    const slashIdx = id.indexOf("//");
    if (slashIdx < 0) {
      throw new Error(`parseActorID: missing '//' in ${JSON.stringify(id)}`);
    }
    const name = id.slice(0, slashIdx);
    const rest = id.slice(slashIdx + 2); // "{actor_type}::{namespace}@{node_id}"

    // Split on "@" to separate type::namespace from node_id
    const atIdx = rest.indexOf("@");
    const nodeId = atIdx >= 0 ? rest.slice(atIdx + 1) : "";
    const typeNs = atIdx >= 0 ? rest.slice(0, atIdx) : rest;

    // Split on "::" to separate actor_type from namespace
    const colonIdx = typeNs.indexOf("::");
    const actorType = colonIdx >= 0 ? typeNs.slice(0, colonIdx) : typeNs;
    const namespace = colonIdx >= 0 ? typeNs.slice(colonIdx + 2) : "";

    return new ActorID(name, actorType, namespace, nodeId);
  }

  /** Return the canonical actor ID string: {name}//{actor_type}::{namespace}@{node_id}. */
  toString(): string {
    if (this.nodeId) {
      return `${this.name}//${this.actorType}::${this.namespace}@${this.nodeId}`;
    }
    return `${this.name}//${this.actorType}::${this.namespace}`;
  }

  /**
   * Return a copy with an explicit actor type and name.
   *
   * Use this to build a canonical ID for a peer actor with the given type and name,
   * keeping the same namespace and node.
   *
   * For supervisor-spawned actors with stable role names (name == type == role):
   * ```ts
   * const peer = self.withTypeAndName("budget_manager", "budget_manager");
   * ```
   *
   * For actors where name and type differ (e.g. ULID-named workers of a shared type):
   * ```ts
   * const peer = self.withTypeAndName("inference_worker", ulid);
   * ```
   */
  withTypeAndName(actorType: string, name: string): ActorID {
    return new ActorID(name, actorType, this.namespace, this.nodeId);
  }

  /** Return a copy with a different name. */
  withName(name: string): ActorID {
    return new ActorID(name, this.actorType, this.namespace, this.nodeId);
  }

  /** Return a copy with a different name and actor_type. */
  withType(name: string, actorType: string): ActorID {
    return new ActorID(name, actorType, this.namespace, this.nodeId);
  }
}
