// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Tests for process-group first-member helpers.
// These target the pure helper module shared by ProcessGroups.first,
// ProcessGroups.firstOrThrow, and the exported pgFirst helpers.

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { firstGroupMember, firstGroupMemberOrThrow } from '../dist/process_groups.js';

// ---------------------------------------------------------------------------
// ProcessGroups.first tests
// ---------------------------------------------------------------------------

describe('ProcessGroups.first', () => {
  it('returns the first member when group has members', () => {
    assert.equal(firstGroupMember(['actor1@node', 'actor2@node']), 'actor1@node');
  });

  it('returns null when group has no members', () => {
    assert.equal(firstGroupMember([]), null);
  });

  it('returns the only member in a single-member group', () => {
    assert.equal(firstGroupMember(['llm-router@node']), 'llm-router@node');
  });
});

// ---------------------------------------------------------------------------
// Exported pgFirst helper tests
// ---------------------------------------------------------------------------

describe('pgFirst helper', () => {
  it('mirrors ProcessGroups.first semantics', () => {
    assert.equal(firstGroupMember(['actor1@node', 'actor2@node']), 'actor1@node');
  });

  it('returns null when group is empty', () => {
    assert.equal(firstGroupMember([]), null);
  });
});

// ---------------------------------------------------------------------------
// ProcessGroups.firstOrThrow tests
// ---------------------------------------------------------------------------

describe('ProcessGroups.firstOrThrow', () => {
  it('returns the first member when group has members', () => {
    assert.equal(firstGroupMemberOrThrow('svc:agent', ['agent-1@node', 'agent-2@node']), 'agent-1@node');
  });

  it('throws when group is empty', () => {
    assert.throws(
      () => firstGroupMemberOrThrow('svc:missing', []),
      /no members in process group 'svc:missing'/,
    );
  });

  it('throws with the group name in the error message', () => {
    assert.throws(
      () => firstGroupMemberOrThrow('svc:absent', []),
      /svc:absent/,
    );
  });
});
