// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Tests for ProcessGroups.first / firstOrThrow.
// Uses inline mock objects to simulate host PG functions without
// requiring the WASM host imports.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

// ---------------------------------------------------------------------------
// Inline ProcessGroups simulation (mirrors ProcessGroups.members + first logic)
// ---------------------------------------------------------------------------

function pgMembers(store, group) {
  return store.get(group) ?? [];
}

function pgFirst(store, group) {
  const members = pgMembers(store, group);
  return members.length > 0 ? members[0] : null;
}

function pgFirstOrThrow(store, group) {
  const members = pgMembers(store, group);
  if (members.length === 0) {
    throw new Error(`no members in process group '${group}'`);
  }
  return members[0];
}

// ---------------------------------------------------------------------------
// ProcessGroups.first tests
// ---------------------------------------------------------------------------

describe('ProcessGroups.first', () => {
  it('returns the first member when group has members', () => {
    const store = new Map([['svc:test', ['actor1@node', 'actor2@node']]]);
    assert.equal(pgFirst(store, 'svc:test'), 'actor1@node');
  });

  it('returns null when group has no members', () => {
    const store = new Map();
    assert.equal(pgFirst(store, 'svc:empty'), null);
  });

  it('returns the only member in a single-member group', () => {
    const store = new Map([['svc:llm', ['llm-router@node']]]);
    assert.equal(pgFirst(store, 'svc:llm'), 'llm-router@node');
  });
});

// ---------------------------------------------------------------------------
// ProcessGroups.firstOrThrow tests
// ---------------------------------------------------------------------------

describe('ProcessGroups.firstOrThrow', () => {
  it('returns the first member when group has members', () => {
    const store = new Map([['svc:agent', ['agent-1@node', 'agent-2@node']]]);
    assert.equal(pgFirstOrThrow(store, 'svc:agent'), 'agent-1@node');
  });

  it('throws when group is empty', () => {
    const store = new Map();
    assert.throws(
      () => pgFirstOrThrow(store, 'svc:missing'),
      /no members in process group 'svc:missing'/,
    );
  });

  it('throws with the group name in the error message', () => {
    const store = new Map([['svc:present', ['x@n']]]);
    assert.throws(
      () => pgFirstOrThrow(store, 'svc:absent'),
      /svc:absent/,
    );
  });
});
