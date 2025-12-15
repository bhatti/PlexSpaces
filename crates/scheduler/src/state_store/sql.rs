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

//! SQL-based scheduling state store implementations (SQLite and PostgreSQL).

use crate::state_store::SchedulingStateStore;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use plexspaces_proto::v1::actor::ActorResourceRequirements;
use plexspaces_proto::scheduling::v1::{SchedulingRequest, SchedulingStatus};
use prost::Message;
use sqlx::{Pool, Sqlite};
use std::error::Error;

/// SQLite-based scheduling state store.
#[derive(Clone)]
pub struct SqliteSchedulingStateStore {
    pool: Pool<Sqlite>,
}

impl SqliteSchedulingStateStore {
    /// Create a new SQLite scheduling state store.
    ///
    /// ## Arguments
    /// - `path`: Database file path (use ":memory:" for in-memory database)
    pub async fn new(path: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&format!("sqlite:{}", path))
            .await?;

        // Run migrations
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await?;

        Ok(Self { pool })
    }

    /// Create in-memory store for testing
    pub async fn new_in_memory() -> Result<Self, Box<dyn Error + Send + Sync>> {
        Self::new(":memory:").await
    }
}

#[async_trait]
impl SchedulingStateStore for SqliteSchedulingStateStore {
    async fn store_request(&self, request: SchedulingRequest) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Serialize requirements (using prost Message encoding, then base64)
        let requirements_json = if let Some(ref req) = request.requirements {
            let mut buf = Vec::new();
            req.encode(&mut buf)?;
            general_purpose::STANDARD.encode(&buf)
        } else {
            return Err("requirements field is required".into());
        };

        // Convert status to string
        let status_str = match SchedulingStatus::try_from(request.status) {
            Ok(SchedulingStatus::SchedulingStatusPending) => "PENDING",
            Ok(SchedulingStatus::SchedulingStatusScheduled) => "SCHEDULED",
            Ok(SchedulingStatus::SchedulingStatusFailed) => "FAILED",
            _ => "UNSPECIFIED",
        };

        // Convert timestamp to Unix seconds
        let created_at_secs = request.created_at.map(|ts| ts.seconds).unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
        });

        sqlx::query(
            r#"
            INSERT INTO scheduling_requests
            (request_id, status, requirements_json, namespace, tenant_id,
             selected_node_id, actor_id, error_message, metadata_json,
             created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&request.request_id)
        .bind(status_str)
        .bind(&requirements_json)
        .bind(&request.namespace)
        .bind(&request.tenant_id)
        .bind(&request.selected_node_id)
        .bind(&request.actor_id)
        .bind(&request.error_message)
        .bind(None::<String>) // metadata_json - not in proto, but kept in schema for extensibility
        .bind(created_at_secs)
        .bind(created_at_secs) // updated_at same as created_at initially
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_request(&self, request_id: &str) -> Result<Option<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        let row = sqlx::query(
            r#"
            SELECT request_id, status, requirements_json, namespace, tenant_id,
                   selected_node_id, actor_id, error_message, metadata_json,
                   created_at, scheduled_at, completed_at
            FROM scheduling_requests
            WHERE request_id = ?
            "#,
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(row_to_scheduling_request(row)?))
        } else {
            Ok(None)
        }
    }

    async fn update_request(&self, request: SchedulingRequest) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Serialize requirements (using prost Message encoding, then base64)
        let requirements_json = if let Some(ref req) = request.requirements {
            let mut buf = Vec::new();
            req.encode(&mut buf)?;
            general_purpose::STANDARD.encode(&buf)
        } else {
            return Err("requirements field is required".into());
        };

        // Convert status to string
        let status_str = match SchedulingStatus::try_from(request.status) {
            Ok(SchedulingStatus::SchedulingStatusPending) => "PENDING",
            Ok(SchedulingStatus::SchedulingStatusScheduled) => "SCHEDULED",
            Ok(SchedulingStatus::SchedulingStatusFailed) => "FAILED",
            _ => "UNSPECIFIED",
        };

        // Convert timestamps to Unix seconds (SQLite stores as INTEGER or TEXT)
        // For now, we'll store as Unix timestamp (seconds since epoch)
        let scheduled_at_secs = request.scheduled_at.map(|ts| ts.seconds);
        let completed_at_secs = request.completed_at.map(|ts| ts.seconds);

        sqlx::query(
            r#"
            UPDATE scheduling_requests
            SET status = ?,
                requirements_json = ?,
                namespace = ?,
                tenant_id = ?,
                selected_node_id = ?,
                actor_id = ?,
                error_message = ?,
                scheduled_at = COALESCE(?, CASE WHEN ? = 'SCHEDULED' AND scheduled_at IS NULL THEN CAST(strftime('%s', 'now') AS INTEGER) ELSE scheduled_at END),
                completed_at = COALESCE(?, CASE WHEN ? IN ('SCHEDULED', 'FAILED') AND completed_at IS NULL THEN CAST(strftime('%s', 'now') AS INTEGER) ELSE completed_at END),
                updated_at = CAST(strftime('%s', 'now') AS INTEGER)
            WHERE request_id = ?
            "#,
        )
        .bind(&status_str)
        .bind(&requirements_json)
        .bind(&request.namespace)
        .bind(&request.tenant_id)
        .bind(&request.selected_node_id)
        .bind(&request.actor_id)
        .bind(&request.error_message)
        .bind(&scheduled_at_secs)
        .bind(&status_str)
        .bind(&completed_at_secs)
        .bind(&status_str)
        .bind(&request.request_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn query_pending_requests(&self) -> Result<Vec<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        let rows = sqlx::query(
            r#"
            SELECT request_id, status, requirements_json, namespace, tenant_id,
                   selected_node_id, actor_id, error_message, metadata_json,
                   created_at, scheduled_at, completed_at
            FROM scheduling_requests
            WHERE status = 'PENDING'
            ORDER BY created_at ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut requests = Vec::new();
        for row in rows {
            requests.push(row_to_scheduling_request(row)?);
        }

        Ok(requests)
    }
}

// Helper to convert SQL row to SchedulingRequest
fn row_to_scheduling_request(row: sqlx::sqlite::SqliteRow) -> Result<SchedulingRequest, Box<dyn Error + Send + Sync>> {
    use sqlx::Row;

    let request_id: String = row.get("request_id");
    let status_str: String = row.get("status");
    let requirements_json: String = row.get("requirements_json");
    let namespace: Option<String> = row.get("namespace");
    let tenant_id: Option<String> = row.get("tenant_id");
    let selected_node_id: Option<String> = row.get("selected_node_id");
    let actor_id: Option<String> = row.get("actor_id");
    let error_message: Option<String> = row.get("error_message");
    let _metadata_json: Option<String> = row.get("metadata_json");

    // Parse status
    let status = match status_str.as_str() {
        "PENDING" => SchedulingStatus::SchedulingStatusPending as i32,
        "SCHEDULED" => SchedulingStatus::SchedulingStatusScheduled as i32,
        "FAILED" => SchedulingStatus::SchedulingStatusFailed as i32,
        _ => SchedulingStatus::SchedulingStatusUnspecified as i32,
    };

    // Parse requirements (proto message - decode from base64)
    let requirements_bytes = general_purpose::STANDARD.decode(&requirements_json)?;
    let requirements = ActorResourceRequirements::decode(
        requirements_bytes.as_slice()
    )?;

    // Parse timestamps from row
    let created_at: Option<i64> = row.try_get("created_at").ok();
    let scheduled_at: Option<i64> = row.try_get("scheduled_at").ok();
    let completed_at: Option<i64> = row.try_get("completed_at").ok();

    // Convert Unix timestamps to protobuf Timestamp
    let created_at_ts = created_at.map(|secs| plexspaces_proto::prost_types::Timestamp {
        seconds: secs,
        nanos: 0,
    });
    let scheduled_at_ts = scheduled_at.map(|secs| plexspaces_proto::prost_types::Timestamp {
        seconds: secs,
        nanos: 0,
    });
    let completed_at_ts = completed_at.map(|secs| plexspaces_proto::prost_types::Timestamp {
        seconds: secs,
        nanos: 0,
    });

    Ok(SchedulingRequest {
        request_id,
        requirements: Some(requirements),
        namespace: namespace.unwrap_or_default(),
        tenant_id: tenant_id.unwrap_or_default(),
        status,
        selected_node_id: selected_node_id.unwrap_or_default(),
        actor_id: actor_id.unwrap_or_default(),
        error_message: error_message.unwrap_or_default(),
        created_at: created_at_ts,
        scheduled_at: scheduled_at_ts,
        completed_at: completed_at_ts,
    })
}

