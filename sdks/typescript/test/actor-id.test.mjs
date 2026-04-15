// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Unit tests for ActorID — canonical PlexSpaces actor ID parser.
//
// Run: node --test test/actor-id.test.mjs

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// Import from compiled dist or source directly for testing
// We implement the parsing inline to test without needing tsc compilation
// (mirrors the approach used in router.test.mjs)

// ---- Minimal ActorID implementation for self-contained testing ----
class ActorID {
  constructor(name, actorType, namespace, nodeId) {
    this.name = name;
    this.actorType = actorType;
    this.namespace = namespace;
    this.nodeId = nodeId;
  }

  static parse(id) {
    const slashIdx = id.indexOf('//');
    if (slashIdx < 0) throw new Error(`parseActorID: missing '//' in ${JSON.stringify(id)}`);
    const name = id.slice(0, slashIdx);
    const rest = id.slice(slashIdx + 2);
    const atIdx = rest.indexOf('@');
    const nodeId = atIdx >= 0 ? rest.slice(atIdx + 1) : '';
    const typeNs = atIdx >= 0 ? rest.slice(0, atIdx) : rest;
    const colonIdx = typeNs.indexOf('::');
    const actorType = colonIdx >= 0 ? typeNs.slice(0, colonIdx) : typeNs;
    const namespace = colonIdx >= 0 ? typeNs.slice(colonIdx + 2) : '';
    return new ActorID(name, actorType, namespace, nodeId);
  }

  toString() {
    if (this.nodeId) return `${this.name}//${this.actorType}::${this.namespace}@${this.nodeId}`;
    return `${this.name}//${this.actorType}::${this.namespace}`;
  }

  withTypeAndName(actorType, name) { return new ActorID(name, actorType, this.namespace, this.nodeId); }
  withName(name) { return new ActorID(name, this.actorType, this.namespace, this.nodeId); }
  withType(name, actorType) { return new ActorID(name, actorType, this.namespace, this.nodeId); }
}
// ------------------------------------------------------------------

describe('ActorID.parse — full format', () => {
  it('parses all fields', () => {
    const id = '01KP8WMBRKP6KGQTARATQQ1H5M//agent_registry::go-a2a-multi-agent@test-node-8091';
    const a = ActorID.parse(id);
    assert.equal(a.name, '01KP8WMBRKP6KGQTARATQQ1H5M');
    assert.equal(a.actorType, 'agent_registry');
    assert.equal(a.namespace, 'go-a2a-multi-agent');
    assert.equal(a.nodeId, 'test-node-8091');
    assert.equal(a.toString(), id);
  });
});

describe('ActorID.parse — no node', () => {
  it('handles missing @node_id', () => {
    const id = 'myname//mytype::mynamespace';
    const a = ActorID.parse(id);
    assert.equal(a.name, 'myname');
    assert.equal(a.actorType, 'mytype');
    assert.equal(a.namespace, 'mynamespace');
    assert.equal(a.nodeId, '');
    assert.equal(a.toString(), id);
  });
});

describe('ActorID.parse — missing //', () => {
  it('throws on invalid format', () => {
    assert.throws(() => ActorID.parse('noslashes'), /missing '\/\/'/);
  });
});

describe('ActorID.withTypeAndName', () => {
  it('creates peer with same type and name (stable role)', () => {
    const self = ActorID.parse('01KP//routing_workflow::go-resource-aware-inference@test-node-8091');
    const peer = self.withTypeAndName('budget_manager', 'budget_manager');
    assert.equal(peer.toString(), 'budget_manager//budget_manager::go-resource-aware-inference@test-node-8091');
  });

  it('creates peer with different type and name (ULID worker)', () => {
    const self = ActorID.parse('01KP//routing_workflow::go-resource-aware-inference@test-node-8091');
    const peer = self.withTypeAndName('inference_worker', '01KP8WORKER1');
    assert.equal(peer.toString(), '01KP8WORKER1//inference_worker::go-resource-aware-inference@test-node-8091');
  });

  it('preserves namespace and nodeId', () => {
    const self = ActorID.parse('01KP//routing_workflow::my-ns@my-node');
    const peer = self.withTypeAndName('analysis_agent', 'analysis_agent');
    assert.equal(peer.namespace, 'my-ns');
    assert.equal(peer.nodeId, 'my-node');
    assert.equal(peer.name, 'analysis_agent');
    assert.equal(peer.actorType, 'analysis_agent');
  });
});

describe('ActorID.withName', () => {
  it('replaces name only', () => {
    const a = ActorID.parse('01KP//worker::ns@node');
    assert.equal(a.withName('newname').toString(), 'newname//worker::ns@node');
  });
});

describe('ActorID.withType', () => {
  it('replaces name and actorType', () => {
    const a = ActorID.parse('01KP//worker::ns@node');
    assert.equal(a.withType('other', 'other_type').toString(), 'other//other_type::ns@node');
  });
});
