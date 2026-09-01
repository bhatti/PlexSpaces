// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Redis Cluster — domain types and module re-exports.

use serde::{Deserialize, Serialize};

// =============================================================================
// Domain Types
// =============================================================================

/// A value stored in the Redis-like key–value store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEntry {
    pub value: String,
    /// Unix-epoch milliseconds when this entry expires, or None if it never expires.
    pub expires_at_ms: Option<u64>,
}

/// Parsed options for the SET command (NX/XX/EX/PX).
#[derive(Debug, Clone, Default)]
pub struct SetOptions {
    pub nx: bool,
    pub xx: bool,
    /// Expiry in seconds (EX option).
    pub ex: Option<u64>,
    /// Expiry in milliseconds (PX option).
    pub px: Option<u64>,
}

/// Result of a single Redis command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum RedisResult {
    Ok(String),
    Integer(i64),
    Nil,
    Error(String),
    Array(Vec<RedisResult>),
}

impl RedisResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, RedisResult::Ok(_))
    }
    pub fn is_nil(&self) -> bool {
        matches!(self, RedisResult::Nil)
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RedisResult::Ok(s) => Some(s.as_str()),
            _ => None,
        }
    }
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            RedisResult::Integer(n) => Some(*n),
            _ => None,
        }
    }
}

impl std::fmt::Display for RedisResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedisResult::Ok(s) => write!(f, "{}", s),
            RedisResult::Integer(n) => write!(f, "(integer) {}", n),
            RedisResult::Nil => write!(f, "(nil)"),
            RedisResult::Error(e) => write!(f, "(error) {}", e),
            RedisResult::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    write!(f, "{}) {}", i + 1, item)?;
                    if i + 1 < items.len() {
                        write!(f, "\n")?;
                    }
                }
                Ok(())
            }
        }
    }
}

// =============================================================================
// Module Declarations
// =============================================================================

pub mod commands;
pub mod storage;
pub mod connection;
pub mod cluster;
