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

//! Outbox relay: drain loop that reads from a channel and sends messages
//! to target actors at AIMD-controlled concurrency.
//!
//! ## Purpose
//! Bridges the outbox channel (durable storage) to actual actor delivery.
//! Dequeues batches, acquires concurrency permits, sends via actor messaging,
//! then ack/nack based on outcome. RESOURCE_EXHAUSTED responses trigger
//! multiplicative decrease on the concurrency limiter.
//!
//! ## Architecture Context
//! Runs as an async task; caller owns the lifecycle via `stop()` + handle await.

use crate::concurrency_limiter::ConcurrencyLimiter;
use plexspaces_channel::Channel;
use plexspaces_common::retry::{compute_backoff, should_retry};
use plexspaces_proto::common::v1::{Message, RetryPolicy};
use plexspaces_proto::outbox::v1::OutboxConfig;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const DEFAULT_RELAY_INTERVAL_MS: u64 = 50;
const DEFAULT_RELAY_BATCH_SIZE: u64 = 100;

/// Callback trait for delivering messages to target actors.
/// Abstracts away the ActorRef/messaging layer for testability.
#[async_trait::async_trait]
pub trait MessageDelivery: Send + Sync {
    /// Deliver a message to its target. Returns Ok on success,
    /// Err with error string on failure. "RESOURCE_EXHAUSTED" triggers backoff.
    async fn deliver(&self, message: &Message) -> Result<(), String>;
}

/// Runtime statistics for the relay.
#[derive(Debug, Default)]
pub struct RelayStats {
    /// Messages successfully delivered.
    pub delivered: AtomicU64,
    /// Messages that failed delivery (after retries).
    pub failed: AtomicU64,
    /// Messages sent to dead-letter queue.
    pub dead_lettered: AtomicU64,
    /// Current number of pending messages in channel.
    pub pending: AtomicU64,
}

/// Outbox relay that drains a channel and delivers messages via AIMD concurrency control.
pub struct OutboxRelay {
    channel: Arc<dyn Channel>,
    limiter: Arc<ConcurrencyLimiter>,
    delivery: Arc<dyn MessageDelivery>,
    retry_policy: RetryPolicy,
    relay_interval: Duration,
    batch_size: u64,
    running: Arc<AtomicBool>,
    stats: Arc<RelayStats>,
}

impl OutboxRelay {
    /// Create a new relay from config and dependencies.
    pub fn new(
        config: &OutboxConfig,
        channel: Arc<dyn Channel>,
        limiter: Arc<ConcurrencyLimiter>,
        delivery: Arc<dyn MessageDelivery>,
    ) -> Self {
        let retry_policy = config.retry_policy.clone().unwrap_or_default();
        let interval_ms = if config.relay_interval_ms == 0 {
            DEFAULT_RELAY_INTERVAL_MS
        } else {
            config.relay_interval_ms
        };
        let batch = if config.relay_batch_size == 0 {
            DEFAULT_RELAY_BATCH_SIZE
        } else {
            config.relay_batch_size
        };
        Self {
            channel,
            limiter,
            delivery,
            retry_policy,
            relay_interval: Duration::from_millis(interval_ms),
            batch_size: batch,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RelayStats::default()),
        }
    }

    /// Get a reference to the relay stats.
    pub fn stats(&self) -> &Arc<RelayStats> {
        &self.stats
    }

    /// Get a reference to the limiter.
    pub fn limiter(&self) -> &Arc<ConcurrencyLimiter> {
        &self.limiter
    }

    /// Check if relay is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// Stop the relay loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    /// Run the relay loop. Call this from a spawned tokio task.
    /// Returns when `stop()` is called.
    pub async fn run(&self) {
        self.running.store(true, Ordering::Release);
        tracing::info!(
            batch_size = self.batch_size,
            interval_ms = self.relay_interval.as_millis() as u64,
            "OutboxRelay started"
        );

        while self.running.load(Ordering::Acquire) {
            let batch_limit = self.limiter.available().min(self.batch_size as u32);
            if batch_limit == 0 {
                sleep(self.relay_interval).await;
                continue;
            }

            match self.channel.try_receive(batch_limit).await {
                Ok(messages) if messages.is_empty() => {
                    self.stats.pending.store(0, Ordering::Release);
                    metrics::gauge!("plexspaces_outbox_relay_pending").set(0.0);
                    sleep(self.relay_interval).await;
                }
                Ok(messages) => {
                    let count = messages.len() as u64;
                    self.stats.pending.store(count, Ordering::Release);
                    metrics::gauge!("plexspaces_outbox_relay_pending").set(count as f64);
                    for msg in messages {
                        self.process_message(msg).await;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "OutboxRelay channel receive error");
                    sleep(self.relay_interval).await;
                }
            }
        }

        tracing::info!("OutboxRelay stopped");
    }

    async fn process_message(&self, msg: Message) {
        let msg_id = msg.id.clone();

        let permit = match self.limiter.try_acquire() {
            Some(p) => p,
            None => {
                if let Err(e) = self.channel.nack(&msg_id, true, None).await {
                    tracing::warn!(msg_id = %msg_id, error = %e, "nack/requeue failed on permit exhaustion");
                }
                return;
            }
        };

        let t0 = std::time::Instant::now();
        match self.delivery.deliver(&msg).await {
            Ok(()) => {
                let elapsed = t0.elapsed().as_secs_f64();
                metrics::histogram!("plexspaces_outbox_relay_delivery_seconds").record(elapsed);
                metrics::counter!("plexspaces_outbox_relay_delivered_total").increment(1);
                self.limiter.on_success(permit);
                self.stats.delivered.fetch_add(1, Ordering::Relaxed);
                if let Err(e) = self.channel.ack(&msg_id).await {
                    tracing::warn!(msg_id = %msg_id, error = %e, "ack failed after delivery");
                }
            }
            Err(err) => {
                let elapsed = t0.elapsed().as_secs_f64();
                metrics::histogram!("plexspaces_outbox_relay_delivery_seconds").record(elapsed);
                let is_exhausted = err.contains(crate::RESOURCE_EXHAUSTED);
                if is_exhausted {
                    metrics::counter!("plexspaces_outbox_relay_backpressure_total").increment(1);
                    self.limiter.on_rejection(permit);
                } else {
                    metrics::counter!("plexspaces_outbox_relay_errors_total").increment(1);
                    self.limiter.on_success(permit);
                }

                let attempt = msg.delivery_count + 1;
                if should_retry(&self.retry_policy, attempt, &err) {
                    let backoff = {
                        let mut rng = rand::thread_rng();
                        compute_backoff(&self.retry_policy, attempt, &mut rng)
                    };
                    metrics::counter!("plexspaces_outbox_relay_retries_total").increment(1);
                    metrics::histogram!("plexspaces_outbox_relay_backoff_seconds")
                        .record(backoff.as_secs_f64());
                    tracing::debug!(
                        msg_id = %msg_id,
                        attempt = attempt,
                        backoff_ms = backoff.as_millis() as u64,
                        "nack with send_at delay"
                    );
                    if let Err(e) = self.channel.nack(&msg_id, true, Some(backoff)).await {
                        tracing::warn!(msg_id = %msg_id, error = %e, "nack/requeue failed");
                    }
                } else {
                    metrics::counter!("plexspaces_outbox_relay_dead_lettered_total").increment(1);
                    self.stats.failed.fetch_add(1, Ordering::Relaxed);
                    self.stats.dead_lettered.fetch_add(1, Ordering::Relaxed);
                    if let Err(e) = self.channel.nack(&msg_id, false, None).await {
                        tracing::warn!(msg_id = %msg_id, error = %e, "nack/dlq failed");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_channel::ChannelResult;
    use plexspaces_proto::channel::v1::ChannelConfig;
    use plexspaces_proto::outbox::v1::ConcurrencyConfig;
    use std::sync::Mutex;
    use tokio::sync::Notify;

    struct MockChannel {
        messages: Mutex<Vec<Message>>,
        acked: Mutex<Vec<String>>,
        nacked: Mutex<Vec<(String, bool)>>,
    }

    impl MockChannel {
        fn new(messages: Vec<Message>) -> Self {
            Self {
                messages: Mutex::new(messages),
                acked: Mutex::new(vec![]),
                nacked: Mutex::new(vec![]),
            }
        }
    }

    #[async_trait::async_trait]
    impl Channel for MockChannel {
        async fn send(&self, _msg: Message) -> ChannelResult<String> {
            Ok("mock".to_string())
        }
        async fn receive(&self, max: u32) -> ChannelResult<Vec<Message>> {
            self.try_receive(max).await
        }
        async fn try_receive(&self, max: u32) -> ChannelResult<Vec<Message>> {
            let mut msgs = self.messages.lock().unwrap();
            let n = (max as usize).min(msgs.len());
            let batch: Vec<_> = msgs.drain(..n).collect();
            Ok(batch)
        }
        async fn subscribe(
            &self,
            _cg: Option<String>,
        ) -> ChannelResult<futures::stream::BoxStream<'static, Message>> {
            unimplemented!()
        }
        async fn publish(&self, _msg: Message) -> ChannelResult<u32> {
            Ok(0)
        }
        async fn ack(&self, msg_id: &str) -> ChannelResult<()> {
            self.acked.lock().unwrap().push(msg_id.to_string());
            Ok(())
        }
        async fn nack(&self, msg_id: &str, requeue: bool, _delay: Option<Duration>) -> ChannelResult<()> {
            self.nacked
                .lock()
                .unwrap()
                .push((msg_id.to_string(), requeue));
            Ok(())
        }
        async fn close(&self) -> ChannelResult<()> {
            Ok(())
        }
        async fn get_stats(&self) -> ChannelResult<plexspaces_proto::channel::v1::ChannelStats> {
            Ok(plexspaces_proto::channel::v1::ChannelStats::default())
        }
        fn is_closed(&self) -> bool {
            false
        }
        fn get_config(&self) -> &ChannelConfig {
            static DEFAULT_CONFIG: std::sync::OnceLock<ChannelConfig> = std::sync::OnceLock::new();
            DEFAULT_CONFIG.get_or_init(ChannelConfig::default)
        }
    }

    struct SuccessDelivery;
    #[async_trait::async_trait]
    impl MessageDelivery for SuccessDelivery {
        async fn deliver(&self, _msg: &Message) -> Result<(), String> {
            Ok(())
        }
    }

    struct FailDelivery {
        error: String,
    }
    #[async_trait::async_trait]
    impl MessageDelivery for FailDelivery {
        async fn deliver(&self, _msg: &Message) -> Result<(), String> {
            Err(self.error.clone())
        }
    }

    struct CountingDelivery {
        count: AtomicU64,
        notify: Notify,
        target: u64,
    }
    #[async_trait::async_trait]
    impl MessageDelivery for CountingDelivery {
        async fn deliver(&self, _msg: &Message) -> Result<(), String> {
            let n = self.count.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= self.target {
                self.notify.notify_one();
            }
            Ok(())
        }
    }

    fn test_message(id: &str) -> Message {
        Message {
            id: id.to_string(),
            ..Default::default()
        }
    }

    fn test_config() -> OutboxConfig {
        OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 100,
                strategy: 0,
            }),
            retry_policy: Some(RetryPolicy {
                max_attempts: 3,
                ..Default::default()
            }),
            relay_interval_ms: 10,
            relay_batch_size: 10,
            storage_channel: None,
        }
    }

    #[tokio::test]
    async fn test_relay_delivers_messages() {
        let msgs = vec![test_message("m1"), test_message("m2"), test_message("m3")];
        let channel = Arc::new(MockChannel::new(msgs));
        let limiter = Arc::new(ConcurrencyLimiter::new(
            &test_config().concurrency.unwrap(),
        ));
        let delivery = Arc::new(CountingDelivery {
            count: AtomicU64::new(0),
            notify: Notify::new(),
            target: 3,
        });

        let relay = OutboxRelay::new(&test_config(), channel.clone(), limiter, delivery.clone());
        let relay_ref = &relay;

        tokio::select! {
            _ = relay_ref.run() => {},
            _ = delivery.notify.notified() => {
                relay.stop();
            },
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                relay.stop();
                panic!("timeout");
            }
        }

        assert_eq!(delivery.count.load(Ordering::SeqCst), 3);
        assert_eq!(relay.stats().delivered.load(Ordering::SeqCst), 3);
        assert_eq!(channel.acked.lock().unwrap().len(), 3);
    }

    struct NotifyOnFail {
        error: String,
        notify: Notify,
    }
    #[async_trait::async_trait]
    impl MessageDelivery for NotifyOnFail {
        async fn deliver(&self, _msg: &Message) -> Result<(), String> {
            self.notify.notify_one();
            Err(self.error.clone())
        }
    }

    #[tokio::test]
    async fn test_relay_nacks_on_failure() {
        let msgs = vec![test_message("fail1")];
        let channel = Arc::new(MockChannel::new(msgs));
        let limiter = Arc::new(ConcurrencyLimiter::new(
            &test_config().concurrency.unwrap(),
        ));
        let delivery = Arc::new(NotifyOnFail {
            error: "INTERNAL_ERROR".to_string(),
            notify: Notify::new(),
        });

        let config = OutboxConfig {
            retry_policy: Some(RetryPolicy {
                max_attempts: 1,
                ..Default::default()
            }),
            relay_interval_ms: 10,
            relay_batch_size: 10,
            ..Default::default()
        };
        let relay = OutboxRelay::new(&config, channel.clone(), limiter, delivery.clone());

        tokio::select! {
            _ = relay.run() => {},
            _ = delivery.notify.notified() => {
                tokio::task::yield_now().await;
                relay.stop();
            },
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                relay.stop();
                panic!("timeout");
            }
        }

        assert_eq!(relay.stats().failed.load(Ordering::SeqCst), 1);
        let nacked = channel.nacked.lock().unwrap();
        assert_eq!(nacked.len(), 1);
        assert!(!nacked[0].1);
    }

    #[tokio::test]
    async fn test_relay_resource_exhausted_decreases_limit() {
        let msgs = vec![test_message("ex1")];
        let channel = Arc::new(MockChannel::new(msgs));
        let config = test_config();
        let limiter = Arc::new(ConcurrencyLimiter::new(config.concurrency.as_ref().unwrap()));
        let initial_limit = limiter.current_limit();
        let delivery = Arc::new(NotifyOnFail {
            error: "RESOURCE_EXHAUSTED".to_string(),
            notify: Notify::new(),
        });

        let relay = OutboxRelay::new(&config, channel.clone(), limiter.clone(), delivery.clone());

        tokio::select! {
            _ = relay.run() => {},
            _ = delivery.notify.notified() => {
                tokio::task::yield_now().await;
                relay.stop();
            },
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                relay.stop();
                panic!("timeout");
            }
        }

        assert!(limiter.current_limit() < initial_limit);
    }

    #[tokio::test]
    async fn test_relay_stop() {
        let channel = Arc::new(MockChannel::new(vec![]));
        let limiter = Arc::new(ConcurrencyLimiter::with_defaults());
        let delivery = Arc::new(SuccessDelivery);

        let relay = Arc::new(OutboxRelay::new(
            &test_config(),
            channel,
            limiter,
            delivery,
        ));

        assert!(!relay.is_running());
        let relay_clone = relay.clone();
        let handle = tokio::spawn(async move { relay_clone.run().await });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert!(relay.is_running());
        relay.stop();
        handle.await.unwrap();
        assert!(!relay.is_running());
    }

    #[tokio::test]
    async fn test_relay_empty_channel_idles() {
        let channel = Arc::new(MockChannel::new(vec![]));
        let limiter = Arc::new(ConcurrencyLimiter::with_defaults());
        let delivery = Arc::new(SuccessDelivery);

        let relay = Arc::new(OutboxRelay::new(
            &test_config(),
            channel,
            limiter,
            delivery,
        ));

        let relay_clone = relay.clone();
        let handle = tokio::spawn(async move { relay_clone.run().await });
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        assert!(relay.is_running());
        assert_eq!(relay.stats().delivered.load(Ordering::SeqCst), 0);
        relay.stop();
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_relay_nacks_on_permit_exhaustion() {
        let msgs = vec![test_message("p1"), test_message("p2"), test_message("p3")];
        let channel = Arc::new(MockChannel::new(msgs));
        let config_with_limit_1 = ConcurrencyConfig {
            initial_limit: 1,
            min_limit: 1,
            max_limit: 1,
            strategy: 1, // fixed
        };
        let limiter = Arc::new(ConcurrencyLimiter::new(&config_with_limit_1));
        let delivery = Arc::new(CountingDelivery {
            count: AtomicU64::new(0),
            notify: Notify::new(),
            target: 3,
        });

        let config = OutboxConfig {
            concurrency: Some(config_with_limit_1),
            retry_policy: Some(RetryPolicy {
                max_attempts: 3,
                ..Default::default()
            }),
            relay_interval_ms: 10,
            relay_batch_size: 10,
            ..Default::default()
        };
        let relay = OutboxRelay::new(&config, channel.clone(), limiter, delivery.clone());

        tokio::select! {
            _ = relay.run() => {},
            _ = delivery.notify.notified() => {
                relay.stop();
            },
            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                relay.stop();
                panic!("timeout");
            }
        }

        assert_eq!(delivery.count.load(Ordering::SeqCst), 3);
        let nacked = channel.nacked.lock().unwrap();
        assert!(nacked.iter().all(|(_, requeue)| *requeue));
    }
}
