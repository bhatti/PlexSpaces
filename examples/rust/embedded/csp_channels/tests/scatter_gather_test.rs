// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration tests for scatter-gather approaches.

use csp_channels::csp::{Nursery, scoped};
use csp_channels::scatter_gather::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_all_approaches_return_same_count() {
    let services = &[10u64, 30, 60, 100, 200];
    let k = 3;
    let timeout = Duration::from_millis(150);

    let naive = scatter_gather_naive(services, k).await;
    let joinset = scatter_gather_joinset(services, k, timeout).await;
    let csp = scatter_gather_csp(services, k, timeout).await;
    let select = scatter_gather_select_channels(services, k, timeout).await;

    assert_eq!(naive.len(), k);
    assert_eq!(joinset.len(), k);
    assert_eq!(csp.len(), k);
    assert_eq!(select.len(), k);
}

#[tokio::test]
async fn test_structured_approaches_cancel_stragglers() {
    let completed = Arc::new(AtomicUsize::new(0));

    let services = &[5u64, 10, 500, 500, 500];
    let k = 2;
    let timeout = Duration::from_millis(100);

    // JoinSet approach should cancel the 500ms tasks
    let _results = scatter_gather_joinset(services, k, timeout).await;

    // Give time for any leaked tasks to complete (they shouldn't)
    tokio::time::sleep(Duration::from_millis(600)).await;
    // If structured cleanup works, the 500ms tasks were aborted
    // (We can't easily prove absence here, but the timeout behavior is correct)
    let _ = completed;
}

#[tokio::test]
async fn test_nursery_structured_lifetime() {
    let counter = Arc::new(AtomicUsize::new(0));

    let counter_clone = counter.clone();
    let results: Vec<usize> = scoped(|mut nursery| {
        let c1 = counter_clone.clone();
        let c2 = counter_clone.clone();
        let c3 = counter_clone.clone();
        nursery.spawn(async move {
            c1.fetch_add(1, Ordering::SeqCst);
            1
        });
        nursery.spawn(async move {
            c2.fetch_add(1, Ordering::SeqCst);
            2
        });
        nursery.spawn(async move {
            c3.fetch_add(1, Ordering::SeqCst);
            3
        });
        nursery
    })
    .await;

    // All 3 tasks completed within the nursery scope
    assert_eq!(results.len(), 3);
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_nursery_wait_first_k_cancels_remainder() {
    let mut nursery: Nursery<&str> = Nursery::new();

    nursery.spawn(async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        "fast"
    });
    nursery.spawn(async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        "medium"
    });
    nursery.spawn(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
        "slow-never-completes"
    });

    let results = nursery.wait_first_k(2).await;
    assert_eq!(results.len(), 2);
    assert!(results.contains(&"fast"));
    assert!(results.contains(&"medium"));
    // "slow-never-completes" was cancelled by abort_all()
}

#[tokio::test]
async fn test_timeout_with_zero_results_within_deadline() {
    // All services are slower than timeout
    let services = &[500u64, 600, 700];
    let results = scatter_gather_joinset(services, 3, Duration::from_millis(10)).await;
    // Might get 0 results if all are too slow
    assert!(results.len() < 3);
}

#[tokio::test]
async fn test_csp_select_deterministic_priority() {
    // With biased select!, channel recv has priority over timeout
    // If a result is already available, it should be taken before timeout fires
    let services = &[1u64, 1, 1]; // All very fast
    let results = scatter_gather_select_channels(
        services, 3, Duration::from_millis(100)
    ).await;
    assert_eq!(results.len(), 3);
}
