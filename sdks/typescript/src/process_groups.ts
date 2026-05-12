// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors

/** Return the first process-group member, or null if the group is empty. */
export function firstGroupMember(members: string[]): string | null {
  return members.length > 0 ? members[0] : null;
}

/** Return the first process-group member, throwing if the group is empty. */
export function firstGroupMemberOrThrow(group: string, members: string[]): string {
  const first = firstGroupMember(members);
  if (first === null) {
    throw new Error(`no members in process group '${group}'`);
  }
  return first;
}
