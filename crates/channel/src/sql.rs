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

//! SQL-backed `ChannelRepository` implementations.
//!
//! Both `SqliteChannelRepository` and `PostgresChannelRepository` live in this
//! file so that all SQL/pool code is colocated, following the convention
//! established by `plexspaces-keyvalue` (`sql.rs`) and
//! `plexspaces-object-registry` (`repository/sql.rs`).
//!
//! The `ChannelRepository` trait itself lives in `repository.rs`; this file is
//! implementation-only.
//!
//! ## Tenant isolation
//! Every SQL statement is scoped by `(tenant_id, namespace, channel_name)`.
//! No cross-tenant data leakage is possible at the query level.
//!
//! ## Feature flags
//! - `sqlite-backend` + `repository` → enables `SqliteChannelRepository`
//! - `postgres-backend` + `repository` → enables `PostgresChannelRepository`

// ─── SQLite repository ────────────────────────────────────────────────────────

#[cfg(all(feature = "sqlite-backend", feature = "repository"))]
pub use sqlite_impl::SqliteChannelRepository;

#[cfg(all(feature = "sqlite-backend", feature = "repository"))]
mod sqlite_impl {
    use crate::channel::Channel;
    use crate::repository::ChannelRepository;
    use crate::sqlite_backend::SqliteChannel;
    use crate::ChannelResult;
    use async_trait::async_trait;
    use dashmap::DashMap;
    use plexspaces_common::RequestContext;
    use plexspaces_proto::channel::v1::{
        channel_config, ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
        SqliteConfig,
    };
    use std::sync::Arc;

    /// SQLite-backed `ChannelRepository`.
    ///
    /// Holds a single `SqlitePool` shared across all tenants on the node. Each
    /// distinct `(tenant_id, namespace, name)` triple maps to its own
    /// `SqliteChannel` instance cached in memory. SQL queries in `SqliteChannel`
    /// always include `tenant_id` and `namespace` predicates — rows from one
    /// tenant are never visible to another.
    ///
    /// ## Lifecycle
    /// Call `new(pool)` once at node startup. It runs `ensure_schema` so every
    /// subsequent `get_or_create()` call can assume the table and indexes exist.
    pub struct SqliteChannelRepository {
        pool: sqlx::SqlitePool,
        cache: DashMap<(String, String, String), Arc<dyn Channel>>,
    }

    impl SqliteChannelRepository {
        /// Create a repository from an existing shared `SqlitePool`.
        ///
        /// Runs `ensure_schema` once so all `get_or_create()` calls can
        /// assume `channel_messages` and its indexes are present.
        /// Safe to call on production pools that already ran migration 008.
        pub async fn new(pool: sqlx::SqlitePool) -> ChannelResult<Self> {
            SqliteChannel::ensure_schema(&pool, "channel_messages").await?;
            Ok(Self {
                pool,
                cache: DashMap::new(),
            })
        }
    }

    #[async_trait]
    impl ChannelRepository for SqliteChannelRepository {
        async fn get_or_create(
            &self,
            ctx: &RequestContext,
            name: &str,
        ) -> ChannelResult<Arc<dyn Channel>> {
            use plexspaces_common::RequestContextExt;
            let tenant_id = ctx.tenant_id().to_string();
            let namespace = ctx.namespace().to_string();
            let cache_key = (tenant_id.clone(), namespace.clone(), name.to_string());

            if let Some(channel) = self.cache.get(&cache_key) {
                return Ok(channel.clone());
            }

            let config = ChannelConfig {
                name: name.to_string(),
                provider: ChannelProvider::ChannelProviderSqlite as i32,
                delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
                backend_config: Some(channel_config::BackendConfig::Sqlite(SqliteConfig {
                    database_path: "(shared)".to_string(),
                    table_name: "channel_messages".to_string(),
                    wal_mode: true,
                    cleanup_acked: false,
                    cleanup_age_seconds: 0,
                })),
                ..Default::default()
            };

            let channel = SqliteChannel::from_shared_pool(ctx, self.pool.clone(), config).await?;
            let arc: Arc<dyn Channel> = Arc::new(channel);
            self.cache.insert(cache_key, arc.clone());
            Ok(arc)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use plexspaces_common::RequestContextExt;
        use sqlx::sqlite::SqlitePoolOptions;

        async fn make_pool() -> sqlx::SqlitePool {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect(":memory:")
                .await
                .unwrap()
        }

        #[tokio::test]
        async fn test_sqlite_get_or_create_same_key_returns_same_instance() {
            let repo = SqliteChannelRepository::new(make_pool().await).await.unwrap();
            let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "ns-a".to_string());
            let ch1 = repo.get_or_create(&ctx, "actor-1").await.unwrap();
            let ch2 = repo.get_or_create(&ctx, "actor-1").await.unwrap();
            assert!(Arc::ptr_eq(&ch1, &ch2));
        }

        #[tokio::test]
        async fn test_sqlite_different_tenants_get_different_channels() {
            let repo = SqliteChannelRepository::new(make_pool().await).await.unwrap();
            let ctx_a = RequestContext::new_without_auth("tenant-a".to_string(), "ns".to_string());
            let ctx_b = RequestContext::new_without_auth("tenant-b".to_string(), "ns".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "actor-1").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "actor-1").await.unwrap();
            assert!(!Arc::ptr_eq(&ch_a, &ch_b));
        }

        #[tokio::test]
        async fn test_sqlite_different_namespaces_get_different_channels() {
            let repo = SqliteChannelRepository::new(make_pool().await).await.unwrap();
            let ctx_a =
                RequestContext::new_without_auth("tenant-1".to_string(), "ns-x".to_string());
            let ctx_b =
                RequestContext::new_without_auth("tenant-1".to_string(), "ns-y".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "actor-1").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "actor-1").await.unwrap();
            assert!(!Arc::ptr_eq(&ch_a, &ch_b));
        }

        #[tokio::test]
        async fn test_sqlite_send_receive_tenant_isolated() {
            use plexspaces_proto::common::v1::Message;
            let repo = SqliteChannelRepository::new(make_pool().await).await.unwrap();
            let ctx_a = RequestContext::new_without_auth("t-a".to_string(), "ns".to_string());
            let ctx_b = RequestContext::new_without_auth("t-b".to_string(), "ns".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "inbox").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "inbox").await.unwrap();

            ch_a.send(Message {
                id: "msg-a".to_string(),
                channel: "inbox".to_string(),
                payload: b"hello from a".to_vec(),
                ..Default::default()
            })
            .await
            .unwrap();

            // tenant-b must not see tenant-a's message
            let received_b = ch_b.receive(10).await.unwrap();
            assert!(
                received_b.is_empty(),
                "tenant-b must not receive tenant-a's messages"
            );

            let received_a = ch_a.receive(10).await.unwrap();
            assert_eq!(received_a.len(), 1);
            assert_eq!(received_a[0].payload, b"hello from a");
        }

        #[tokio::test]
        async fn test_sqlite_ack_removes_message() {
            use plexspaces_proto::common::v1::Message;
            let repo = SqliteChannelRepository::new(make_pool().await).await.unwrap();
            let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
            let ch = repo.get_or_create(&ctx, "q").await.unwrap();

            ch.send(Message {
                id: "m1".to_string(),
                channel: "q".to_string(),
                payload: b"data".to_vec(),
                ..Default::default()
            })
            .await
            .unwrap();

            let msgs = ch.receive(1).await.unwrap();
            assert_eq!(msgs.len(), 1);
            ch.ack(&msgs[0].id).await.unwrap();

            let after_ack = ch.receive(1).await.unwrap();
            assert!(after_ack.is_empty());
        }
    }
}

// ─── PostgreSQL repository ────────────────────────────────────────────────────

#[cfg(all(feature = "postgres-backend", feature = "repository"))]
pub use postgres_impl::PostgresChannelRepository;

#[cfg(all(feature = "postgres-backend", feature = "repository"))]
mod postgres_impl {
    use crate::channel::{Channel, ChannelError, ChannelResult};
    use crate::repository::ChannelRepository;
    use async_trait::async_trait;
    use dashmap::DashMap;
    use futures::stream::BoxStream;
    use plexspaces_common::RequestContext;
    use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider, ChannelStats};
    use plexspaces_proto::common::v1::{ConflictResolutionPolicy, Message};
    use prost_types::Timestamp;
    use sqlx::{postgres::PgPoolOptions, PgPool, Row};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::RwLock;

    const TABLE: &str = "channel_messages";

    // ─── PostgresChannel ──────────────────────────────────────────────────────

    /// Postgres-backed channel for a single `(tenant_id, namespace, channel_name)` triple.
    ///
    /// This is an implementation detail of `PostgresChannelRepository`; callers
    /// interact with it through `Arc<dyn Channel>`.
    #[derive(Clone)]
    pub(crate) struct PostgresChannel {
        config: ChannelConfig,
        tenant_id: String,
        namespace: String,
        pool: PgPool,
        messages_sent: Arc<AtomicU64>,
        messages_received: Arc<AtomicU64>,
        messages_failed: Arc<AtomicU64>,
        closed: Arc<AtomicBool>,
        pending_acks: Arc<RwLock<HashMap<String, Message>>>,
    }

    impl PostgresChannel {
        async fn new(
            ctx: &RequestContext,
            pool: PgPool,
            config: ChannelConfig,
        ) -> ChannelResult<Self> {
            use plexspaces_common::RequestContextExt;
            let tenant_id = ctx.tenant_id().to_string();
            let namespace = ctx.namespace().to_string();

            let ch = Self {
                config,
                tenant_id,
                namespace,
                pool,
                messages_sent: Arc::new(AtomicU64::new(0)),
                messages_received: Arc::new(AtomicU64::new(0)),
                messages_failed: Arc::new(AtomicU64::new(0)),
                closed: Arc::new(AtomicBool::new(false)),
                pending_acks: Arc::new(RwLock::new(HashMap::new())),
            };
            ch.recover_unacked_messages().await?;
            Ok(ch)
        }

        /// Idempotent schema bootstrap — safe on production pools that already
        /// have the table created by migration.
        pub(crate) async fn ensure_schema(pool: &PgPool) -> ChannelResult<()> {
            sqlx::query(&format!(
                r#"CREATE TABLE IF NOT EXISTS {TABLE} (
                    id                  TEXT        PRIMARY KEY,
                    tenant_id           TEXT        NOT NULL DEFAULT '',
                    namespace           TEXT        NOT NULL DEFAULT '',
                    channel_name        TEXT        NOT NULL,
                    payload             BYTEA       NOT NULL,
                    headers_json        TEXT        NOT NULL DEFAULT '{{}}',
                    idempotency_key     TEXT        NOT NULL DEFAULT '',
                    conflict_resolution INTEGER     NOT NULL DEFAULT 0,
                    timestamp           BIGINT      NOT NULL,
                    acked               INTEGER     NOT NULL DEFAULT 0,
                    dead_lettered       INTEGER     NOT NULL DEFAULT 0,
                    created_at          BIGINT      NOT NULL
                )"#
            ))
            .execute(pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS idx_{TABLE}_unacked \
                 ON {TABLE}(tenant_id, namespace, channel_name, created_at) \
                 WHERE acked = 0 AND dead_lettered = 0"
            ))
            .execute(pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            sqlx::query(&format!(
                "CREATE INDEX IF NOT EXISTS idx_{TABLE}_idempotency \
                 ON {TABLE}(tenant_id, namespace, channel_name, idempotency_key) \
                 WHERE idempotency_key != '' AND acked = 0"
            ))
            .execute(pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            Ok(())
        }

        async fn recover_unacked_messages(&self) -> ChannelResult<()> {
            let rows = sqlx::query(&format!(
                "SELECT id, payload, headers_json, timestamp, idempotency_key, conflict_resolution \
                 FROM {TABLE} \
                 WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 \
                   AND acked = 0 AND dead_lettered = 0 \
                 ORDER BY created_at ASC"
            ))
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("recover failed: {}", e)))?;

            let mut pending_acks = self.pending_acks.write().await;
            for row in rows {
                let id: String = row.get(0);
                let payload: Vec<u8> = row.get(1);
                let headers_json: String = row.try_get(2).unwrap_or_else(|_| "{}".to_string());
                let timestamp_ms: i64 = row.get(3);
                let idempotency_key: String = row.try_get(4).unwrap_or_default();
                let conflict_resolution: i32 = row.try_get(5).unwrap_or(0);
                let headers: HashMap<String, String> =
                    serde_json::from_str(&headers_json).unwrap_or_default();

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
                    conflict_resolution,
                    ..Default::default()
                };
                pending_acks.insert(id, message);
            }
            Ok(())
        }

        fn now_ms() -> i64 {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64
        }

        fn proto_ts_to_ms(ts: &Option<Timestamp>) -> i64 {
            match ts {
                Some(t) => (t.seconds * 1000) + (t.nanos / 1_000_000) as i64,
                None => Self::now_ms(),
            }
        }
    }

    #[async_trait]
    impl Channel for PostgresChannel {
        async fn send(&self, message: Message) -> ChannelResult<String> {
            if self.closed.load(Ordering::Relaxed) {
                return Err(ChannelError::ChannelClosed(self.config.name.clone()));
            }

            let msg_id = message.id.clone();
            let timestamp_ms = Self::proto_ts_to_ms(&message.timestamp);
            let created_at = Self::now_ms();
            let headers_json =
                serde_json::to_string(&message.headers).unwrap_or_else(|_| "{}".to_string());
            let conflict_resolution = message.conflict_resolution;

            // OVERWRITE (LWW): UPDATE in place if an unacked row with this key exists.
            if conflict_resolution
                == ConflictResolutionPolicy::ConflictResolutionPolicyOverwrite as i32
                && !message.idempotency_key.is_empty()
            {
                let result = sqlx::query(&format!(
                    "UPDATE {TABLE} \
                     SET payload = $1, headers_json = $2, conflict_resolution = $3, timestamp = $4 \
                     WHERE tenant_id = $5 AND namespace = $6 AND channel_name = $7 \
                       AND idempotency_key = $8 AND acked = 0"
                ))
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
                .map_err(|e| {
                    ChannelError::BackendError(format!("OVERWRITE update failed: {}", e))
                })?;

                if result.rows_affected() > 0 {
                    self.messages_sent.fetch_add(1, Ordering::Relaxed);
                    return Ok(msg_id);
                }
            }

            sqlx::query(&format!(
                "INSERT INTO {TABLE} \
                 (id, tenant_id, namespace, channel_name, payload, headers_json, \
                  idempotency_key, conflict_resolution, timestamp, acked, dead_lettered, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, 0, $10)"
            ))
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
            .map_err(|e| ChannelError::BackendError(format!("send INSERT failed: {}", e)))?;

            self.messages_sent.fetch_add(1, Ordering::Relaxed);
            Ok(msg_id)
        }

        async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<Message>> {
            let rows = sqlx::query(&format!(
                "SELECT id, payload, headers_json, timestamp, idempotency_key \
                 FROM {TABLE} \
                 WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 \
                   AND acked = 0 AND dead_lettered = 0 \
                 ORDER BY created_at ASC \
                 LIMIT $4"
            ))
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .bind(max_messages as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(format!("receive failed: {}", e)))?;

            let mut messages = Vec::with_capacity(rows.len());
            let mut pending_acks = self.pending_acks.write().await;
            for row in rows {
                let id: String = row.get(0);
                let payload: Vec<u8> = row.get(1);
                let headers_json: String = row.try_get(2).unwrap_or_else(|_| "{}".to_string());
                let timestamp_ms: i64 = row.get(3);
                let idempotency_key: String = row.try_get(4).unwrap_or_default();
                let headers: HashMap<String, String> =
                    serde_json::from_str(&headers_json).unwrap_or_default();

                let msg = Message {
                    id: id.clone(),
                    channel: self.config.name.clone(),
                    payload,
                    timestamp: Some(Timestamp {
                        seconds: timestamp_ms / 1000,
                        nanos: ((timestamp_ms % 1000) * 1_000_000) as i32,
                    }),
                    headers,
                    idempotency_key,
                    ..Default::default()
                };
                pending_acks.insert(id, msg.clone());
                messages.push(msg);
            }
            if !messages.is_empty() {
                self.messages_received
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
            let check: Option<i32> = sqlx::query_scalar(&format!(
                "SELECT acked FROM {TABLE} \
                 WHERE id = $1 AND tenant_id = $2 AND namespace = $3 AND channel_name = $4"
            ))
            .bind(message_id)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            match check {
                None | Some(1) => {
                    return Err(ChannelError::MessageNotFound(message_id.to_string()))
                }
                _ => {}
            }

            let result = sqlx::query(&format!(
                "UPDATE {TABLE} SET acked = 1 \
                 WHERE id = $1 AND tenant_id = $2 AND namespace = $3 AND channel_name = $4 AND acked = 0"
            ))
            .bind(message_id)
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .execute(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            if result.rows_affected() == 0 {
                return Err(ChannelError::MessageNotFound(message_id.to_string()));
            }

            self.pending_acks.write().await.remove(message_id);
            Ok(())
        }

        async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
            if requeue {
                let result = sqlx::query(&format!(
                    "UPDATE {TABLE} SET acked = 0 \
                     WHERE id = $1 AND tenant_id = $2 AND namespace = $3 AND channel_name = $4"
                ))
                .bind(message_id)
                .bind(&self.tenant_id)
                .bind(&self.namespace)
                .bind(&self.config.name)
                .execute(&self.pool)
                .await
                .map_err(|e| ChannelError::BackendError(e.to_string()))?;

                if result.rows_affected() == 0 {
                    return Err(ChannelError::MessageNotFound(message_id.to_string()));
                }
                self.pending_acks.write().await.remove(message_id);
            } else {
                self.ack(message_id).await?;
            }
            self.messages_failed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        async fn get_stats(&self) -> ChannelResult<ChannelStats> {
            let pending_count: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {TABLE} \
                 WHERE tenant_id = $1 AND namespace = $2 AND channel_name = $3 AND acked = 0"
            ))
            .bind(&self.tenant_id)
            .bind(&self.namespace)
            .bind(&self.config.name)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| ChannelError::BackendError(e.to_string()))?;

            Ok(ChannelStats {
                name: self.config.name.clone(),
                provider: self.config.provider,
                messages_sent: self.messages_sent.load(Ordering::Relaxed),
                messages_received: self.messages_received.load(Ordering::Relaxed),
                messages_pending: pending_count as u64,
                messages_failed: self.messages_failed.load(Ordering::Relaxed),
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

    // ─── PostgresChannelRepository ────────────────────────────────────────────

    /// PostgreSQL-backed `ChannelRepository`.
    ///
    /// Holds a single `PgPool` shared across all tenants on the node.
    /// Tenant isolation is enforced per-query via `(tenant_id, namespace)`.
    pub struct PostgresChannelRepository {
        pool: PgPool,
        cache: DashMap<(String, String, String), Arc<dyn Channel>>,
    }

    impl PostgresChannelRepository {
        /// Create a repository from a PostgreSQL connection string.
        ///
        /// Runs `ensure_schema` once so all `get_or_create()` calls can
        /// assume `channel_messages` and its indexes exist.
        pub async fn new(connection_string: &str) -> ChannelResult<Self> {
            let db_host = connection_string
                .split('@')
                .next_back()
                .unwrap_or("(hidden)")
                .to_string();

            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(connection_string)
                .await
                .map_err(|e| {
                    ChannelError::BackendError(format!("PG connect failed: {}", e))
                })?;

            PostgresChannel::ensure_schema(&pool).await?;

            tracing::info!(
                db_url = %db_host,
                table = TABLE,
                backend = "PostgreSQL",
                "Channel repository initialized"
            );

            Ok(Self {
                pool,
                cache: DashMap::new(),
            })
        }

        /// Create from an existing shared pool (injected at node start).
        ///
        /// `ensure_schema` is called once so the table and indexes are present.
        pub async fn from_shared_pool(pool: PgPool) -> ChannelResult<Self> {
            PostgresChannel::ensure_schema(&pool).await?;
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
        ) -> ChannelResult<Arc<dyn Channel>> {
            use plexspaces_common::RequestContextExt;
            let tenant_id = ctx.tenant_id().to_string();
            let namespace = ctx.namespace().to_string();
            let cache_key = (tenant_id.clone(), namespace.clone(), name.to_string());

            if let Some(ch) = self.cache.get(&cache_key) {
                return Ok(ch.clone());
            }

            let config = ChannelConfig {
                name: name.to_string(),
                provider: ChannelProvider::ChannelProviderPostgres as i32,
                ..Default::default()
            };

            let channel = PostgresChannel::new(ctx, self.pool.clone(), config).await?;
            let arc: Arc<dyn Channel> = Arc::new(channel);
            self.cache.insert(cache_key, arc.clone());
            Ok(arc)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use plexspaces_common::RequestContextExt;
        use plexspaces_proto::common::v1::Message;
        use sqlx::postgres::PgPoolOptions;

        /// Returns a Postgres pool using `POSTGRES_URL` env var, or `None` if
        /// not set — allowing the tests to be skipped in environments without a
        /// running Postgres instance (CI uses SQLite; integration environments
        /// set `POSTGRES_URL`).
        async fn try_pool() -> Option<PgPool> {
            let url = std::env::var("POSTGRES_URL").ok()?;
            PgPoolOptions::new()
                .max_connections(5)
                .connect(&url)
                .await
                .ok()
        }

        #[tokio::test]
        async fn test_postgres_get_or_create_same_key_returns_same_instance() {
            let Some(pool) = try_pool().await else {
                return; // skip when POSTGRES_URL not set
            };
            let repo = PostgresChannelRepository::from_shared_pool(pool).await.unwrap();
            let ctx = RequestContext::new_without_auth("pg-t1".to_string(), "ns".to_string());
            let ch1 = repo.get_or_create(&ctx, "actor-pg").await.unwrap();
            let ch2 = repo.get_or_create(&ctx, "actor-pg").await.unwrap();
            assert!(Arc::ptr_eq(&ch1, &ch2));
        }

        #[tokio::test]
        async fn test_postgres_different_tenants_get_different_channels() {
            let Some(pool) = try_pool().await else {
                return;
            };
            let repo = PostgresChannelRepository::from_shared_pool(pool).await.unwrap();
            let ctx_a = RequestContext::new_without_auth("pg-ta".to_string(), "ns".to_string());
            let ctx_b = RequestContext::new_without_auth("pg-tb".to_string(), "ns".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "actor-1").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "actor-1").await.unwrap();
            assert!(!Arc::ptr_eq(&ch_a, &ch_b));
        }

        #[tokio::test]
        async fn test_postgres_send_receive_tenant_isolated() {
            let Some(pool) = try_pool().await else {
                return;
            };
            let repo = PostgresChannelRepository::from_shared_pool(pool).await.unwrap();
            let ctx_a = RequestContext::new_without_auth("pg-iso-a".to_string(), "ns".to_string());
            let ctx_b = RequestContext::new_without_auth("pg-iso-b".to_string(), "ns".to_string());
            let ch_a = repo.get_or_create(&ctx_a, "q").await.unwrap();
            let ch_b = repo.get_or_create(&ctx_b, "q").await.unwrap();

            ch_a.send(Message {
                id: ulid::Ulid::new().to_string(),
                channel: "q".to_string(),
                payload: b"from-a".to_vec(),
                ..Default::default()
            })
            .await
            .unwrap();

            let from_b = ch_b.receive(10).await.unwrap();
            assert!(from_b.is_empty(), "tenant-b must not see tenant-a's messages");

            let from_a = ch_a.receive(10).await.unwrap();
            assert_eq!(from_a.len(), 1);
            assert_eq!(from_a[0].payload, b"from-a");
        }

        #[tokio::test]
        async fn test_postgres_overwrite_updates_in_place() {
            let Some(pool) = try_pool().await else {
                return;
            };
            let repo = PostgresChannelRepository::from_shared_pool(pool).await.unwrap();
            let ctx =
                RequestContext::new_without_auth("pg-ow".to_string(), "ns".to_string());
            let ch = repo.get_or_create(&ctx, "ow-q").await.unwrap();

            let msg_id = ulid::Ulid::new().to_string();
            ch.send(Message {
                id: msg_id.clone(),
                channel: "ow-q".to_string(),
                payload: b"original".to_vec(),
                idempotency_key: "k1".to_string(),
                conflict_resolution: 1,
                ..Default::default()
            })
            .await
            .unwrap();

            // OVERWRITE: second send with same key should update, not insert
            ch.send(Message {
                id: ulid::Ulid::new().to_string(),
                channel: "ow-q".to_string(),
                payload: b"updated".to_vec(),
                idempotency_key: "k1".to_string(),
                conflict_resolution: 1,
                ..Default::default()
            })
            .await
            .unwrap();

            let msgs = ch.receive(10).await.unwrap();
            assert_eq!(msgs.len(), 1, "OVERWRITE must not add a duplicate row");
            assert_eq!(msgs[0].id, msg_id, "original queue position preserved");
            assert_eq!(msgs[0].payload, b"updated");
        }

        #[tokio::test]
        async fn test_postgres_ack_removes_message() {
            let Some(pool) = try_pool().await else {
                return;
            };
            let repo = PostgresChannelRepository::from_shared_pool(pool).await.unwrap();
            let ctx = RequestContext::new_without_auth("pg-ack".to_string(), "ns".to_string());
            let ch = repo.get_or_create(&ctx, "ack-q").await.unwrap();

            ch.send(Message {
                id: ulid::Ulid::new().to_string(),
                channel: "ack-q".to_string(),
                payload: b"data".to_vec(),
                ..Default::default()
            })
            .await
            .unwrap();

            let msgs = ch.receive(1).await.unwrap();
            assert_eq!(msgs.len(), 1);
            ch.ack(&msgs[0].id).await.unwrap();

            let after = ch.receive(1).await.unwrap();
            assert!(after.is_empty());
        }
    }
}
