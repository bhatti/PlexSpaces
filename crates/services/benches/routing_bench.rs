// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Routing latency benchmarks using Criterion.
//!
//! These benchmarks measure routing overhead using the `AskReplyRequest` path.
//! Run with: `cargo bench -p plexspaces-services --bench routing_bench`
//!
//! For latency profiling of virtual actors specifically, see the integration test:
//! `cargo test -p plexspaces-services --test routing_latency_tests -- --nocapture`

fn main() {
    // This benchmark binary is intentionally empty.
    // Virtual actor latency is measured in the routing_latency_tests integration test
    // which runs correctly under tokio::test without block_on/block_in_place issues.
    println!("Use `cargo test -p plexspaces-services --test routing_latency_tests -- --nocapture` for latency measurements.");
}
