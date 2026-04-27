// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Byzantine Generals Example Library
//
// Demonstrates:
// - Application framework (like Erlang)
// - BehaviorRegistry for custom actor types
// - node.spawn() for simplified actor creation
// - Config loading via ConfigBootstrap

pub mod application;
pub mod config;
pub mod general;

pub use application::ByzantineApplication;
pub use config::ByzantineConfig;
pub use general::{General, GeneralMessage, Value, Decision};
