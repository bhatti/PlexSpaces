// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Command parsing utilities for the Redis Cluster example.

use crate::SetOptions;

/// Parse NX/XX/EX/PX options from SET args slice.
/// Expects args in the form: `key value [NX|XX] [EX seconds | PX millis]`
pub fn parse_set_options(args: &[&str]) -> SetOptions {
    let mut opts = SetOptions::default();
    let mut i = 2; // skip key and value
    while i < args.len() {
        match args[i].to_uppercase().as_str() {
            "NX" => { opts.nx = true; i += 1; }
            "XX" => { opts.xx = true; i += 1; }
            "EX" => {
                if i + 1 < args.len() {
                    opts.ex = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "PX" => {
                if i + 1 < args.len() {
                    opts.px = args[i + 1].parse().ok();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => { i += 1; }
        }
    }
    opts
}

/// Returns true if this command mutates state and must be replicated.
pub fn is_write_command(name: &str) -> bool {
    matches!(
        name.to_uppercase().as_str(),
        "SET" | "DEL" | "INCR" | "DECR" | "APPEND" | "EXPIRE" | "PERSIST" | "RENAME"
    )
}

/// Simple hash-based shard routing — same algorithm as the PlexSpaces framework.
pub fn shard_for_key(key: &str, shard_count: usize) -> usize {
    let hash = key
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % shard_count as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_set_nx_ex() {
        let args = ["key", "val", "NX", "EX", "60"];
        let opts = parse_set_options(&args);
        assert!(opts.nx);
        assert!(!opts.xx);
        assert_eq!(opts.ex, Some(60));
        assert!(opts.px.is_none());
    }

    #[test]
    fn parse_set_px() {
        let args = ["key", "val", "PX", "5000"];
        let opts = parse_set_options(&args);
        assert_eq!(opts.px, Some(5000));
        assert!(opts.ex.is_none());
    }

    #[test]
    fn is_write_command_check() {
        assert!(is_write_command("SET"));
        assert!(is_write_command("set"));
        assert!(is_write_command("DEL"));
        assert!(is_write_command("INCR"));
        assert!(!is_write_command("GET"));
        assert!(!is_write_command("PING"));
    }

    #[test]
    fn shard_routing_deterministic() {
        let s1 = shard_for_key("user:1", 3);
        let s2 = shard_for_key("user:1", 3);
        assert_eq!(s1, s2);
    }
}
