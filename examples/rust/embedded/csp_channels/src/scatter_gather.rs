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

//! Scatter-gather pattern implemented three ways:
//! 1. Naive — fire-and-forget spawns (leaks tasks on early return)
//! 2. JoinSet + timeout — proper structured lifetime
//! 3. CSP channels + select! — Hoare-style guarded commands

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::csp::Nursery;

/// Simulates a service call that takes variable time.
async fn simulate_service(id: usize, latency_ms: u64) -> ServiceResponse {
    tokio::time::sleep(Duration::from_millis(latency_ms)).await;
    ServiceResponse {
        service_id: id,
        data: format!("result-from-service-{}", id),
        latency_ms,
    }
}

#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub service_id: usize,
    pub data: String,
    pub latency_ms: u64,
}

// ---------------------------------------------------------------------------
// Approach 1: NAIVE — fire-and-forget (BROKEN — leaks tasks)
// ---------------------------------------------------------------------------

/// Scatter-gather with no structured lifetime.
/// BUG: spawned tasks leak if we return early (after collecting K results).
/// The orphaned tasks continue running, consuming resources.
pub async fn scatter_gather_naive(
    services: &[u64],
    first_k: usize,
) -> Vec<ServiceResponse> {
    let (tx, mut rx) = mpsc::channel(services.len());

    // Fire-and-forget spawns — no handle retained, no cancellation path
    for (id, &latency) in services.iter().enumerate() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let response = simulate_service(id, latency).await;
            let _ = tx.send(response).await;
        });
    }
    drop(tx); // Close our copy so channel drains properly

    // Collect first K results — remaining tasks are LEAKED
    let mut results = Vec::with_capacity(first_k);
    while results.len() < first_k {
        match rx.recv().await {
            Some(resp) => results.push(resp),
            None => break,
        }
    }
    // BUG: N - K tasks still running in background with no way to cancel
    results
}

// ---------------------------------------------------------------------------
// Approach 2: JoinSet + timeout — structured lifetime
// ---------------------------------------------------------------------------

/// Scatter-gather with structured concurrency via JoinSet.
/// All tasks are cancelled when the scope exits — no leaks.
pub async fn scatter_gather_joinset(
    services: &[u64],
    first_k: usize,
    timeout: Duration,
) -> Vec<ServiceResponse> {
    let mut set = JoinSet::new();

    for (id, &latency) in services.iter().enumerate() {
        set.spawn(async move {
            simulate_service(id, latency).await
        });
    }

    let mut results = Vec::with_capacity(first_k);
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        if results.len() >= first_k {
            break;
        }
        tokio::select! {
            maybe = set.join_next() => {
                match maybe {
                    Some(Ok(resp)) => results.push(resp),
                    Some(Err(_)) => continue,
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    // JoinSet::drop cancels all remaining tasks — structured cleanup
    set.abort_all();
    results
}

// ---------------------------------------------------------------------------
// Approach 3: CSP-style — rendezvous channels + Nursery
// ---------------------------------------------------------------------------

/// Scatter-gather using CSP-style rendezvous channels and a Nursery.
/// Each service gets its own channel; select! acts as guarded command.
pub async fn scatter_gather_csp(
    services: &[u64],
    first_k: usize,
    timeout: Duration,
) -> Vec<ServiceResponse> {
    let mut nursery: Nursery<ServiceResponse> = Nursery::new();

    for (id, &latency) in services.iter().enumerate() {
        nursery.spawn(async move {
            simulate_service(id, latency).await
        });
    }

    nursery.wait_first_k_or_timeout(first_k, timeout).await
}

/// Demonstrates CSP-style select! as a guarded command over multiple channels.
/// Each channel represents a distinct communication event (Hoare's alphabet).
pub async fn scatter_gather_select_channels(
    services: &[u64],
    first_k: usize,
    timeout: Duration,
) -> Vec<ServiceResponse> {
    let (tx, mut rx) = mpsc::channel::<ServiceResponse>(1);

    let mut set = JoinSet::new();
    for (id, &latency) in services.iter().enumerate() {
        let tx = tx.clone();
        set.spawn(async move {
            let resp = simulate_service(id, latency).await;
            // Rendezvous-like: small buffer means sender may block
            let _ = tx.send(resp).await;
        });
    }
    drop(tx);

    let mut results = Vec::with_capacity(first_k);
    let deadline = tokio::time::Instant::now() + timeout;

    // CSP guarded command: select over channel recv OR timeout
    loop {
        if results.len() >= first_k {
            break;
        }
        tokio::select! {
            biased; // Deterministic priority — like CSP external choice with ordering

            msg = rx.recv() => {
                match msg {
                    Some(resp) => results.push(resp),
                    None => break,
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                break;
            }
        }
    }

    // Structured cleanup — abort all spawned tasks
    set.abort_all();
    // Drain any remaining messages
    drop(rx);
    // Wait for JoinSet to finish aborting
    while set.join_next().await.is_some() {}

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    // Service latencies in ms — 5 services, varying speeds
    const SERVICES: &[u64] = &[10, 50, 200, 500, 1000];

    #[tokio::test]
    async fn test_naive_returns_first_k() {
        let results = scatter_gather_naive(SERVICES, 3).await;
        assert_eq!(results.len(), 3);
        // Results should be the fastest 3
        for r in &results {
            assert!(r.latency_ms <= 200);
        }
    }

    #[tokio::test]
    async fn test_joinset_structured() {
        let results = scatter_gather_joinset(
            SERVICES, 3, Duration::from_millis(300)
        ).await;
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_joinset_timeout_limits_results() {
        // Very short timeout — might not get all K
        let results = scatter_gather_joinset(
            SERVICES, 5, Duration::from_millis(30)
        ).await;
        // Should get at least 1 (10ms service) but not all 5
        assert!(results.len() >= 1);
        assert!(results.len() < 5);
    }

    #[tokio::test]
    async fn test_csp_nursery() {
        let results = scatter_gather_csp(
            SERVICES, 3, Duration::from_millis(300)
        ).await;
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_csp_select_channels() {
        let results = scatter_gather_select_channels(
            SERVICES, 3, Duration::from_millis(300)
        ).await;
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_csp_select_timeout() {
        let results = scatter_gather_select_channels(
            SERVICES, 5, Duration::from_millis(30)
        ).await;
        assert!(results.len() >= 1);
        assert!(results.len() < 5);
    }
}
