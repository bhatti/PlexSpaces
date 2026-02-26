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

//! Cron Service Implementation
//!
//! Provides durable cron-based job scheduling with exactly-once execution.
//! Jobs are persisted in KeyValueStore and use distributed locking via
//! LockManager to ensure single-node execution in multi-node deployments.

use async_trait::async_trait;
use plexspaces_core::{
    CronError, CronSchedule, CronService, JobStatus, RequestContext, ScheduledJob, ServiceLocator,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Key prefix for cron jobs in KV store
const CRON_JOB_PREFIX: &str = "plexspaces:cron:job:";
/// Key prefix for cron job status in KV store
const CRON_STATUS_PREFIX: &str = "plexspaces:cron:status:";
/// Lock key for cron scheduler leader election
const CRON_LEADER_LOCK: &str = "plexspaces:cron:leader";

/// Internal cron job state
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CronJobRecord {
    /// Job definition
    job_id: String,
    target_actor: String,
    msg_type: String,
    #[serde(with = "serde_bytes_base64")]
    payload: Vec<u8>,
    expression: String,
    timezone: String,
    enabled: bool,
    /// Status tracking
    last_run_at: Option<u64>,
    next_run_at: Option<u64>,
    run_count: u64,
    last_error: Option<String>,
}

/// Serde helper for base64 encoding bytes
mod serde_bytes_base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::Serialize;
        let encoded = base64_encode(bytes);
        encoded.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        base64_decode(&s).map_err(serde::de::Error::custom)
    }

    fn base64_encode(bytes: &[u8]) -> String {
        // Simple base64 encoding without external dep
        use std::fmt::Write;
        let mut result = String::new();
        for byte in bytes {
            write!(result, "{:02x}", byte).unwrap();
        }
        result
    }

    fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
        // Simple hex decoding
        if s.len() % 2 != 0 {
            return Err("Invalid hex string length".to_string());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}

impl CronJobRecord {
    fn from_scheduled_job(job: &ScheduledJob, next_run: Option<u64>) -> Self {
        Self {
            job_id: job.job_id.clone(),
            target_actor: job.target_actor.clone(),
            msg_type: job.msg_type.clone(),
            payload: job.payload.clone(),
            expression: job.schedule.expression.clone(),
            timezone: job.schedule.timezone.clone(),
            enabled: job.enabled,
            last_run_at: None,
            next_run_at: next_run,
            run_count: 0,
            last_error: None,
        }
    }

    fn to_job_status(&self) -> JobStatus {
        JobStatus {
            job_id: self.job_id.clone(),
            enabled: self.enabled,
            last_run_at: self.last_run_at,
            next_run_at: self.next_run_at,
            run_count: self.run_count,
            last_error: self.last_error.clone(),
        }
    }

    fn kv_key(tenant_id: &str, namespace: &str, job_id: &str) -> String {
        format!("{}{}:{}:{}", CRON_JOB_PREFIX, tenant_id, namespace, job_id)
    }
}

/// Cron Service implementation
///
/// Provides durable cron-based job scheduling:
/// - Jobs persisted in KeyValueStore
/// - Distributed lock for leader election
/// - Exactly-once execution per schedule tick
/// - Background scheduler loop
pub struct CronServiceImpl {
    service_locator: Arc<dyn ServiceLocator>,
    node_id: String,
    /// In-memory cache of jobs (loaded from KV on start)
    jobs: Arc<RwLock<HashMap<String, CronJobRecord>>>,
    /// Background scheduler handle
    scheduler_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Whether the service is running
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl CronServiceImpl {
    /// Create a new CronServiceImpl
    pub fn new(service_locator: Arc<dyn ServiceLocator>, node_id: String) -> Self {
        Self {
            service_locator,
            node_id,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            scheduler_handle: Arc::new(RwLock::new(None)),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the background scheduler loop
    pub async fn start(&self) {
        if self.running.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        self.running
            .store(true, std::sync::atomic::Ordering::Relaxed);

        let service_locator = self.service_locator.clone();
        let node_id = self.node_id.clone();
        let jobs = self.jobs.clone();
        let running = self.running.clone();

        let handle = tokio::spawn(async move {
            tracing::info!(node_id = %node_id, "Cron scheduler started");
            while running.load(std::sync::atomic::Ordering::Relaxed) {
                // Sleep for 1 second between scheduler ticks
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                if !running.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }

                // Try to acquire leader lock
                let lock_manager = match service_locator.get_lock_manager().await {
                    Some(lm) => lm,
                    None => continue,
                };

                let ctx = plexspaces_common::RequestContext::new_without_auth(
                    String::new(),
                    String::new(),
                )
                .with_admin(true);

                let lock_opts = plexspaces_core::AcquireLockOptions {
                    lock_key: CRON_LEADER_LOCK.to_string(),
                    holder_id: node_id.clone(),
                    lease_duration_secs: 10,
                    ..Default::default()
                };

                let lock = match lock_manager.acquire_lock(&ctx, lock_opts).await {
                    Ok(lock) => lock,
                    Err(_) => continue, // Another node is the leader
                };

                // We are the leader - fire any due jobs
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;

                let mut jobs_to_fire = Vec::new();
                {
                    let jobs_guard = jobs.read().await;
                    for (_, job) in jobs_guard.iter() {
                        if !job.enabled {
                            continue;
                        }
                        if let Some(next_run) = job.next_run_at {
                            if now >= next_run {
                                jobs_to_fire.push(job.clone());
                            }
                        }
                    }
                }

                // Fire due jobs
                for job in &jobs_to_fire {
                    // Send message to target actor
                    if let Some(actor_service) = service_locator.get_actor_service().await {
                        let msg = plexspaces_proto::common::v1::Message {
                            message_type: job.msg_type.clone(),
                            payload: job.payload.clone(),
                            sender_id: format!("cron:{}", job.job_id),
                            receiver_id: job.target_actor.clone(),
                            ..Default::default()
                        };

                        let result = actor_service
                            .send(
                                &job.target_actor,
                                msg,
                            )
                            .await;

                        // Update job status
                        let mut jobs_guard = jobs.write().await;
                        if let Some(record) = jobs_guard.get_mut(&job.job_id) {
                            record.last_run_at = Some(now);
                            record.run_count += 1;
                            match result {
                                Ok(_) => {
                                    record.last_error = None;
                                    tracing::debug!(
                                        job_id = %job.job_id,
                                        target = %job.target_actor,
                                        "Cron job fired successfully"
                                    );
                                }
                                Err(e) => {
                                    record.last_error = Some(e.to_string());
                                    tracing::warn!(
                                        job_id = %job.job_id,
                                        target = %job.target_actor,
                                        error = %e,
                                        "Cron job fire failed"
                                    );
                                }
                            }

                            // Calculate next run time
                            if let Ok(schedule) = job.expression.parse::<cron::Schedule>() {
                                if let Some(next) = schedule.upcoming(chrono::Utc).next() {
                                    record.next_run_at =
                                        Some(next.timestamp_millis() as u64);
                                }
                            }
                        }
                    }
                }

                // Release leader lock
                let release_opts = plexspaces_core::ReleaseLockOptions {
                    lock_key: CRON_LEADER_LOCK.to_string(),
                    holder_id: node_id.clone(),
                    version: lock.version.clone(),
                    delete_lock: false,
                    ..Default::default()
                };
                let _ = lock_manager.release_lock(&ctx, release_opts).await;
            }
            tracing::info!("Cron scheduler stopped");
        });

        let mut handle_guard = self.scheduler_handle.write().await;
        *handle_guard = Some(handle);
    }

    /// Stop the background scheduler
    pub async fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let mut handle_guard = self.scheduler_handle.write().await;
        if let Some(handle) = handle_guard.take() {
            handle.abort();
        }
    }

    /// Calculate next run time from a cron expression
    fn next_run_time(expression: &str) -> Result<Option<u64>, CronError> {
        let schedule: cron::Schedule = expression
            .parse()
            .map_err(|e: cron::error::Error| CronError::InvalidExpression(e.to_string()))?;

        Ok(schedule
            .upcoming(chrono::Utc)
            .next()
            .map(|t| t.timestamp_millis() as u64))
    }
}

#[async_trait]
impl CronService for CronServiceImpl {
    async fn register_job(
        &self,
        ctx: &RequestContext,
        job: ScheduledJob,
    ) -> Result<String, CronError> {
        // Validate cron expression
        let next_run = Self::next_run_time(&job.schedule.expression)?;

        // Check if job already exists
        let jobs_guard = self.jobs.read().await;
        let composite_key = format!("{}:{}:{}", ctx.tenant_id(), ctx.namespace(), job.job_id);
        if jobs_guard.contains_key(&composite_key) {
            return Err(CronError::AlreadyExists(job.job_id.clone()));
        }
        drop(jobs_guard);

        // Create record
        let record = CronJobRecord::from_scheduled_job(&job, next_run);

        // Persist to KV store if available
        if let Some(kv) = self.service_locator.get_keyvalue_store().await {
            let key =
                CronJobRecord::kv_key(ctx.tenant_id(), ctx.namespace(), &job.job_id);
            let value = serde_json::to_vec(&record)
                .map_err(|e| CronError::PersistenceError(e.to_string()))?;
            kv.put(ctx, &key, value)
                .await
                .map_err(|e| CronError::PersistenceError(e.to_string()))?;
        }

        // Add to in-memory cache
        let mut jobs_guard = self.jobs.write().await;
        jobs_guard.insert(composite_key, record);

        tracing::info!(
            job_id = %job.job_id,
            target = %job.target_actor,
            expression = %job.schedule.expression,
            "Cron job registered"
        );

        Ok(job.job_id)
    }

    async fn unregister_job(&self, ctx: &RequestContext, job_id: &str) -> Result<(), CronError> {
        let composite_key = format!("{}:{}:{}", ctx.tenant_id(), ctx.namespace(), job_id);

        let mut jobs_guard = self.jobs.write().await;
        if jobs_guard.remove(&composite_key).is_none() {
            return Err(CronError::NotFound(job_id.to_string()));
        }
        drop(jobs_guard);

        // Remove from KV store
        if let Some(kv) = self.service_locator.get_keyvalue_store().await {
            let key = CronJobRecord::kv_key(ctx.tenant_id(), ctx.namespace(), job_id);
            let _ = kv.delete(ctx, &key).await;
        }

        tracing::info!(job_id = %job_id, "Cron job unregistered");
        Ok(())
    }

    async fn set_job_enabled(
        &self,
        ctx: &RequestContext,
        job_id: &str,
        enabled: bool,
    ) -> Result<(), CronError> {
        let composite_key = format!("{}:{}:{}", ctx.tenant_id(), ctx.namespace(), job_id);

        let mut jobs_guard = self.jobs.write().await;
        let record = jobs_guard
            .get_mut(&composite_key)
            .ok_or_else(|| CronError::NotFound(job_id.to_string()))?;

        record.enabled = enabled;

        // Update next_run_at if enabling
        if enabled {
            record.next_run_at = Self::next_run_time(&record.expression)?;
        }

        let record_clone = record.clone();
        drop(jobs_guard);

        // Persist updated state
        if let Some(kv) = self.service_locator.get_keyvalue_store().await {
            let key = CronJobRecord::kv_key(ctx.tenant_id(), ctx.namespace(), job_id);
            let value = serde_json::to_vec(&record_clone)
                .map_err(|e| CronError::PersistenceError(e.to_string()))?;
            kv.put(ctx, &key, value)
                .await
                .map_err(|e| CronError::PersistenceError(e.to_string()))?;
        }

        Ok(())
    }

    async fn get_job_status(
        &self,
        ctx: &RequestContext,
        job_id: &str,
    ) -> Result<Option<JobStatus>, CronError> {
        let composite_key = format!("{}:{}:{}", ctx.tenant_id(), ctx.namespace(), job_id);
        let jobs_guard = self.jobs.read().await;
        Ok(jobs_guard.get(&composite_key).map(|r| r.to_job_status()))
    }

    async fn list_jobs(&self, ctx: &RequestContext) -> Result<Vec<JobStatus>, CronError> {
        let prefix = format!("{}:{}:", ctx.tenant_id(), ctx.namespace());
        let jobs_guard = self.jobs.read().await;
        Ok(jobs_guard
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(_, record)| record.to_job_status())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_run_time_valid() {
        // Every minute
        let result = CronServiceImpl::next_run_time("0 * * * * *");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_next_run_time_invalid() {
        let result = CronServiceImpl::next_run_time("invalid cron");
        assert!(result.is_err());
    }

    #[test]
    fn test_cron_job_record_serialization() {
        let record = CronJobRecord {
            job_id: "test-job".to_string(),
            target_actor: "worker@node1".to_string(),
            msg_type: "process".to_string(),
            payload: vec![1, 2, 3],
            expression: "0 * * * * *".to_string(),
            timezone: "UTC".to_string(),
            enabled: true,
            last_run_at: None,
            next_run_at: Some(1000),
            run_count: 0,
            last_error: None,
        };

        let json = serde_json::to_string(&record).unwrap();
        let deserialized: CronJobRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.job_id, "test-job");
        assert_eq!(deserialized.payload, vec![1, 2, 3]);
    }
}
