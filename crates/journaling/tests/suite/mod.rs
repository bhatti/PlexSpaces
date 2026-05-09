// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-journaling crate


pub mod actor_replay_integration;
pub mod checkpoint_recovery;
pub mod ddb_integration_tests;
pub mod debug_tests;
pub mod deterministic_replay;
pub mod durable_promises;
pub mod event_sourcing_integration;
pub mod event_sourcing_sql_integration;
pub mod persistence_integration;
pub mod postgres_replay_integration;
pub mod reminder_facet_tests;
pub mod reminder_sql_integration;
pub mod replay_integration;
pub mod replay_isolation;
pub mod side_effect_caching;
pub mod sqlite_persistence;
pub mod memoize_facet_integration;
pub mod timer_facet_distributed_locks;
pub mod timer_facet_tests;
pub mod truncation_tests;
