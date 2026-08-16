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

//! Pure Rust CSP-Style Structured Concurrency Demo
//!
//! Demonstrates the scatter-gather pattern (fan out to N services, collect
//! first K responses within a timeout, cancel stragglers) using three approaches:
//! 1. Naive fire-and-forget (broken — leaks tasks)
//! 2. JoinSet + timeout (structured lifetime)
//! 3. CSP-style channels with select! (Hoare guarded commands)

use csp_channels::scatter_gather::*;
use std::time::{Duration, Instant};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // 5 services with latencies: 10ms, 50ms, 200ms, 500ms, 1000ms
    let services = vec![10u64, 50, 200, 500, 1000];
    let first_k = 3;
    let timeout = Duration::from_millis(300);

    info!("=== Scatter-Gather: request {} services, want first {} within {:?} ===\n",
        services.len(), first_k, timeout);

    // --- Approach 1: Naive (fire-and-forget) ---
    info!("--- Approach 1: Naive (fire-and-forget, LEAKS tasks) ---");
    let start = Instant::now();
    let results = scatter_gather_naive(&services, first_k).await;
    info!("  Got {} results in {:?}", results.len(), start.elapsed());
    for r in &results {
        info!("    service-{}: {} ({}ms)", r.service_id, r.data, r.latency_ms);
    }
    info!("  WARNING: {} tasks leaked (still running in background)\n",
        services.len() - results.len());

    // --- Approach 2: JoinSet + timeout (structured) ---
    info!("--- Approach 2: JoinSet + timeout (structured lifetime) ---");
    let start = Instant::now();
    let results = scatter_gather_joinset(&services, first_k, timeout).await;
    info!("  Got {} results in {:?}", results.len(), start.elapsed());
    for r in &results {
        info!("    service-{}: {} ({}ms)", r.service_id, r.data, r.latency_ms);
    }
    info!("  All remaining tasks cancelled on scope exit\n");

    // --- Approach 3: CSP channels + select! ---
    info!("--- Approach 3: CSP-style channels + select! (guarded commands) ---");
    let start = Instant::now();
    let results = scatter_gather_csp(&services, first_k, timeout).await;
    info!("  Got {} results in {:?}", results.len(), start.elapsed());
    for r in &results {
        info!("    service-{}: {} ({}ms)", r.service_id, r.data, r.latency_ms);
    }
    info!("  Nursery guarantees: all children cancelled before scope exits\n");

    // --- Approach 3b: Explicit select! over channels ---
    info!("--- Approach 3b: Explicit select! over channel (CSP guarded command) ---");
    let start = Instant::now();
    let results = scatter_gather_select_channels(&services, first_k, timeout).await;
    info!("  Got {} results in {:?}", results.len(), start.elapsed());
    for r in &results {
        info!("    service-{}: {} ({}ms)", r.service_id, r.data, r.latency_ms);
    }
    info!("  biased select! gives deterministic priority (like CSP external choice)\n");

    info!("=== Done ===");
    Ok(())
}
