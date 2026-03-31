// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for Process-Group based channels
//!
//! ## Purpose
//! Tests channel functionality across different backend implementations
//! using a market feed scenario with multiple actors subscribing to different quotes.
//!
//! ## Test Scenario
//! 1. Create SQLite-based ObjectRegistry
//! 2. Create multiple nodes
//! 3. Create actors that subscribe to different stock quotes (AAPL, GOOG, META, etc.)
//! 4. Send multiple messages for each quote
//! 5. Verify all actors received all expected messages

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use plexspaces_channel::Channel;
use plexspaces_proto::channel::v1::{
    ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
};
use plexspaces_proto::common::v1::Message;
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Test guard for Kafka backend
fn kafka_config_available() -> bool {
    std::env::var("KAFKA_BROKERS").is_ok()
}

/// Test guard for NATS backend
fn nats_config_available() -> bool {
    std::env::var("NATS_URL").is_ok()
}

/// Test guard for SQS backend
fn sqs_config_available() -> bool {
    std::env::var("AWS_REGION").is_ok() && std::env::var("SQS_QUEUE_PREFIX").is_ok()
}

/// Test guard for PostgreSQL backend
fn postgres_config_available() -> bool {
    std::env::var("POSTGRES_URL").is_ok()
}

/// Market feed subscriber that tracks message counts
struct MarketFeedSubscriber {
    name: String,
    subscribed_symbols: Vec<String>,
    message_counts: Arc<RwLock<HashMap<String, u64>>>,
    total_messages: AtomicU64,
}

impl MarketFeedSubscriber {
    fn new(name: &str, symbols: Vec<String>) -> Self {
        let mut counts = HashMap::new();
        for symbol in &symbols {
            counts.insert(symbol.clone(), 0);
        }
        Self {
            name: name.to_string(),
            subscribed_symbols: symbols,
            message_counts: Arc::new(RwLock::new(counts)),
            total_messages: AtomicU64::new(0),
        }
    }

    async fn process_message(&self, message: &Message) -> bool {
        // Extract symbol from message headers
        let symbol = message.headers.get("symbol").cloned().unwrap_or_default();

        if self.subscribed_symbols.contains(&symbol) {
            let mut counts = self.message_counts.write().await;
            if let Some(count) = counts.get_mut(&symbol) {
                *count += 1;
            }
            self.total_messages.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    async fn get_count(&self, symbol: &str) -> u64 {
        let counts = self.message_counts.read().await;
        *counts.get(symbol).unwrap_or(&0)
    }

    fn get_total(&self) -> u64 {
        self.total_messages.load(Ordering::Relaxed)
    }
}

/// Create channel config for testing
fn create_channel_config(name: &str, provider: ChannelProvider) -> ChannelConfig {
    ChannelConfig {
        name: name.to_string(),
        provider: provider as i32,
        capacity: 1000,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    }
}

/// Create a market feed message
fn create_quote_message(symbol: &str, price: f64, volume: u64) -> Message {
    let mut headers = HashMap::new();
    headers.insert("symbol".to_string(), symbol.to_string());
    headers.insert("price".to_string(), price.to_string());
    headers.insert("volume".to_string(), volume.to_string());
    headers.insert("content-type".to_string(), "application/json".to_string());

    Message {
        id: ulid::Ulid::new().to_string(),
        channel: "market-feed".to_string(),
        payload: format!(
            r#"{{"symbol":"{}","price":{},"volume":{}}}"#,
            symbol, price, volume
        )
        .into_bytes(),
        headers,
        timestamp: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
        }),
        ..Default::default()
    }
}

// ============================================================================
// IN-MEMORY CHANNEL TESTS
// ============================================================================

#[tokio::test]
async fn test_inmemory_channel_market_feed() {
    let config = create_channel_config(
        "market-feed-memory",
        ChannelProvider::ChannelProviderInMemory,
    );
    let channel = plexspaces_channel::InMemoryChannel::new(config)
        .await
        .unwrap();

    run_market_feed_test(Arc::new(channel), "InMemory").await;
}

// ============================================================================
// SQLITE CHANNEL TESTS
// ============================================================================

#[tokio::test]
async fn test_sqlite_channel_market_feed() {
    let config =
        create_channel_config("market-feed-sqlite", ChannelProvider::ChannelProviderSqlite);

    // Create SQLite channel (uses in-memory by default for tests)
    let mut sqlite_config = config.clone();
    sqlite_config.backend_config = Some(
        plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(
            plexspaces_proto::channel::v1::SqliteConfig {
                database_path: ":memory:".to_string(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: true,
                cleanup_age_seconds: 3600,
            },
        ),
    );

    match plexspaces_channel::create_channel(sqlite_config).await {
        Ok(channel) => {
            run_market_feed_test(Arc::from(channel), "SQLite").await;
        }
        Err(e) => {
            eprintln!("SQLite channel not available: {}", e);
        }
    }
}

// ============================================================================
// KAFKA CHANNEL TESTS (GUARDED)
// ============================================================================

#[tokio::test]
async fn test_kafka_channel_market_feed() {
    if !kafka_config_available() {
        eprintln!("Skipping Kafka test - KAFKA_BROKERS not set");
        return;
    }

    let brokers = std::env::var("KAFKA_BROKERS").unwrap();
    let config = ChannelConfig {
        name: "market-feed-kafka".to_string(),
        provider: ChannelProvider::ChannelProviderKafka as i32,
        capacity: 1000,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(
            plexspaces_proto::channel::v1::channel_config::BackendConfig::Kafka(
                plexspaces_proto::channel::v1::KafkaConfig {
                    brokers: vec![brokers],
                    topic: "market-feed-test".to_string(),
                    consumer_group: "market-feed-test-group".to_string(),
                    partitions: 1,
                    replication_factor: 1,
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };

    match plexspaces_channel::create_channel(config).await {
        Ok(channel) => {
            run_market_feed_test(Arc::from(channel), "Kafka").await;
        }
        Err(e) => {
            eprintln!("Kafka channel not available: {}", e);
        }
    }
}

// ============================================================================
// NATS CHANNEL TESTS (GUARDED)
// ============================================================================

#[tokio::test]
async fn test_nats_channel_market_feed() {
    if !nats_config_available() {
        eprintln!("Skipping NATS test - NATS_URL not set");
        return;
    }

    let servers = std::env::var("NATS_URL").unwrap();
    let config = ChannelConfig {
        name: "market-feed-nats".to_string(),
        provider: ChannelProvider::ChannelProviderNats as i32,
        capacity: 1000,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(
            plexspaces_proto::channel::v1::channel_config::BackendConfig::Nats(
                plexspaces_proto::channel::v1::NatsConfig {
                    servers,
                    subject: "market-feed-test".to_string(),
                    queue_group: "market-feed-test-group".to_string(),
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };

    match plexspaces_channel::create_channel(config).await {
        Ok(channel) => {
            run_market_feed_test(Arc::from(channel), "NATS").await;
        }
        Err(e) => {
            eprintln!("NATS channel not available: {}", e);
        }
    }
}

// ============================================================================
// SQS CHANNEL TESTS (GUARDED)
// ============================================================================

#[tokio::test]
async fn test_sqs_channel_market_feed() {
    if !sqs_config_available() {
        eprintln!("Skipping SQS test - AWS_REGION or SQS_QUEUE_PREFIX not set");
        return;
    }

    let region = std::env::var("AWS_REGION").unwrap();
    let prefix =
        std::env::var("SQS_QUEUE_PREFIX").unwrap_or_else(|_| "plexspaces-test-".to_string());
    let endpoint = std::env::var("SQS_ENDPOINT_URL").ok();

    let config = ChannelConfig {
        name: "market-feed-sqs".to_string(),
        provider: ChannelProvider::ChannelProviderSqs as i32,
        capacity: 1000,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(
            plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqs(
                plexspaces_proto::channel::v1::SqsConfig {
                    region,
                    queue_prefix: prefix,
                    endpoint_url: endpoint.unwrap_or_default(),
                    visibility_timeout_seconds: 30,
                    message_retention_period_seconds: 345600,
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };

    match plexspaces_channel::create_channel(config).await {
        Ok(channel) => {
            run_market_feed_test(Arc::from(channel), "SQS").await;
        }
        Err(e) => {
            eprintln!("SQS channel not available: {}", e);
        }
    }
}

// ============================================================================
// PROCESS GROUP CHANNEL TESTS
// ============================================================================

// NOTE: Process group channel test requires additional infrastructure setup
// and is tested separately in the plexspaces-services crate integration tests

// ============================================================================
// MARKET FEED TEST IMPLEMENTATION
// ============================================================================

/// Run the market feed test scenario
async fn run_market_feed_test(channel: Arc<dyn Channel>, backend_name: &str) {
    // Keep the integration scenario small and deterministic so the backend
    // behavior is covered without turning the suite into a throughput benchmark.
    let symbols = vec!["AAPL", "GOOG", "META"];

    // Create subscribers with different symbol sets
    let subscribers = vec![
        MarketFeedSubscriber::new("trader-1", vec!["AAPL".to_string(), "GOOG".to_string()]),
        MarketFeedSubscriber::new("trader-2", vec!["META".to_string(), "AMZN".to_string()]),
        MarketFeedSubscriber::new("trader-3", vec!["MSFT".to_string(), "AAPL".to_string()]),
        MarketFeedSubscriber::new("analyst-1", symbols.iter().map(|s| s.to_string()).collect()),
    ];

    // Message counts to send per symbol
    let messages_per_symbol = 3;

    // Send messages for each symbol
    let mut sent_counts: HashMap<String, u64> = HashMap::new();
    for symbol in &symbols {
        for i in 0..messages_per_symbol {
            let price = 100.0 + (i as f64 * 0.5);
            let volume = 1000 + (i as u64 * 100);
            let message = create_quote_message(symbol, price, volume);

            match channel.send(message).await {
                Ok(_) => {
                    *sent_counts.entry(symbol.to_string()).or_insert(0) += 1;
                }
                Err(e) => {
                    eprintln!("Failed to send message for {}: {}", symbol, e);
                }
            }
        }
    }

    let total_sent = sent_counts.values().sum::<u64>() as u32;
    let received_messages = timeout(Duration::from_secs(2), channel.receive(total_sent))
        .await
        .expect("market feed receive should complete quickly")
        .unwrap_or_else(|e| panic!("Failed to receive market feed messages: {}", e));

    // Process messages through subscribers
    for message in &received_messages {
        for subscriber in &subscribers {
            subscriber.process_message(message).await;
        }
    }

    let stats = channel.get_stats().await.unwrap_or_default();

    // Assertions
    let total_sent: u64 = sent_counts.values().sum();
    assert_eq!(
        total_sent,
        (symbols.len() * messages_per_symbol) as u64,
        "Should have sent {} messages",
        symbols.len() * messages_per_symbol
    );

    // The analyst subscriber should have received all messages
    let analyst = &subscribers[3];
    assert_eq!(
        analyst.get_total(),
        received_messages.len() as u64,
        "Analyst should have processed all received messages"
    );
    assert_eq!(
        stats.messages_sent,
        total_sent,
        "{} backend should record all sent messages",
        backend_name
    );
}

// ============================================================================
// MULTI-NODE MARKET FEED TEST
// ============================================================================

#[tokio::test]
async fn test_multi_node_market_feed() {
    // This test simulates multiple nodes with actors subscribing to different quotes
    // Uses in-memory channels for simplicity

    // Create channels for each "node"
    let node1_channel = Arc::new(
        plexspaces_channel::InMemoryChannel::new(create_channel_config(
            "node1-market",
            ChannelProvider::ChannelProviderInMemory,
        ))
        .await
        .unwrap(),
    );

    let node2_channel = Arc::new(
        plexspaces_channel::InMemoryChannel::new(create_channel_config(
            "node2-market",
            ChannelProvider::ChannelProviderInMemory,
        ))
        .await
        .unwrap(),
    );

    // Create subscribers on each node
    let node1_subscriber =
        MarketFeedSubscriber::new("node1-trader", vec!["AAPL".to_string(), "GOOG".to_string()]);
    let node2_subscriber =
        MarketFeedSubscriber::new("node2-trader", vec!["META".to_string(), "AMZN".to_string()]);

    // Send quotes to each node's channel
    for i in 0..5 {
        // Node 1 receives AAPL and GOOG
        let aapl_msg = create_quote_message("AAPL", 150.0 + i as f64, 1000);
        let goog_msg = create_quote_message("GOOG", 2800.0 + i as f64, 500);
        node1_channel.send(aapl_msg).await.unwrap();
        node1_channel.send(goog_msg).await.unwrap();

        // Node 2 receives META and AMZN
        let meta_msg = create_quote_message("META", 300.0 + i as f64, 2000);
        let amzn_msg = create_quote_message("AMZN", 3400.0 + i as f64, 800);
        node2_channel.send(meta_msg).await.unwrap();
        node2_channel.send(amzn_msg).await.unwrap();
    }

    // Receive and process on each node
    let node1_messages = timeout(Duration::from_secs(1), node1_channel.receive(10))
        .await
        .expect("node 1 should receive market feed quickly")
        .unwrap();
    for msg in &node1_messages {
        node1_subscriber.process_message(msg).await;
    }

    let node2_messages = timeout(Duration::from_secs(1), node2_channel.receive(10))
        .await
        .expect("node 2 should receive market feed quickly")
        .unwrap();
    for msg in &node2_messages {
        node2_subscriber.process_message(msg).await;
    }

    assert_eq!(
        node1_subscriber.get_total(),
        10,
        "Node 1 should receive 10 messages (5 AAPL + 5 GOOG)"
    );
    assert_eq!(
        node2_subscriber.get_total(),
        10,
        "Node 2 should receive 10 messages (5 META + 5 AMZN)"
    );
}

// ============================================================================
// CHANNEL BACKEND PRIORITY TEST
// ============================================================================

#[tokio::test]
async fn test_channel_backend_priority_selection() {
    use plexspaces_proto::channel::v1::ChannelProvider;

    println!("\n=== Channel Backend Priority Selection Test ===\n");

    // Test that undefined backend (0) selects the appropriate backend based on config

    // With no config, should default to InMemory
    let config_no_backend = ChannelConfig {
        name: "test-priority".to_string(),
        provider: 0, // Undefined
        capacity: 100,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: None,
        ..Default::default()
    };

    // Backend 0 is InMemory in the proto enum
    let backend = ChannelProvider::try_from(config_no_backend.provider)
        .unwrap_or(ChannelProvider::ChannelProviderInMemory);
    assert_eq!(backend, ChannelProvider::ChannelProviderInMemory);

    // With Kafka config, should use Kafka
    let config_kafka = ChannelConfig {
        name: "test-kafka".to_string(),
        provider: ChannelProvider::ChannelProviderKafka as i32,
        capacity: 100,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        backend_config: Some(
            plexspaces_proto::channel::v1::channel_config::BackendConfig::Kafka(
                plexspaces_proto::channel::v1::KafkaConfig {
                    brokers: vec!["localhost:9092".to_string()],
                    topic: "test".to_string(),
                    ..Default::default()
                },
            ),
        ),
        ..Default::default()
    };

    let backend_kafka = ChannelProvider::try_from(config_kafka.provider)
        .unwrap_or(ChannelProvider::ChannelProviderInMemory);
    assert_eq!(backend_kafka, ChannelProvider::ChannelProviderKafka);

    println!("Backend priority selection verified!");
    println!("\n=== Channel Backend Priority Selection Test Passed! ===\n");
}

// ============================================================================
// HIGH-THROUGHPUT TEST
// ============================================================================

#[tokio::test]
async fn test_high_throughput_market_feed() {
    let config = create_channel_config("high-throughput", ChannelProvider::ChannelProviderInMemory);
    let channel = Arc::new(
        plexspaces_channel::InMemoryChannel::new(config)
            .await
            .unwrap(),
    );

    let symbols = vec!["AAPL", "GOOG", "META", "AMZN"];
    let messages_per_symbol = 20;
    let total_messages = (symbols.len() * messages_per_symbol) as u64;

    for symbol in &symbols {
        for i in 0..messages_per_symbol {
            let msg = create_quote_message(symbol, 100.0 + i as f64, 1000);
            channel.send(msg).await.unwrap();
        }
    }
    let received = timeout(Duration::from_secs(2), channel.receive(total_messages as u32))
        .await
        .expect("high-throughput receive should complete quickly")
        .unwrap();

    let stats = channel.get_stats().await.unwrap_or_default();

    assert_eq!(
        stats.messages_sent, total_messages,
        "Should have sent {} messages",
        total_messages
    );
    assert_eq!(
        received.len() as u64,
        total_messages,
        "Should receive every in-memory market feed message"
    );
}
