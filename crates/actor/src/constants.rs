// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Re-export constants from plexspaces-service-traits as the canonical definition.
// Do not redefine them here — duplicate definitions cause silent divergence.
pub use plexspaces_service_traits::{TEMP_SENDER_ACTOR_TYPE, TEMP_SENDER_PREFIX};
