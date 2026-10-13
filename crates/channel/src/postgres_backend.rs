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

//! PostgreSQL backend for durable channels.
//!
//! ## Purpose
//! Provides a persistent, tenant-isolated channel implementation backed by PostgreSQL
//! for production multi-node deployments.
//!
//! ## Design
//! - All SQL is scoped by `(tenant_id, namespace, channel_name)` — row-level isolation.
//! - OVERWRITE (LWW) uses `UPDATE … WHERE … AND NOT acked` then INSERT if 0 rows affected,
//!   preserving the original row's queue position (`created_at` unchanged).
//! - `PostgresChannel` is always created via the shared pool injected by the node factory.
//! - `PostgresChannelRepository` caches channels by `(tenant_id, namespace, name)` so
//!   repeated `get_or_create()` calls return the same `Arc<dyn Channel>`.
//!
//! ## Schema
//! ```sql
//! CREATE TABLE channel_messages (
//!     id                  TEXT        PRIMARY KEY,
//!     tenant_id           TEXT        NOT NULL DEFAULT '',
//!     namespace           TEXT        NOT NULL DEFAULT '',
//!     channel_name        TEXT        NOT NULL,
//!     payload             BYTEA       NOT NULL,
//!     headers_json        JSONB       NOT NULL DEFAULT '{}',
//!     idempotency_key     TEXT        NOT NULL DEFAULT '',
//!     conflict_resolution SMALLINT    NOT NULL DEFAULT 0,
//!     timestamp           BIGINT      NOT NULL,  -- Unix ms
//!     acked               BOOLEAN     NOT NULL DEFAULT FALSE,
//!     dead_lettered       BOOLEAN     NOT NULL DEFAULT FALSE,
//!     created_at          BIGINT      NOT NULL   -- Unix ms
//! );
//! ```

use crate::channel::{Channel, ChannelRepository};
use crate::{ChannelError, ChannelResult};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::stream::BoxStream;
use plexspaces_common::RequestContext;
use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider, ChannelStats};
use plexspaces_proto::common::v1::{ConflictResolutionPolicy, Message};
use prost_types::Timestamp;
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// PostgreSQL channel implementation for durable messaging.
///
/// ## Invariants
/// - Always created via `from_shared_pool` through `PostgresChannelRepository`.
/// - All reads/writes are scoped to `(tenant_id, namespace, channel_name)`.
/// - OVERWRITE updates in-place, preserving `created_at` (queue position).
#[derive(Clone)]
pub struct PostgresChannel {
    config: ChannelConfig,
    tenant_id: String,
    namespace: String,
    pool: PgPool,
    table_name: String,
    stats: Arc<ChannelStatsData>,
    closed: Arc<AtomicBool>,
    pending_acks: Arc<RwLock<HashMap<String, Message>>>,
}

struct ChannelStatsData {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    messages_pending: AtomicU64,
    messages_failed: AtomicU64,
}

impl PostgresChannel {
    /// Create a `PostgresChannel` using an existing shared pool.
    ///
    /// All channels on the same node share one `PgPool`. `ensure_schema` must have
    /// been called before this (done by `PostgresChannelRepository::new()`).
    pub(crate) async fn from_shared_pool(
        ctx: &RequestContext,
        pool: PgPool,
        config: ChannelConfig,
    ) -> ChannelResult<Self> {
        use plexspaces_common::RequestContextExt;
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();
        let table_name = "channel_messages".to_string();

        tracing::info!(
            channel_name = %config.name,
            tenant_id = %tenant_id,
            namespace = %namespace,
            backend = "PostgreSQL(shared)",
            "Channel created using shared node pool"
        );

        let channel = Self {
            config,
            tenant_id,
            namespace,
            pool,
            table_name,
            stats: Arc::new(ChannelStatsData {
                messages_sent: AtomicU64::new(0),
                messages_received: AtomicU64::new(0),
                messages_pending: AtomicU64::new(0),
                messages_failed: AtomicU64::new(0),
            }),
            closed: Arc::new(AtomicBool::new(false)),
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
        };

        channel.recover_unacked_messages().await?;
        Ok(channel)
    }

    /// Create the `channel_messages` table and its indexes if they do not yet exist.
    ///
    /// Safe to call on production databases that already have the schema — all
    /// statements use `IF NOT EXISTS`.
    pub(crate) async fn ensure_schema(pool: &PgPool, table_name: &str) -> ChannelResult<()> {
        let create_table_sql = format!(
            r#"CREATE TABLE IF NOT EXISTS {tbl} (
                id                  TEXT     PRIMARY KEY,
                tenant_id           TEXT     NOT NULL DEFAULT '',
                namespace           TEXT     NOT NULL DEFAULT '',
                channel_name        TEXT     NOT NULL,
                payload             BYTEA    NOT NULL,
                headers_json        JSONB    NOT NULL DEFAULT '{{}}',
                idempotency_key     TEXT     NOT NULL DEFAULT '',
                conflict_resolution SMALLINT NOT NULL DEFAULT 0,
                timestamp           BIGINT   NOT NULL,
                acked               BOOLEAN  NOT NULL DEFAULT FALSE,
                dead_lettered       BOOLEAN  NOT NULL DEFAULT FALSE,
                created_at          BIGINT   NOT NULL
            )"#,
            tbl = table_name
        );
        sqlx::query(&create_table_sql)
            .execute(pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

        sqlx::query(&format!(
            "CREATE INDEX IF NOT EXISTS idx_{tbl}_unacked ON {tbl}(tenant_id, namespace, channel_name, created_at) \
             WHERE NOT acked AND NOT dead_lettered",
            tbl = table_name
        ))
        .execute(pool)
        .await
        .map_err(|e| ChannelError::BackendError(e.to_string()))?;

        sqlx::query(&format!(
            "CREATE INDEX IF NOT EXISTS idx_{tbl}_idempotency ON {tbl}(tenant_id, namespace, channel_name, idempotency_key) \
             WHERE idempotency_key != '' AND NOT acked",
            tbl = table_name
        ))
        .execute(pool)
        .await
        .map_err(|e| ChannelError::BackendError(e.to_string()))?;

        sqlx::query(&format!(
            "CREATE INDEX IF NOT EXISTS idx_{tbl}_dlq ON {tbl}(tenant_id, namespace, channel_name) \
             WHERE dead_lettered",
            tbl = table_name
        ))
        .execute(pool)
        .await
        .map_err(|e| ChannelError::BackendError(e.to_string()))?;

        Ok(())
    }

    fn system_time_to_unix_ms(time: SystemTime) -> i64 {
        time.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
    }

    fn proto_timestamp_to_unix_ms(ts: &Option<Timestamp>) -> i64 {
        if let Some(t) = ts {
            (t.seconds * 1000) + (t.nanos / 1_000_000) as i64
        } else {
            Self::system_time_to_unix_ms(SystemTime::now())
        }
    }

    async fn recover_unacked_messages(&self) -> ChannelResult<()> {
        let query_sql = format!(
            "SELECT id, payload, headers_json::text, timestamp, idempotency_key, conflict_resolution \
             FROM {} WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 \
             AND NOT acked AND NOT dead_lettered ORDER BY created_at ASC",
            self.table_name
        );

        let rows = sqlx::query(&query_sql)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to recover messages: {}", e)))?;

        let mut pending_acks = self.pending_acks.write().await;
        let mut recovered_count = 0u64;

        for row in rows {
            let id: String = row.get(0);
            let payload: Vec<u8> = row.get(1);
            let headers_json: String = row.try_get(2).unwrap_or_else(|_| "{}".to_string());
            let timestamp_ms: i64 = row.get(3);
            let headers: HashMap<String, String> =
                serde_json::from_str(&headers_json).unwrap_or_default();
            let idempotency_key: String = row.try_get(4).unwrap_or_default();
            let conflict_resolution: i16 = row.try_get(5).unwrap_or(0);

            let message = Message {
                id: id.clone(),
                channel: self.config.name.clone(),
                payload,
                timestamp: Some(Timestamp {
                    seconds: timestamp_ms / 1000,
                    nanos: ((timestamp_ms % 1000) * 1_000_000) as i32,
                }),
                headers,
                idempotency_key,
                conflict_resolution: conflict_resolution as i32,
                ..Default::default()
            };

            pending_acks.insert(id, message);
            recovered_count += 1;
        }

        self.stats.messages_pending.store(recovered_count, Ordering::Relaxed);

        if recovered_count > 0 {
            tracing::info!(
                "Recovered {} unacked messages for PG channel '{}'",
                recovered_count,
                self.config.name
            );
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for PostgresChannel {
    async fn send(&self, message: Message) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let msg_id = message.id.clone();
        let timestamp_ms = Self::proto_timestamp_to_unix_ms(&message.timestamp);
        let created_at = Self::system_time_to_unix_ms(SystemTime::now());
        let headers_json =
            serde_json::to_string(&message.headers).unwrap_or_else(|_| "{}".to_string());
        let conflict_resolution = message.conflict_resolution as i16;

        // OVERWRITE (LWW): update in place, preserving queue position (created_at unchanged)
        if conflict_resolution == ConflictResolutionPolicy::ConflictResolutionPolicyOverwrite as i16
            && !message.idempotency_key.is_empty()
        {
            let update_sql = format!(
                "UPDATE {} SET payload = $1, headers_json = $2::jsonb, conflict_resolution = $3, timestamp = $4 \
                 WHERE tenant_id = $5 AND namespace = $6 AND channel_name = $7 AND idempotency_key = $8 AND NOT acked",
                self.table_name
            );
            let result = sqlx::query(&update_sql)
                .bind(&message.payload)
                .bind(&headers_json)
                .bind(conflict_resolution)
                .bind(timestamp_ms)
                .bind(&self.tenant_id)
                .bind(&self.namespace)
                .bind(&self.config.name)
                .bind(&message.idempotency_key)
                .execute(&self.pool)
                .await
                .map_err(|e| ChannelError::BackendError(format!("OVERWRITE update failed: {}", e)))?;

            if result.rows_affected() > 0 {
                self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
                return Ok(msg_id);
            }
            // No unacked row with this key — fall through to INSERT.
        }

        let insert_sql = format!(
            "INSERT INTO {} (id, tenant_id, namespace, channel_name, payload, headers_json, \
             idempotency_key, conflict_resolution, timestamp, acked, dead_lettered, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, $8, $9, FALSE, FALSE, $10)",
            self.table_name
        );

        sqlx::query(&insert_sql)
            .bind(&message.id)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .bind(&message.payload)
            .bind(&headers_json)
            .bind(&message.idempotency_key)
            .bind(conflict_resolution)
            .bind(timestamp_ms)
            .bind(created_at)
            .execute(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to send message: {}", e)))?;

        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.messages_pending.fetch_add(1, Ordering::Relaxed);

        Ok(msg_id)
    }

    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<Message>> {
        let query_sql = format!(
            "SELECT id, payload, headers_json::text, timestamp, idempotency_key, conflict_resolution \
             FROM {} WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 \
             AND NOT acked AND NOT dead_lettered ORDER BY created_at ASC LIMIT $4",
            self.table_name
        );

        let rows = sqlx::query(&query_sql)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .bind(max_messages as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to receive messages: {}", e)))?;

        let mut messages = Vec::new();

        for row in rows {
            let id: String = row.get(0);
            let payload: Vec<u8> = row.get(1);
            let headers_json: String = row.try_get(2).unwrap_or_else(|_| "{}".to_string());
            let timestamp_ms: i64 = row.get(3);
            let headers: HashMap<String, String> =
                serde_json::from_str(&headers_json).unwrap_or_default();
            let idempotency_key: String = row.try_get(4).unwrap_or_default();
            let conflict_resolution: i16 = row.try_get(5).unwrap_or(0);

            let message = Message {
                id: id.clone(),
                channel: self.config.name.clone(),
                payload,
                timestamp: Some(Timestamp {
                    seconds: timestamp_ms / 1000,
                    nanos: ((timestamp_ms % 1000) * 1_000_000) as i32,
                }),
                headers,
                idempotency_key,
                conflict_resolution: conflict_resolution as i32,
                ..Default::default()
            };

            messages.push(message.clone());
            let mut pending_acks = self.pending_acks.write().await;
            pending_acks.insert(id, message);
        }

        if !messages.is_empty() {
            self.stats
                .messages_received
                .fetch_add(messages.len() as u64, Ordering::Relaxed);
        }

        Ok(messages)
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<Message>> {
        self.receive(max_messages).await
    }

    async fn subscribe(
        &self,
        _consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, Message>> {
        Err(ChannelError::BackendError(
            "PostgreSQL backend does not support subscribe (use receive instead)".to_string(),
        ))
    }

    async fn publish(&self, message: Message) -> ChannelResult<u32> {
        self.send(message).await?;
        Ok(0)
    }

    async fn ack(&self, message_id: &str) -> ChannelResult<()> {
        let check_sql = format!(
            "SELECT acked FROM {} WHERE id = $1 AND tenant_id = $2 AND namespace = $3 AND channel_name = $4",
            self.table_name
        );

        let acked_status: Option<bool> = sqlx::query_scalar(&check_sql)
            .bind(message_id)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to check message status: {}", e)))?;

        match acked_status {
            None | Some(true) => {
                return Err(ChannelError::MessageNotFound(message_id.to_string()));
            }
            Some(false) => {}
        }

        let update_sql = format!(
            "UPDATE {} SET acked = TRUE WHERE id = $1 AND tenant_id = $2 AND namespace = $3 \
             AND channel_name = $4 AND NOT acked",
            self.table_name
        );

        let result = sqlx::query(&update_sql)
            .bind(message_id)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .execute(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to ack message: {}", e)))?;

        if result.rows_affected() == 0 {
            return Err(ChannelError::MessageNotFound(message_id.to_string()));
        }

        let mut pending_acks = self.pending_acks.write().await;
        pending_acks.remove(message_id);
        self.stats.messages_pending.fetch_sub(1, Ordering::Relaxed);

        Ok(())
    }

    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
        if requeue {
            // Already "unacked" in DB — nothing to do for a fresh message.
            // For a message in pending_acks (received but not yet acked), remove from cache
            // so it can be dequeued again.
            let mut pending_acks = self.pending_acks.write().await;
            pending_acks.remove(message_id);
        } else {
            self.ack(message_id).await?;
        }

        self.stats.messages_failed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        let count_sql = format!(
            "SELECT COUNT(*) FROM {} WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 AND NOT acked",
            self.table_name
        );

        let pending_count: i64 = sqlx::query_scalar(&count_sql)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("Failed to get stats: {}", e)))?;

        Ok(ChannelStats {
            name: self.config.name.clone(),
            provider: self.config.provider,
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_received: self.stats.messages_received.load(Ordering::Relaxed),
            messages_pending: pending_count as u64,
            messages_failed: self.stats.messages_failed.load(Ordering::Relaxed),
            avg_latency_us: 0,
            throughput: 0.0,
            backend_stats: HashMap::new(),
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}

// ─── PostgresChannelRepository ────────────────────────────────────────────────
//
// Follows the same pattern as `SqliteChannelRepository` in `sqlite_backend.rs`.
// Lives here alongside `PostgresChannel` so all PostgreSQL/sqlx code is co-located.

#[cfg(feature = "repository")]
pub use repository::PostgresChannelRepository;

#[cfg(feature = "repository")]
mod repository {
    use super::PostgresChannel;
    use crate::channel::{Channel, ChannelRepository};
    use crate::ChannelResult;
    use async_trait::async_trait;
    use dashmap::DashMap;
    use plexspaces_common::RequestContext;
    use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee};
    use sqlx::PgPool;
    use std::sync::Arc;

    /// PostgreSQL-backed `ChannelRepository`.
    ///
    /// Holds a single `PgPool` shared across all tenants on the node. Each
    /// distinct `(tenant_id, namespace, name)` triple maps to its own
    /// `PostgresChannel` instance cached in memory.
    ///
    /// ## Lifecycle
    /// Call `new(pool)` once at node startup. It runs `ensure_schema` so every
    /// subsequent `get_or_create()` call can assume the table and indexes exist.
    pub struct PostgresChannelRepository {
        pool: PgPool,
        cache: DashMap<(String, String, String), Arc<dyn Channel>>,
    }

    impl PostgresChannelRepository {
        /// Create a repository from an existing shared `PgPool`.
        ///
        /// Runs `ensure_schema` once so that all `get_or_create()` calls can
        /// assume `channel_messages` and its indexes are present.
        pub async fn new(pool: PgPool) -> crate::ChannelResult<Self> {
            PostgresChannel::ensure_schema(&pool, "channel_messages").await?;
            Ok(Self {
                pool,
                cache: DashMap::new(),
            })
        }
    }

    #[async_trait]
    impl ChannelRepository for PostgresChannelRepository {
        async fn get_or_create(
            &self,
            ctx: &RequestContext,
            name: &str,
        ) -> crate::ChannelResult<Arc<dyn Channel>> {
            use plexspaces_common::RequestContextExt;
            let tenant_id = ctx.tenant_id().to_string();
            let namespace = ctx.namespace().to_string();
            let cache_key = (tenant_id.clone(), namespace.clone(), name.to_string());

            if let Some(channel) = self.cache.get(&cache_key) {
                return Ok(channel.clone());
            }

            let config = ChannelConfig {
                name: name.to_string(),
                provider: ChannelProvider::ChannelProviderPostgres as i32,
                delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
                ..Default::default()
            };

            let channel =
                PostgresChannel::from_shared_pool(ctx, self.pool.clone(), config).await?;
            let arc: Arc<dyn Channel> = Arc::new(channel);

            self.cache.insert(cache_key, arc.clone());
            Ok(arc)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use plexspaces_common::RequestContextExt;

        // Run with: DATABASE_URL=postgres://localhost/test cargo test -p plexspaces-channel --features postgres-backend,repository -- --ignored
        fn pg_url() -> String {
            std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/plexspaces_test".to_string())
        }

        #[tokio::test]
        #[ignore]
        async fn test_get_or_create_returns_same_instance_for_same_key() {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&pg_url())
                .await
                .unwrap();
            let repo = PostgresChannelRepository::new(pool).await.unwrap();
            let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "ns-a".to_string());
            let ch1 = repo.get_or_create(&ctx, "actor-1").await.unwrap();
            let ch2 = repo.get_or_create(&ctx, "actor-1").await.unwrap();
            assert!(Arc::ptr_eq(&ch1, &ch2));
        }

        #[tokio::test]
        #[ignore]
        async fn test_different_tenants_get_different_channels() {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&pg_url())
                .await
                .unwrap();
            let repo = PostgresChannelRepository::new(pool).await.unwrap();
            let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns".to_string());
            let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "actor-1").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "actor-1").await.unwrap();
            assert!(!Arc::ptr_eq(&ch_a, &ch_b));
        }

        #[tokio::test]
        #[ignore]
        async fn test_send_and_receive() {
            use plexspaces_proto::common::v1::Message;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&pg_url())
                .await
                .unwrap();
            let repo = PostgresChannelRepository::new(pool).await.unwrap();
            let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "ns".to_string());
            let ch = repo.get_or_create(&ctx, "actor-send-recv").await.unwrap();

            let msg = Message {
                id: "pg-msg-1".to_string(),
                channel: "actor-send-recv".to_string(),
                payload: b"hello postgres".to_vec(),
                ..Default::default()
            };
            ch.send(msg).await.unwrap();

            let received = ch.receive(1).await.unwrap();
            assert_eq!(received.len(), 1);
            assert_eq!(received[0].payload, b"hello postgres");
        }

        #[tokio::test]
        #[ignore]
        async fn test_overwrite_updates_in_place() {
            use plexspaces_proto::common::v1::Message;
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(&pg_url())
                .await
                .unwrap();
            let repo = PostgresChannelRepository::new(pool).await.unwrap();
            let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "ns".to_string());
            let ch = repo.get_or_create(&ctx, "actor-overwrite").await.unwrap();

            let mut msg1 = Message {
                id: "pg-ow-msg1".to_string(),
                channel: "actor-overwrite".to_string(),
                payload: b"original".to_vec(),
                idempotency_key: "key-ow".to_string(),
                conflict_resolution: 1,
                ..Default::default()
            };
            ch.send(msg1.clone()).await.unwrap();

            // Nack to keep acked = false
            let queued = ch.receive(10).await.unwrap();
            ch.nack(&queued[0].id, true).await.unwrap();

            let mut msg2 = Message {
                id: "pg-ow-msg2".to_string(),
                channel: "actor-overwrite".to_string(),
                payload: b"updated".to_vec(),
                idempotency_key: "key-ow".to_string(),
                conflict_resolution: 1,
                ..Default::default()
            };
            ch.send(msg2).await.unwrap();

            let queued2 = ch.receive(10).await.unwrap();
            assert_eq!(queued2.len(), 1);
            assert_eq!(queued2[0].id, "pg-ow-msg1"); // original row id preserved
            assert_eq!(queued2[0].payload, b"updated");
        }
    }
}
