// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Partitioned outbox relay for scalable high-fan-out dispatch.
//!
//! Shards messages across N independent partitions using `hash(target_actor_id) % N`.
//! Each partition has its own channel, AIMD concurrency limiter, and relay task.
//! Same target always routes to the same partition, preserving per-recipient ordering.
//!
//! ## Scaling
//! Unlike Kafka partitions, these channels are transient queues — messages flow through
//! in milliseconds, not days. To change partition count: create a new relay with the
//! desired N, drain the old one, switch over. No data movement or rebalancing.

use crate::hash_router::HashRouter;
use crate::outbox_relay::{MessageDelivery, OutboxRelay};
use crate::ConcurrencyLimiter;
use plexspaces_channel::{Channel, ChannelResult, InMemoryChannel};
use plexspaces_proto::channel::v1::{
    ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::outbox::v1::{
    DispatcherStatus, PartitionedDispatcherStatus, PartitionedOutboxConfig,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// A partitioned outbox relay that distributes messages across N independent partitions.
pub struct PartitionedOutboxRelay {
    router: HashRouter,
    partitions: Vec<Partition>,
    dispatcher_id: String,
}

struct Partition {
    channel: Arc<dyn Channel>,
    limiter: Arc<ConcurrencyLimiter>,
    relay: Arc<OutboxRelay>,
}

impl PartitionedOutboxRelay {
    /// Create a new partitioned relay with in-memory channels per partition.
    /// For durable partitions, compose N individual `OutboxRelay` instances
    /// with injected DB-backed channels and a `HashRouter` for routing.
    pub async fn new(
        dispatcher_id: &str,
        config: &PartitionedOutboxConfig,
        delivery: Arc<dyn MessageDelivery>,
    ) -> ChannelResult<Self> {
        assert!(config.num_partitions > 0, "num_partitions must be >= 1 (proto contract)");
        let n = config.num_partitions;
        let per_partition = config.per_partition.clone().unwrap_or_default();

        let mut partitions = Vec::with_capacity(n as usize);
        for i in 0..n {
            let channel_config = ChannelConfig {
                name: format!("{}-partition-{}", dispatcher_id, i),
                provider: ChannelProvider::ChannelProviderInMemory as i32,
                capacity: per_partition.relay_batch_size.max(100) * 10,
                delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
                ..Default::default()
            };
            let channel: Arc<dyn Channel> = Arc::new(InMemoryChannel::new(channel_config).await?);
            let limiter = Arc::new(ConcurrencyLimiter::new(
                per_partition.concurrency.as_ref().unwrap_or(&Default::default()),
            ));
            let relay = Arc::new(OutboxRelay::new(
                &per_partition,
                channel.clone(),
                limiter.clone(),
                delivery.clone(),
            ));
            partitions.push(Partition { channel, limiter, relay });
        }

        Ok(Self {
            router: HashRouter::new(n),
            partitions,
            dispatcher_id: dispatcher_id.to_string(),
        })
    }

    /// Send a message targeted at a specific actor. Routes to the correct partition.
    pub async fn send(&self, target_actor_id: &str, msg: Message) -> ChannelResult<String> {
        let idx = self.router.route(target_actor_id) as usize;
        self.partitions[idx].channel.send(msg).await
    }

    /// Start all relay tasks. Returns join handles for the caller to await.
    /// Caller MUST call `stop()` and await all handles before dropping.
    pub fn run(&self) -> Vec<JoinHandle<()>> {
        self.partitions
            .iter()
            .map(|p| {
                let relay = p.relay.clone();
                tokio::spawn(async move { relay.run().await })
            })
            .collect()
    }

    /// Stop all relays.
    pub fn stop(&self) {
        for p in &self.partitions {
            p.relay.stop();
        }
    }

    /// Number of partitions.
    pub fn num_partitions(&self) -> u32 {
        self.router.num_partitions()
    }

    /// Aggregate status across all partitions.
    pub fn status(&self) -> PartitionedDispatcherStatus {
        let mut total_delivered = 0u64;
        let mut total_failed = 0u64;
        let mut total_pending = 0u64;
        let mut partition_statuses = Vec::with_capacity(self.partitions.len());

        for (i, p) in self.partitions.iter().enumerate() {
            let stats = p.relay.stats();
            let delivered = stats.delivered.load(Ordering::Relaxed);
            let failed = stats.failed.load(Ordering::Relaxed);
            let dead_lettered = stats.dead_lettered.load(Ordering::Relaxed);
            let pending = stats.pending.load(Ordering::Relaxed);

            total_delivered += delivered;
            total_failed += failed;
            total_pending += pending;

            let total = delivered + failed;
            let success_rate = if total > 0 { delivered as f64 / total as f64 } else { 1.0 };

            partition_statuses.push(DispatcherStatus {
                dispatcher_id: format!("{}-partition-{}", self.dispatcher_id, i),
                pending_count: pending,
                in_flight_count: p.limiter.in_flight() as u64,
                delivered_count: delivered,
                failed_count: failed,
                dead_letter_count: dead_lettered,
                current_concurrency_limit: p.limiter.current_limit(),
                success_rate,
            });
        }

        let grand_total = total_delivered + total_failed;
        let aggregate_success_rate = if grand_total > 0 {
            total_delivered as f64 / grand_total as f64
        } else {
            1.0
        };

        PartitionedDispatcherStatus {
            dispatcher_id: self.dispatcher_id.clone(),
            num_partitions: self.partitions.len() as u32,
            partition_statuses,
            total_delivered,
            total_failed,
            total_pending,
            aggregate_success_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageDelivery;
    use plexspaces_proto::outbox::v1::OutboxConfig;
    use std::sync::atomic::AtomicU64;
    use tokio::sync::Notify;

    struct TestDelivery {
        delivered: AtomicU64,
        notify: Notify,
        target: u64,
    }

    #[async_trait::async_trait]
    impl MessageDelivery for TestDelivery {
        async fn deliver(&self, _msg: &Message) -> Result<(), crate::outbox_relay::DeliveryError> {
            let n = self.delivered.fetch_add(1, Ordering::SeqCst) + 1;
            if n >= self.target {
                self.notify.notify_one();
            }
            Ok(())
        }
    }

    fn test_config(num_partitions: u32) -> PartitionedOutboxConfig {
        PartitionedOutboxConfig {
            num_partitions,
            per_partition: Some(OutboxConfig {
                concurrency: Some(plexspaces_proto::outbox::v1::ConcurrencyConfig {
                    initial_limit: 10,
                    min_limit: 1,
                    max_limit: 50,
                    strategy: 0,
                }),
                retry_policy: Some(plexspaces_common::retry::default_retry_policy()),
                relay_interval_ms: 5,
                relay_batch_size: 20,
                storage_channel: None,
            }),
            hash_strategy: 0,
        }
    }

    #[tokio::test]
    async fn test_all_messages_delivered() {
        let total = 100u64;
        let delivery = Arc::new(TestDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target: total,
        });

        let relay = PartitionedOutboxRelay::new("test", &test_config(4), delivery.clone())
            .await
            .unwrap();

        for i in 0..total {
            let target = format!("actor-{}", i % 20);
            let msg = plexspaces_sdk_test_msg(i);
            relay.send(&target, msg).await.unwrap();
        }

        let handles = relay.run();

        tokio::select! {
            _ = delivery.notify.notified() => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("timeout: {}/{} delivered", delivery.delivered.load(Ordering::SeqCst), total);
            }
        }

        relay.stop();
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(delivery.delivered.load(Ordering::SeqCst), total);
    }

    #[tokio::test]
    async fn test_status_aggregation() {
        let delivery = Arc::new(TestDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target: 50,
        });

        let relay = PartitionedOutboxRelay::new("status-test", &test_config(4), delivery.clone())
            .await
            .unwrap();

        for i in 0..50u64 {
            let target = format!("actor-{}", i);
            relay.send(&target, plexspaces_sdk_test_msg(i)).await.unwrap();
        }

        let handles = relay.run();

        tokio::select! {
            _ = delivery.notify.notified() => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("timeout");
            }
        }

        relay.stop();
        for h in handles {
            h.await.unwrap();
        }

        let status = relay.status();
        assert_eq!(status.num_partitions, 4);
        assert_eq!(status.partition_statuses.len(), 4);
        assert_eq!(status.total_delivered, 50);
        assert_eq!(status.total_failed, 0);
        assert!((status.aggregate_success_rate - 1.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn test_same_target_same_partition() {
        let delivery = Arc::new(TestDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target: 10,
        });

        let relay = PartitionedOutboxRelay::new("order-test", &test_config(8), delivery.clone())
            .await
            .unwrap();

        let target = "actor-fixed-target";
        for i in 0..10u64 {
            relay.send(target, plexspaces_sdk_test_msg(i)).await.unwrap();
        }

        let handles = relay.run();

        tokio::select! {
            _ = delivery.notify.notified() => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("timeout");
            }
        }

        relay.stop();
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(delivery.delivered.load(Ordering::SeqCst), 10);

        let status = relay.status();
        let active: Vec<_> = status.partition_statuses.iter()
            .filter(|s| s.delivered_count > 0)
            .collect();
        assert_eq!(active.len(), 1, "all messages for same target should go to exactly one partition");
        assert_eq!(active[0].delivered_count, 10);
    }

    #[tokio::test]
    #[should_panic(expected = "num_partitions must be >= 1")]
    async fn test_zero_partitions_panics() {
        let delivery = Arc::new(TestDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target: u64::MAX,
        });

        let config = PartitionedOutboxConfig {
            num_partitions: 0,
            per_partition: Some(OutboxConfig::default()),
            hash_strategy: 0,
        };

        let _ = PartitionedOutboxRelay::new("zero-test", &config, delivery).await;
    }

    #[tokio::test]
    async fn test_failure_isolated_to_partition() {
        use std::sync::atomic::AtomicBool;

        struct PartialFailDelivery {
            fail_flag: AtomicBool,
            delivered: AtomicU64,
            failed: AtomicU64,
            notify: Notify,
            target: u64,
        }

        #[async_trait::async_trait]
        impl MessageDelivery for PartialFailDelivery {
            async fn deliver(&self, msg: &Message) -> Result<(), crate::outbox_relay::DeliveryError> {
                if self.fail_flag.load(Ordering::SeqCst) && msg.id.contains("fail") {
                    self.failed.fetch_add(1, Ordering::SeqCst);
                    return Err(crate::outbox_relay::DeliveryError::Capacity {
                        message: "mailbox full".to_string(),
                        retry_after_ms: 100,
                    });
                }
                let n = self.delivered.fetch_add(1, Ordering::SeqCst) + 1;
                if n >= self.target {
                    self.notify.notify_one();
                }
                Ok(())
            }
        }

        let delivery = Arc::new(PartialFailDelivery {
            fail_flag: AtomicBool::new(true),
            delivered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            notify: Notify::new(),
            target: 10,
        });

        let config = test_config(4);
        let relay = PartitionedOutboxRelay::new("isolation-test", &config, delivery.clone())
            .await
            .unwrap();

        let good_target = "actor-good";
        for i in 0..10u64 {
            let msg = Message {
                id: format!("good-{}", i),
                payload: format!("{{\"i\": {}}}", i).into_bytes(),
                ..Default::default()
            };
            relay.send(good_target, msg).await.unwrap();
        }

        let bad_target = "actor-bad";
        for i in 0..5u64 {
            let msg = Message {
                id: format!("fail-{}", i),
                payload: format!("{{\"i\": {}}}", i).into_bytes(),
                ..Default::default()
            };
            relay.send(bad_target, msg).await.unwrap();
        }

        let handles = relay.run();

        tokio::select! {
            _ = delivery.notify.notified() => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                panic!("timeout");
            }
        }

        delivery.fail_flag.store(false, Ordering::SeqCst);
        relay.stop();
        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(delivery.delivered.load(Ordering::SeqCst), 10);
        assert!(delivery.failed.load(Ordering::SeqCst) > 0, "bad partition must have seen failures");

        let status = relay.status();
        let good_partition = crate::hash_router::HashRouter::new(4).route(good_target) as usize;
        let bad_partition = crate::hash_router::HashRouter::new(4).route(bad_target) as usize;

        if good_partition != bad_partition {
            assert_eq!(status.partition_statuses[good_partition].failed_count, 0,
                "good partition must have zero failures");
        }
    }

    fn plexspaces_sdk_test_msg(i: u64) -> Message {
        Message {
            id: format!("msg-{}", i),
            payload: format!("{{\"job_id\": {}}}", i).into_bytes(),
            ..Default::default()
        }
    }
}
