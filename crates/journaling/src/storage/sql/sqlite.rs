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
use super::*;

/// SQLite-based journal storage.
///
/// ## Purpose
/// Persistent journal storage using SQLite, ideal for:
/// - Single-node deployments
/// - Edge computing
/// - Unit/integration tests
/// - Development environments
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_journaling::{JournalStorage, sql::SqliteJournalStorage};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Persistent database
/// let storage = SqliteJournalStorage::new("/tmp/journal.db").await?;
///
/// // In-memory database (for testing) - RECOMMENDED for tests
/// let storage = SqliteJournalStorage::new(":memory:").await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SqliteJournalStorage {
    pool: Pool<Sqlite>,
    /// Sequence counters by actor_id (cached for performance)
    sequences: Arc<RwLock<HashMap<String, u64>>>,
}

/// Create journal + checkpoint + actor_events + reminders schema for :memory: SQLite.
async fn run_memory_schema_sqlite(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS journal_entries (
            id TEXT PRIMARY KEY, actor_id TEXT NOT NULL, sequence BIGINT NOT NULL, timestamp BIGINT NOT NULL,
            correlation_id TEXT, entry_type TEXT NOT NULL, entry_data BLOB NOT NULL, UNIQUE(actor_id, sequence))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_journal_actor_sequence ON journal_entries (actor_id, sequence)")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_journal_timestamp ON journal_entries (timestamp)")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_journal_entry_type ON journal_entries (entry_type)",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS checkpoints (
            actor_id TEXT NOT NULL, sequence BIGINT NOT NULL, timestamp BIGINT NOT NULL, state_data BLOB NOT NULL,
            compression INTEGER NOT NULL DEFAULT 0, metadata TEXT, state_schema_version INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (actor_id, sequence))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_checkpoint_actor_latest ON checkpoints (actor_id, sequence DESC)")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS actor_events (
            id TEXT PRIMARY KEY, actor_id TEXT NOT NULL, sequence BIGINT NOT NULL, event_type TEXT NOT NULL,
            event_data BLOB NOT NULL, timestamp BIGINT NOT NULL, caused_by TEXT, metadata TEXT, UNIQUE(actor_id, sequence))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_actor_events_actor_sequence ON actor_events(actor_id, sequence)")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_actor_events_timestamp ON actor_events(timestamp)")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_actor_events_caused_by ON actor_events(caused_by) WHERE caused_by IS NOT NULL")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS reminders (
            actor_id TEXT NOT NULL, reminder_name TEXT NOT NULL, interval_seconds INTEGER, interval_nanos INTEGER,
            first_fire_time_seconds INTEGER, first_fire_time_nanos INTEGER, callback_data BLOB,
            persist_across_activations INTEGER NOT NULL DEFAULT 1, max_occurrences INTEGER NOT NULL DEFAULT 0,
            last_fired_seconds INTEGER, last_fired_nanos INTEGER, next_fire_time_seconds INTEGER, next_fire_time_nanos INTEGER,
            fire_count INTEGER NOT NULL DEFAULT 0, is_active INTEGER NOT NULL DEFAULT 1, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL,
            PRIMARY KEY(actor_id, reminder_name))"#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_reminders_next_fire_time ON reminders(next_fire_time_seconds, next_fire_time_nanos) WHERE is_active = 1")
        .execute(pool).await.map_err(|e| e.to_string())?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_reminders_actor_id ON reminders(actor_id) WHERE is_active = 1")
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

impl SqliteJournalStorage {
    /// Create a new SQLite journal storage.
    ///
    /// ## Arguments
    /// - `path`: Database file path (use ":memory:" for in-memory database)
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_journaling::sql::SqliteJournalStorage;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Persistent database
    /// let storage = SqliteJournalStorage::new("/tmp/journal.db").await?;
    ///
    /// // In-memory database (RECOMMENDED for tests)
    /// let storage = SqliteJournalStorage::new(":memory:").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(path: &str) -> JournalResult<Self> {
        use tracing::{debug, error, info};

        // SQLite connection string format for sqlx 0.7:
        // - ":memory:" for in-memory database
        // - "sqlite:///absolute/path?mode=rwc" for file-based (creates if not exists)
        let (connection_string, display_path) = if path == ":memory:" {
            ("sqlite::memory:".to_string(), ":memory:".to_string())
        } else if path.starts_with("sqlite:") {
            // Already has sqlite: scheme, ensure mode=rwc is present
            let url = if path.contains("mode=") {
                path.to_string()
            } else if path.contains('?') {
                format!("{}&mode=rwc", path)
            } else {
                format!("{}?mode=rwc", path)
            };
            (url, path.to_string())
        } else {
            // Build proper sqlite:// URL
            // For absolute paths: sqlite:///absolute/path?mode=rwc
            // For relative paths: sqlite:relative/path?mode=rwc
            let conn_str = if path.starts_with('/') {
                format!("sqlite://{}?mode=rwc", path)
            } else {
                format!("sqlite:{}?mode=rwc", path)
            };
            (conn_str, path.to_string())
        };

        debug!(db_path = %display_path, db_url = %connection_string, "Connecting to journal database");

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&connection_string)
            .await
            .map_err(|e| {
                error!(db_path = %display_path, error = %e, "Failed to connect to journal database");
                JournalError::Storage(e.to_string())
            })?;

        // Production SQLite PRAGMAs (ref: micrologics.org/blog/sqlite-in-production-*)
        sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA synchronous=NORMAL").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA busy_timeout=500").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA cache_size=-64000").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA mmap_size=1073741824").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA journal_size_limit=67108864").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;
        // Disable auto-checkpoint to prevent WAL stalls under high write throughput.
        sqlx::query("PRAGMA wal_autocheckpoint=0").execute(&pool).await.map_err(|e| JournalError::Storage(e.to_string()))?;

        // Get SQLite version for logging
        let db_version = sqlx::query_scalar::<_, String>("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        run_memory_schema_sqlite(&pool).await.map_err(|e| {
            error!(db_path = %display_path, error = %e, "Journal schema creation failed");
            JournalError::Storage(e)
        })?;

        info!(
            db_path = %display_path,
            db_version = %format!("SQLite {}", db_version),
            "Journal storage connected"
        );

        Ok(Self {
            pool,
            sequences: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get next event sequence number for actor (cached)
    async fn next_event_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        // Use a separate key for event sequences (e.g., "actor_id:events")
        let cache_key = format!("{}:events", actor_id);

        if let Some(seq) = sequences.get_mut(&cache_key) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM actor_events
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(cache_key, next + 1);
        Ok(next)
    }

    /// Get next sequence number for actor (cached)
    async fn next_sequence(&self, actor_id: &str) -> JournalResult<u64> {
        let mut sequences = self.sequences.write().await;

        if let Some(seq) = sequences.get_mut(actor_id) {
            let current = *seq;
            *seq += 1;
            return Ok(current);
        }

        // Cache miss - query database for max sequence
        let row = sqlx::query(
            r#"
            SELECT COALESCE(MAX(sequence), 0) as max_seq
            FROM journal_entries
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let max_seq: i64 = row.get("max_seq");
        let next = (max_seq + 1) as u64;

        sequences.insert(actor_id.to_string(), next + 1);
        Ok(next)
    }
}

#[async_trait]
impl JournalStorage for SqliteJournalStorage {
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64> {
        let mut entry = entry.clone();

        // Assign sequence if not set, or sync cache if explicit sequence provided
        let sequence = if entry.sequence == 0 {
            let assigned_seq = self.next_sequence(&entry.actor_id).await?;
            // Update entry.sequence so it's serialized correctly
            entry.sequence = assigned_seq;
            assigned_seq
        } else {
            // Entry has explicit sequence - update cache to match to prevent conflicts
            let mut sequences = self.sequences.write().await;
            let next_seq = entry.sequence + 1;
            sequences.insert(entry.actor_id.clone(), next_seq);
            entry.sequence
        };
        let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

        // Serialize the entire entry to protobuf bytes
        let mut entry_bytes = Vec::new();
        entry
            .encode(&mut entry_bytes)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        // Determine entry_type from oneof
        use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
        let entry_type = match &entry.entry {
            Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
            Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
            Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
            Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
            Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
            Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
            Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
            Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
            None => "Unknown",
        };

        sqlx::query(
            r#"
            INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&entry.id)
        .bind(&entry.actor_id)
        .bind(sequence as i64)
        .bind(timestamp)
        .bind(&entry.correlation_id)
        .bind(entry_type)
        .bind(&entry_bytes)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)> {
        if entries.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, entry) in entries.iter().enumerate() {
            let mut entry = entry.clone();

            // Assign sequence if not set, or sync cache if explicit sequence provided
            let seq = if entry.sequence == 0 {
                self.next_sequence(&entry.actor_id).await?
            } else {
                // Entry has explicit sequence - update cache to match to prevent conflicts
                let mut sequences = self.sequences.write().await;
                let next_seq = entry.sequence + 1;
                sequences.insert(entry.actor_id.clone(), next_seq);
                entry.sequence
            };
            entry.sequence = seq;

            if i == 0 {
                first_sequence = entry.sequence;
            }
            last_sequence = entry.sequence;

            let timestamp = proto_timestamp_to_unix_ms(&entry.timestamp);

            // Serialize the entire entry to protobuf bytes
            let mut entry_bytes = Vec::new();
            entry
                .encode(&mut entry_bytes)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
            let entry_type = match &entry.entry {
                Some(JournalEntryVariant::MessageReceived(_)) => "MessageReceived",
                Some(JournalEntryVariant::MessageProcessed(_)) => "MessageProcessed",
                Some(JournalEntryVariant::StateChanged(_)) => "StateChanged",
                Some(JournalEntryVariant::SideEffectExecuted(_)) => "SideEffectExecuted",
                Some(JournalEntryVariant::TimerScheduled(_)) => "TimerScheduled",
                Some(JournalEntryVariant::TimerFired(_)) => "TimerFired",
                Some(JournalEntryVariant::PromiseCreated(_)) => "PromiseCreated",
                Some(JournalEntryVariant::PromiseResolved(_)) => "PromiseResolved",
                None => "Unknown",
            };

            sqlx::query(
                r#"
                INSERT INTO journal_entries (id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&entry.id)
            .bind(&entry.actor_id)
            .bind(entry.sequence as i64)
            .bind(timestamp)
            .bind(&entry.correlation_id)
            .bind(entry_type)
            .bind(&entry_bytes)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Serialization(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, entries.len()))
    }

    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, timestamp, correlation_id, entry_type, entry_data
            FROM journal_entries
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut entries = Vec::new();

        for row in rows {
            let entry_bytes: Vec<u8> = row.get("entry_data");

            // Deserialize the entire entry from protobuf bytes
            let entry = JournalEntry::decode(&entry_bytes[..])
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            entries.push(entry);
        }

        Ok(entries)
    }

    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        let row = sqlx::query(
            r#"
            SELECT actor_id, sequence, timestamp, state_data, compression, metadata, state_schema_version
            FROM checkpoints
            WHERE actor_id = ?
            ORDER BY sequence DESC
            LIMIT 1
            "#,
        )
        .bind(actor_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            if matches!(e, sqlx::Error::RowNotFound) {
                JournalError::CheckpointNotFound(actor_id.to_string())
            } else {
                JournalError::Storage(e.to_string())
            }
        })?;

        let actor_id: String = row.get("actor_id");
        let sequence: i64 = row.get("sequence");
        let timestamp: i64 = row.get("timestamp");
        let state_data: Vec<u8> = row.get("state_data");
        let compression: i32 = row.get("compression");
        let metadata: Option<String> = row.get("metadata");
        // Read state_schema_version (default to 1 if column doesn't exist for backward compatibility)
        let state_schema_version: i32 = row.try_get("state_schema_version").unwrap_or(1);

        let metadata_map: HashMap<String, String> = if let Some(json) = metadata {
            serde_json::from_str(&json).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Checkpoint {
            actor_id,
            sequence: sequence as u64,
            timestamp: unix_ms_to_proto_timestamp(timestamp),
            state_data,
            compression,
            metadata: metadata_map,
            state_schema_version: state_schema_version as u32,
        })
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        let timestamp = proto_timestamp_to_unix_ms(&checkpoint.timestamp);
        let metadata_json = serde_json::to_string(&checkpoint.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO checkpoints (actor_id, sequence, timestamp, state_data, compression, metadata, state_schema_version)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&checkpoint.actor_id)
        .bind(checkpoint.sequence as i64)
        .bind(timestamp)
        .bind(&checkpoint.state_data)
        .bind(checkpoint.compression)
        .bind(metadata_json)
        .bind(checkpoint.state_schema_version as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM journal_entries
            WHERE actor_id = ? AND sequence <= ?
            "#,
        )
        .bind(actor_id)
        .bind(sequence as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(result.rows_affected())
    }

    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats> {
        let mut stats = JournalStats {
            total_entries: 0,
            total_checkpoints: 0,
            storage_bytes: 0,
            entries_by_actor: HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        };

        if let Some(aid) = actor_id {
            // Stats for specific actor
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count, MIN(timestamp) as oldest, MAX(timestamp) as newest
                FROM journal_entries
                WHERE actor_id = ?
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;
            stats.entries_by_actor.insert(aid.to_string(), count as u64);

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Checkpoint count
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                WHERE actor_id = ?
                "#,
            )
            .bind(aid)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        } else {
            // Global stats
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count, MIN(timestamp) as oldest, MAX(timestamp) as newest
                FROM journal_entries
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            let count: i64 = row.get("count");
            stats.total_entries = count as u64;

            // Only get timestamps if there are entries (MIN/MAX return NULL for empty sets)
            if count > 0 {
                if let Ok(oldest) = row.try_get::<i64, _>("oldest") {
                    stats.oldest_entry = unix_ms_to_proto_timestamp(oldest);
                }
                if let Ok(newest) = row.try_get::<i64, _>("newest") {
                    stats.newest_entry = unix_ms_to_proto_timestamp(newest);
                }
            }

            // Per-actor counts
            let rows = sqlx::query(
                r#"
                SELECT actor_id, COUNT(*) as count
                FROM journal_entries
                GROUP BY actor_id
                "#,
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            for row in rows {
                let actor_id: String = row.get("actor_id");
                let count: i64 = row.get("count");
                stats.entries_by_actor.insert(actor_id, count as u64);
            }

            // Total checkpoints
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as count
                FROM checkpoints
                "#,
            )
            .fetch_one(&self.pool)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

            stats.total_checkpoints = row.get::<i64, _>("count") as u64;
        }

        Ok(stats)
    }

    async fn flush(&self) -> JournalResult<()> {
        // SQLite with WAL mode: Ensure all writes are visible
        // For in-memory databases, WAL mode auto-commits, but we ensure consistency
        // Note: wal_checkpoint may fail for in-memory databases, so we ignore errors
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await;
        Ok(())
    }

    async fn purge_actor(&self, actor_id: &str) -> JournalResult<u64> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let journal_deleted = sqlx::query("DELETE FROM journal_entries WHERE actor_id = ?")
            .bind(actor_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?
            .rows_affected();
        let checkpoint_deleted = sqlx::query("DELETE FROM checkpoints WHERE actor_id = ?")
            .bind(actor_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?
            .rows_affected();
        let event_deleted = sqlx::query("DELETE FROM actor_events WHERE actor_id = ?")
            .bind(actor_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?
            .rows_affected();
        let reminder_deleted = sqlx::query("DELETE FROM reminders WHERE actor_id = ?")
            .bind(actor_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?
            .rows_affected();

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut sequences = self.sequences.write().await;
        sequences.remove(actor_id);
        sequences.remove(&format!("{}:events", actor_id));

        Ok(journal_deleted + checkpoint_deleted + event_deleted + reminder_deleted)
    }

    async fn purge_namespace(&self, namespace: &str) -> JournalResult<u64> {
        let qualified_pattern = format!("%::{}@%", namespace);
        let simple_pattern = format!("%:{}@%", namespace);

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let journal_deleted =
            sqlx::query("DELETE FROM journal_entries WHERE actor_id LIKE ? OR actor_id LIKE ?")
                .bind(&qualified_pattern)
                .bind(&simple_pattern)
                .execute(&mut *tx)
                .await
                .map_err(|e| JournalError::Storage(e.to_string()))?
                .rows_affected();
        let checkpoint_deleted =
            sqlx::query("DELETE FROM checkpoints WHERE actor_id LIKE ? OR actor_id LIKE ?")
                .bind(&qualified_pattern)
                .bind(&simple_pattern)
                .execute(&mut *tx)
                .await
                .map_err(|e| JournalError::Storage(e.to_string()))?
                .rows_affected();
        let event_deleted =
            sqlx::query("DELETE FROM actor_events WHERE actor_id LIKE ? OR actor_id LIKE ?")
                .bind(&qualified_pattern)
                .bind(&simple_pattern)
                .execute(&mut *tx)
                .await
                .map_err(|e| JournalError::Storage(e.to_string()))?
                .rows_affected();
        let reminder_deleted =
            sqlx::query("DELETE FROM reminders WHERE actor_id LIKE ? OR actor_id LIKE ?")
                .bind(&qualified_pattern)
                .bind(&simple_pattern)
                .execute(&mut *tx)
                .await
                .map_err(|e| JournalError::Storage(e.to_string()))?
                .rows_affected();

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(journal_deleted + checkpoint_deleted + event_deleted + reminder_deleted)
    }

    // ==================== Event Sourcing Methods ====================

    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64> {
        let mut event = event.clone();

        // Assign sequence if not set
        if event.sequence == 0 {
            event.sequence = self.next_event_sequence(&event.actor_id).await?;
        }

        let sequence = event.sequence;
        let timestamp = proto_timestamp_to_unix_ms(&event.timestamp);

        // Serialize event data
        let event_data_bytes = event.event_data.clone();

        // Serialize metadata to JSON
        let metadata_json = serde_json::to_string(&event.metadata)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.id)
        .bind(&event.actor_id)
        .bind(sequence as i64)
        .bind(&event.event_type)
        .bind(&event_data_bytes)
        .bind(timestamp)
        .bind(&event.caused_by)
        .bind(&metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(sequence)
    }

    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)> {
        if events.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut first_sequence = 0u64;
        let mut last_sequence = 0u64;

        for (i, event) in events.iter().enumerate() {
            let mut event = event.clone();

            if event.sequence == 0 {
                event.sequence = self.next_event_sequence(&event.actor_id).await?;
            }

            if i == 0 {
                first_sequence = event.sequence;
            }
            last_sequence = event.sequence;

            let timestamp = proto_timestamp_to_unix_ms(&event.timestamp);
            let metadata_json = serde_json::to_string(&event.metadata)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            sqlx::query(
                r#"
                INSERT INTO actor_events (id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&event.id)
            .bind(&event.actor_id)
            .bind(event.sequence as i64)
            .bind(&event.event_type)
            .bind(&event.event_data)
            .bind(timestamp)
            .bind(&event.caused_by)
            .bind(&metadata_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;
        }

        tx.commit()
            .await
            .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok((first_sequence, last_sequence, events.len()))
    }

    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_json).unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(events)
    }

    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)> {
        // offset is the number of events to skip (not a sequence number)
        let skip_count = page_request.offset.max(0) as i64;

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.limit.clamp(1, 1000) as i64;

        // Fetch page_size + 1 to check if there's more
        // First filter by from_sequence, then skip offset events, then take page_size + 1
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ? AND sequence >= ?
            ORDER BY sequence ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(actor_id)
        .bind(from_sequence as i64)
        .bind(page_size + 1)
        .bind(skip_count)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_json).unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Calculate next offset: current offset + number of events returned
        let next_offset = skip_count + events.len() as i64;

        let page_response = PageResponse {
            request_id: ulid::Ulid::new().to_string(),
            total_size: 0, // Total size not available without full scan (expensive)
            offset: next_offset as i32,
            limit: page_size as i32,
            has_next: has_more,
        };

        Ok((events, page_response))
    }

    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory> {
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ?
            ORDER BY sequence ASC
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut events = Vec::new();
        let mut latest_sequence = 0u64;
        let mut created_at: Option<prost_types::Timestamp> = None;
        let mut updated_at: Option<prost_types::Timestamp> = None;

        for row in rows {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_json).unwrap_or_default();

            let timestamp = unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp"));
            if created_at.is_none() {
                created_at = timestamp;
            }
            updated_at = timestamp;

            let sequence = row.get::<i64, _>("sequence") as u64;
            latest_sequence = latest_sequence.max(sequence);

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp,
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at: created_at
                .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            updated_at: updated_at
                .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now()))),
            metadata: HashMap::new(),
            page_response: None,
        })
    }

    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory> {
        // offset is the number of events to skip (not a sequence number)
        let skip_count = page_request.offset.max(0) as i64;

        // Validate and clamp limit (1-1000)
        let page_size = page_request.limit.clamp(1, 1000) as i64;

        // Get latest sequence for history metadata
        let latest_row = sqlx::query(
            r#"
            SELECT MAX(sequence) as max_seq, MIN(timestamp) as min_ts, MAX(timestamp) as max_ts
            FROM actor_events
            WHERE actor_id = ?
            "#,
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let latest_sequence = latest_row
            .as_ref()
            .and_then(|r| r.get::<Option<i64>, _>("max_seq"))
            .map(|s| s as u64)
            .unwrap_or(0);

        let created_at = latest_row
            .as_ref()
            .and_then(|r| {
                unix_ms_to_proto_timestamp(r.get::<Option<i64>, _>("min_ts").unwrap_or(0))
            })
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        let updated_at = latest_row
            .as_ref()
            .and_then(|r| {
                unix_ms_to_proto_timestamp(r.get::<Option<i64>, _>("max_ts").unwrap_or(0))
            })
            .or_else(|| Some(prost_types::Timestamp::from(SystemTime::now())));

        // Fetch page_size + 1 to check if there's more
        // Use OFFSET for skip count pagination
        let rows = sqlx::query(
            r#"
            SELECT id, actor_id, sequence, event_type, event_data, timestamp, caused_by, metadata
            FROM actor_events
            WHERE actor_id = ?
            ORDER BY sequence ASC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(actor_id)
        .bind(page_size + 1)
        .bind(skip_count)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let has_more = rows.len() > page_size as usize;
        let rows_to_return = rows.iter().take(page_size as usize);

        let mut events = Vec::new();
        for row in rows_to_return {
            let metadata_json: String = row.get("metadata");
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_json).unwrap_or_default();

            events.push(ActorEvent {
                id: row.get("id"),
                actor_id: row.get("actor_id"),
                sequence: row.get::<i64, _>("sequence") as u64,
                event_type: row.get("event_type"),
                event_data: row.get("event_data"),
                timestamp: unix_ms_to_proto_timestamp(row.get::<i64, _>("timestamp")),
                caused_by: row.get("caused_by"),
                metadata,
            });
        }

        // Calculate next offset: current offset + number of events returned
        let next_offset = skip_count + events.len() as i64;

        let page_response = PageResponse {
            request_id: ulid::Ulid::new().to_string(),
            total_size: 0, // Total size not available without full scan (expensive)
            offset: next_offset as i32,
            limit: page_size as i32,
            has_next: has_more,
        };

        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at,
            updated_at,
            metadata: HashMap::new(),
            page_response: Some(page_response),
        })
    }

    // ==================== Reminder Methods ====================

    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;

        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO reminders (
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active,
                created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(if reg.persist_across_activations { 1 } else { 0 })
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(if reminder_state.is_active { 1 } else { 0 })
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()> {
        sqlx::query(
            r#"
            DELETE FROM reminders
            WHERE actor_id = ? AND reminder_name = ?
            "#,
        )
        .bind(actor_id)
        .bind(reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn unregister_reminder_if_matches(
        &self,
        actor_id: &str,
        reminder_name: &str,
        expected_next_fire_ms: u64,
    ) -> JournalResult<bool> {
        let expected_seconds = (expected_next_fire_ms / 1000) as i64;
        let expected_nanos = ((expected_next_fire_ms % 1000) * 1_000_000) as i32;
        let result = sqlx::query(
            r#"
            DELETE FROM reminders
            WHERE actor_id = ? AND reminder_name = ? AND next_fire_time_seconds = ? AND next_fire_time_nanos = ?
            "#,
        )
        .bind(actor_id)
        .bind(reminder_name)
        .bind(expected_seconds)
        .bind(expected_nanos)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>> {
        let rows = sqlx::query(
            r#"
            SELECT
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE actor_id = ? AND is_active = 1
            "#,
        )
        .bind(actor_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }

    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let now = system_time_to_unix_ms(SystemTime::now());
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;

        let interval_seconds = reg.interval.as_ref().map(|d| d.seconds);
        let interval_nanos = reg.interval.as_ref().map(|d| d.nanos);
        let first_fire_seconds = reg.first_fire_time.as_ref().map(|t| t.seconds);
        let first_fire_nanos = reg.first_fire_time.as_ref().map(|t| t.nanos);
        let last_fired_seconds = reminder_state.last_fired.as_ref().map(|t| t.seconds);
        let last_fired_nanos = reminder_state.last_fired.as_ref().map(|t| t.nanos);
        let next_fire_seconds = reminder_state.next_fire_time.as_ref().map(|t| t.seconds);
        let next_fire_nanos = reminder_state.next_fire_time.as_ref().map(|t| t.nanos);

        sqlx::query(
            r#"
            UPDATE reminders SET
                interval_seconds = ?,
                interval_nanos = ?,
                first_fire_time_seconds = ?,
                first_fire_time_nanos = ?,
                callback_data = ?,
                persist_across_activations = ?,
                max_occurrences = ?,
                last_fired_seconds = ?,
                last_fired_nanos = ?,
                next_fire_time_seconds = ?,
                next_fire_time_nanos = ?,
                fire_count = ?,
                is_active = ?,
                updated_at = ?
            WHERE actor_id = ? AND reminder_name = ?
            "#,
        )
        .bind(interval_seconds)
        .bind(interval_nanos)
        .bind(first_fire_seconds)
        .bind(first_fire_nanos)
        .bind(&reg.callback_data)
        .bind(if reg.persist_across_activations { 1 } else { 0 })
        .bind(reg.max_occurrences)
        .bind(last_fired_seconds)
        .bind(last_fired_nanos)
        .bind(next_fire_seconds)
        .bind(next_fire_nanos)
        .bind(reminder_state.fire_count)
        .bind(if reminder_state.is_active { 1 } else { 0 })
        .bind(now)
        .bind(&reg.actor_id)
        .bind(&reg.reminder_name)
        .execute(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        Ok(())
    }

    async fn query_due_reminders(
        &self,
        before_time: SystemTime,
    ) -> JournalResult<Vec<ReminderState>> {
        let before_ms = system_time_to_unix_ms(before_time);
        let before_seconds = before_ms / 1000;
        let before_nanos = ((before_ms % 1000) * 1_000_000) as i32;

        let rows = sqlx::query(
            r#"
            SELECT 
                actor_id, reminder_name,
                interval_seconds, interval_nanos,
                first_fire_time_seconds, first_fire_time_nanos,
                callback_data,
                persist_across_activations, max_occurrences,
                last_fired_seconds, last_fired_nanos,
                next_fire_time_seconds, next_fire_time_nanos,
                fire_count, is_active
            FROM reminders
            WHERE is_active = 1
            AND (
                next_fire_time_seconds < ? OR
                (next_fire_time_seconds = ? AND next_fire_time_nanos <= ?)
            )
            ORDER BY next_fire_time_seconds ASC, next_fire_time_nanos ASC
            "#,
        )
        .bind(before_seconds)
        .bind(before_seconds)
        .bind(before_nanos)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| JournalError::Storage(e.to_string()))?;

        let mut reminders = Vec::new();
        for row in rows {
            let reminder = row_to_reminder_state(&row)?;
            reminders.push(reminder);
        }

        Ok(reminders)
    }
}
