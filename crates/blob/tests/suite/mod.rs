// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-blob crate

pub mod config_tests;
pub mod helpers_tests;

// Feature-gated test modules
#[cfg(feature = "sql-backend")]
pub mod integration_tests;
#[cfg(feature = "sql-backend")]
pub mod repository_tests;
#[cfg(feature = "sql-backend")]
pub mod service_tests;

#[cfg(feature = "ddb-backend")]
pub mod ddb_repository_integration_tests;

#[cfg(feature = "server")]
pub mod http_handler_tests;
