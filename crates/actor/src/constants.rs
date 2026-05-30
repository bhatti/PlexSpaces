// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Constants used across PlexSpaces

/// Temporary sender ID prefix for ask() pattern.
/// Format: "{TEMP_SENDER_PREFIX}_{correlation_id}" in ActorId.name().
pub const TEMP_SENDER_PREFIX: &str = "ask";

/// Internal actor type for temporary senders used by ask/reply routing.
pub const TEMP_SENDER_ACTOR_TYPE: &str = "temporary_sender";
