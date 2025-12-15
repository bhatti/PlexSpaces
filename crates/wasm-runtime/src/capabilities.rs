// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASI capabilities for WASM actors (capability-based security)
//!
//! ## Proto-First Design
//! Capabilities are defined in proto and re-exported here for convenience.
//! See `proto/plexspaces/v1/wasm.proto` for the source of truth.

// Re-export proto-generated WasmCapabilities
pub use plexspaces_proto::wasm::v1::WasmCapabilities;

// Helper module for creating capability profiles
pub mod profiles {
    use super::WasmCapabilities;

    /// Create default capabilities (untrusted actors, minimal permissions)
    pub fn default() -> WasmCapabilities {
        WasmCapabilities {
            allow_filesystem: false,
            filesystem_root: String::new(),
            allow_network: false,
            allow_env: false,
            allow_random: true, // Safe to allow
            allow_clocks: true, // Safe to allow
            allow_tuplespace: true,    // Most actors need coordination
            allow_spawn_actors: false, // Only supervisors need this
            allow_send_messages: true, // Most actors send messages
            allow_logging: true,       // Useful for debugging
        }
    }

    /// Create untrusted profile (minimal capabilities)
    pub fn untrusted() -> WasmCapabilities {
        WasmCapabilities {
            allow_filesystem: false,
            filesystem_root: String::new(),
            allow_network: false,
            allow_env: false,
            allow_random: true,
            allow_clocks: true,
            allow_tuplespace: false, // No coordination
            allow_spawn_actors: false,
            allow_send_messages: true, // Can only send messages
            allow_logging: true,
        }
    }

    /// Create trusted profile (full capabilities)
    pub fn trusted() -> WasmCapabilities {
        WasmCapabilities {
            allow_filesystem: true,
            filesystem_root: "/actors".to_string(),
            allow_network: true,
            allow_env: true,
            allow_random: true,
            allow_clocks: true,
            allow_tuplespace: true,
            allow_spawn_actors: true,
            allow_send_messages: true,
            allow_logging: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_capabilities() {
        let caps = profiles::default();
        assert!(!caps.allow_filesystem);
        assert!(!caps.allow_network);
        assert!(caps.allow_send_messages);
        assert!(caps.allow_tuplespace);
    }

    #[test]
    fn test_untrusted_capabilities() {
        let caps = profiles::untrusted();
        assert!(!caps.allow_filesystem);
        assert!(!caps.allow_network);
        assert!(!caps.allow_tuplespace);
        assert!(!caps.allow_spawn_actors);
    }

    #[test]
    fn test_trusted_capabilities() {
        let caps = profiles::trusted();
        assert!(caps.allow_filesystem);
        assert!(caps.allow_network);
        assert!(caps.allow_tuplespace);
        assert!(caps.allow_spawn_actors);
    }
}
