// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Resource limits for WASM actors (memory, fuel, CPU time)
//!
//! ## Proto-First Design
//! Resource limits are defined in proto and re-exported here for convenience.
//! See `proto/plexspaces/v1/wasm.proto` for the source of truth.

// Re-export proto-generated ResourceLimits
pub use plexspaces_proto::wasm::v1::ResourceLimits;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_limits() {
        let limits = ResourceLimits {
            max_memory_bytes: 16 * 1024 * 1024,
            max_stack_bytes: 512 * 1024,
            max_fuel: 10_000_000_000,
            max_execution_time: None,
            max_table_elements: 10_000,
            max_pooled_instances: 10,
        };
        assert_eq!(limits.max_memory_bytes, 16 * 1024 * 1024);
        assert_eq!(limits.max_fuel, 10_000_000_000);
    }
}
