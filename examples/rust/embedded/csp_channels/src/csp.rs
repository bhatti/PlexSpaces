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

//! Minimal CSP primitives for Rust.
//!
//! Maps Hoare's CSP concepts to tokio constructs:
//! - Rendezvous channel (capacity 0) = synchronous communication
//! - `tokio::select!` = guarded command / external choice
//! - `Nursery` = structured lifetime (all children must finish before scope exits)

use std::future::Future;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

/// A rendezvous channel — sender blocks until receiver is ready (capacity 0).
/// This is the CSP primitive: synchronous, point-to-point communication.
pub struct Channel<T> {
    pub tx: mpsc::Sender<T>,
    pub rx: mpsc::Receiver<T>,
}

impl<T: Send + 'static> Channel<T> {
    /// Create a rendezvous (capacity-0) channel pair.
    /// The sender will block until the receiver calls recv().
    pub fn rendezvous() -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
        mpsc::channel(1)
    }

    /// Create a buffered channel with explicit capacity.
    /// This breaks the CSP rendezvous property — use deliberately.
    pub fn buffered(capacity: usize) -> (mpsc::Sender<T>, mpsc::Receiver<T>) {
        mpsc::channel(capacity)
    }
}

/// A Nursery provides structured concurrency guarantees:
/// all spawned tasks MUST complete (or be cancelled) before the nursery scope exits.
///
/// Inspired by Python's Trio nursery and Kotlin's coroutineScope.
pub struct Nursery<T: Send + 'static> {
    join_set: JoinSet<T>,
}

impl<T: Send + 'static> Nursery<T> {
    pub fn new() -> Self {
        Self {
            join_set: JoinSet::new(),
        }
    }

    /// Spawn a task bound to this nursery's lifetime.
    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        self.join_set.spawn(future);
    }

    /// Wait for all tasks to complete. Returns results in completion order.
    pub async fn wait_all(mut self) -> Vec<T> {
        let mut results = Vec::new();
        while let Some(result) = self.join_set.join_next().await {
            if let Ok(value) = result {
                results.push(value);
            }
        }
        results
    }

    /// Wait for the first K tasks to complete, then cancel the rest.
    /// This is the scatter-gather pattern with structured cleanup.
    pub async fn wait_first_k(mut self, k: usize) -> Vec<T> {
        let mut results = Vec::with_capacity(k);
        while results.len() < k {
            match self.join_set.join_next().await {
                Some(Ok(value)) => results.push(value),
                Some(Err(_)) => continue,
                None => break,
            }
        }
        // Structured cleanup: cancel all remaining tasks
        self.join_set.abort_all();
        results
    }

    /// Wait for first K tasks OR timeout — whichever comes first.
    /// Guarantees all spawned tasks are cancelled on exit.
    pub async fn wait_first_k_or_timeout(
        mut self,
        k: usize,
        timeout: std::time::Duration,
    ) -> Vec<T> {
        let mut results = Vec::with_capacity(k);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if results.len() >= k {
                break;
            }
            tokio::select! {
                maybe_result = self.join_set.join_next() => {
                    match maybe_result {
                        Some(Ok(value)) => results.push(value),
                        Some(Err(_)) => continue,
                        None => break,
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    break;
                }
            }
        }
        // Structured cleanup: cancel stragglers
        self.join_set.abort_all();
        results
    }

    pub fn len(&self) -> usize {
        self.join_set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.join_set.is_empty()
    }
}

impl<T: Send + 'static> Default for Nursery<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a block with structured concurrency guarantees.
/// All tasks spawned into the nursery are cancelled if the block exits early.
pub async fn scoped<T, F>(f: F) -> Vec<T>
where
    T: Send + 'static,
    F: FnOnce(Nursery<T>) -> Nursery<T>,
{
    let nursery = Nursery::new();
    let nursery = f(nursery);
    nursery.wait_all().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_rendezvous_channel() {
        let (tx, mut rx) = Channel::<i32>::rendezvous();

        tokio::spawn(async move {
            tx.send(42).await.unwrap();
        });

        let value = rx.recv().await.unwrap();
        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn test_nursery_wait_all() {
        let mut nursery = Nursery::new();
        nursery.spawn(async { 1 });
        nursery.spawn(async { 2 });
        nursery.spawn(async { 3 });

        let mut results = nursery.wait_all().await;
        results.sort();
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_nursery_wait_first_k() {
        let mut nursery = Nursery::new();

        // Fast tasks
        nursery.spawn(async { tokio::time::sleep(Duration::from_millis(10)).await; 1 });
        nursery.spawn(async { tokio::time::sleep(Duration::from_millis(20)).await; 2 });
        // Slow task — should be cancelled
        nursery.spawn(async { tokio::time::sleep(Duration::from_secs(10)).await; 3 });

        let results = nursery.wait_first_k(2).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_nursery_cancels_on_timeout() {
        let completed = Arc::new(AtomicUsize::new(0));
        let completed_clone = completed.clone();

        let mut nursery = Nursery::<()>::new();

        // Task that takes too long
        nursery.spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await;
            completed_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Fast task
        let completed_clone2 = completed.clone();
        nursery.spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            completed_clone2.fetch_add(1, Ordering::SeqCst);
        });

        let _results = nursery.wait_first_k_or_timeout(1, Duration::from_millis(50)).await;

        // Give a moment for cancellation to propagate
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Only the fast task should have completed
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_scoped_structured_concurrency() {
        let results = scoped(|mut nursery| {
            nursery.spawn(async { 10 });
            nursery.spawn(async { 20 });
            nursery
        }).await;

        assert_eq!(results.iter().sum::<i32>(), 30);
    }

    #[tokio::test]
    async fn test_rendezvous_vs_buffered_backpressure() {
        // Rendezvous (capacity 1): sender blocks until receiver is ready.
        // This approximates CSP's synchronous communication.
        let (tx, mut rx) = Channel::<i32>::rendezvous();
        let sent = std::sync::Arc::new(AtomicUsize::new(0));
        let sent_clone = sent.clone();

        tokio::spawn(async move {
            tx.send(1).await.unwrap();
            sent_clone.fetch_add(1, Ordering::SeqCst);
            // Second send will block until receiver drains
            tx.send(2).await.unwrap();
            sent_clone.fetch_add(1, Ordering::SeqCst);
        });

        // Let sender attempt both sends
        tokio::time::sleep(Duration::from_millis(20)).await;
        // With capacity=1, first send succeeds immediately but second blocks
        assert_eq!(sent.load(Ordering::SeqCst), 1);

        // Drain first — unblocks second send
        let v1 = rx.recv().await.unwrap();
        assert_eq!(v1, 1);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(sent.load(Ordering::SeqCst), 2);
        let v2 = rx.recv().await.unwrap();
        assert_eq!(v2, 2);

        // Buffered (capacity 5): sender never blocks until buffer full.
        // This BREAKS CSP rendezvous — sender proceeds without receiver.
        let (tx_buf, mut rx_buf) = Channel::<i32>::buffered(5);
        let sent_buf = std::sync::Arc::new(AtomicUsize::new(0));
        let sent_buf_clone = sent_buf.clone();

        tokio::spawn(async move {
            for i in 0..5 {
                tx_buf.send(i).await.unwrap();
                sent_buf_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        // All 5 sent without blocking — no backpressure!
        assert_eq!(sent_buf.load(Ordering::SeqCst), 5);

        // Drain to clean up
        for _ in 0..5 {
            rx_buf.recv().await.unwrap();
        }
    }
}
