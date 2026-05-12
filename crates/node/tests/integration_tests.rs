// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Main integration test entry point for plexspaces-node
// This file imports all test modules from suite/ directory
// Compiles into a single test binary instead of 31 separate binaries

mod suite;
mod validate_toml_facet_parsing;
