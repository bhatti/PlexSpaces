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

//! Cron Service trait for durable scheduled jobs
//!
//! ## Purpose
//! Defines the interface for registering and managing cron-based scheduled jobs.
//! Jobs are durable (persisted to KV store) and use distributed locking for
//! exactly-once execution in multi-node deployments.
//!
//! ## Design
//! - Jobs are persisted in KeyValueStore for durability across restarts
//! - Distributed lock (LockManager) ensures only one node fires each job
//! - Cron expressions follow standard 6-field format (sec min hour day month weekday)
//! - Jobs fire by sending a message to a target actor via ActorService

use async_trait::async_trait;

use crate::RequestContext;

/// Cron schedule specification
#[derive(Debug, Clone)]
pub struct CronSchedule {
    /// Cron expression (standard 6-field: sec min hour day month weekday)
    /// Examples:
    /// - "0 */5 * * * *" = every 5 minutes
    /// - "0 0 * * * *" = every hour
    /// - "0 0 0 * * *" = daily at midnight
    /// - "0 30 9 * * MON-FRI" = weekdays at 9:30 AM
    pub expression: String,
    /// Timezone for the schedule (e.g., "UTC", "America/New_York")
    /// Empty string defaults to UTC
    pub timezone: String,
}

/// Scheduled job definition
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    /// Unique job ID (must be unique per tenant/namespace)
    pub job_id: String,
    /// Target actor to receive the trigger message
    pub target_actor: String,
    /// Message type to send to the target actor
    pub msg_type: String,
    /// Message payload to send
    pub payload: Vec<u8>,
    /// Cron schedule
    pub schedule: CronSchedule,
    /// Whether job is currently enabled
    pub enabled: bool,
}

/// Job status information
#[derive(Debug, Clone)]
pub struct JobStatus {
    /// Job ID
    pub job_id: String,
    /// Whether job is enabled
    pub enabled: bool,
    /// Timestamp of last execution (None if never run)
    pub last_run_at: Option<u64>,
    /// Timestamp of next scheduled execution (None if disabled)
    pub next_run_at: Option<u64>,
    /// Total number of times the job has been executed
    pub run_count: u64,
    /// Last error message (None if last run succeeded)
    pub last_error: Option<String>,
}

/// Cron service errors
#[derive(Debug, thiserror::Error)]
pub enum CronError {
    /// Invalid cron expression
    #[error("Invalid cron expression: {0}")]
    InvalidExpression(String),

    /// Job not found
    #[error("Job not found: {0}")]
    NotFound(String),

    /// Job already exists
    #[error("Job already exists: {0}")]
    AlreadyExists(String),

    /// Persistence error
    #[error("Persistence error: {0}")]
    PersistenceError(String),

    /// Service not started
    #[error("Cron service not started")]
    NotStarted,

    /// Other error
    #[error("Cron error: {0}")]
    Other(String),
}

/// Trait for durable cron-based job scheduling.
///
/// ## Purpose
/// Provides cron-based job scheduling with:
/// - Durable job persistence (survives node restarts)
/// - Exactly-once execution via distributed lock
/// - Cron expression support
/// - Per-tenant/namespace job isolation
///
/// ## Usage
/// ```rust,ignore
/// let job = ScheduledJob {
///     job_id: "daily-report".to_string(),
///     target_actor: "report-generator@node1".to_string(),
///     msg_type: "generate".to_string(),
///     payload: b"daily".to_vec(),
///     schedule: CronSchedule {
///         expression: "0 0 0 * * *".to_string(),  // midnight daily
///         timezone: "UTC".to_string(),
///     },
///     enabled: true,
/// };
/// cron_service.register_job(&ctx, job).await?;
/// ```
#[async_trait]
pub trait CronService: Send + Sync {
    /// Register a cron job
    ///
    /// ## Arguments
    /// * `ctx` - Request context with tenant/namespace
    /// * `job` - Job definition
    ///
    /// ## Returns
    /// Job ID on success
    async fn register_job(
        &self,
        ctx: &RequestContext,
        job: ScheduledJob,
    ) -> Result<String, CronError>;

    /// Unregister (delete) a cron job
    ///
    /// ## Arguments
    /// * `ctx` - Request context
    /// * `job_id` - ID of the job to remove
    async fn unregister_job(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> Result<(), CronError>;

    /// Enable or disable a job
    ///
    /// ## Arguments
    /// * `ctx` - Request context
    /// * `job_id` - ID of the job
    /// * `enabled` - New enabled state
    async fn set_job_enabled(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        enabled: bool,
    ) -> Result<(), CronError>;

    /// Get status of a specific job
    ///
    /// ## Arguments
    /// * `ctx` - Request context
    /// * `job_id` - ID of the job
    ///
    /// ## Returns
    /// Job status if found, None if not found
    async fn get_job_status(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> Result<Option<JobStatus>, CronError>;

    /// List all jobs for the current tenant/namespace
    ///
    /// ## Arguments
    /// * `ctx` - Request context
    ///
    /// ## Returns
    /// List of all job statuses
    async fn list_jobs(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<JobStatus>, CronError>;
}
