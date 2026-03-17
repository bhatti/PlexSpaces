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

//! # System Service Implementation
//!
//! ## Purpose
//! Implements the SystemService gRPC service for system administration,
//! health checks, and observability.
//!
//! ## Architecture Context
//! This service provides:
//! - System information
//! - Health checks (detailed and probe endpoints)
//! - Configuration management
//! - Metrics and logs
//!
//! ## Design Notes
//! - All RPCs are gRPC-based with HTTP access via gRPC-Gateway
//! - Health probe endpoints return HTTP status codes for Kubernetes
//! - Integrates with PlexSpacesHealthReporter for dependency checks

// Use HealthReporter trait from core - no dependency on node
use plexspaces_core::HealthReporter;
use plexspaces_proto::system::v1::system_service_server::SystemService;
use plexspaces_proto::system::v1::*;
use regex::Regex;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// System Service implementation
pub struct SystemServiceImpl {
    /// Health reporter for dependency checks
    health_reporter: Arc<dyn HealthReporter>,
}

impl SystemServiceImpl {
    /// Create new SystemService implementation
    pub fn new(health_reporter: Arc<dyn HealthReporter>) -> Self {
        Self { health_reporter }
    }
}

#[async_trait::async_trait]
impl SystemService for SystemServiceImpl {
    async fn get_system_info(
        &self,
        request: Request<GetSystemInfoRequest>,
    ) -> Result<Response<GetSystemInfoResponse>, Status> {
        let req = request.into_inner();
        let include_details = req.include_details;

        // Record metrics
        metrics::counter!("plexspaces_node_system_info_requests_total",
            "include_details" => include_details.to_string()
        )
        .increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "System info requested (include_details: {})",
                include_details
            );
        }

        // Get node stats if available
        // TODO: Access node stats through ServiceLocator instead of direct Node reference
        let _stats: Option<()> = None; // Node stats not available without Node dependency

        // Get system info using sysinfo
        use sysinfo::System;
        let mut system = System::new();
        system.refresh_all();

        let total_memory = system.total_memory();
        let _used_memory = system.used_memory();
        let _available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;

        // Get CPU usage (average across all CPUs)
        let _cpu_usage = if include_details {
            system
                .cpus()
                .iter()
                .map(|cpu| cpu.cpu_usage() as f64)
                .sum::<f64>()
                / cpu_count as f64
        } else {
            0.0
        };

        // Build SystemInfo response
        use plexspaces_proto::system::v1::SystemInfo;

        // Get build-time information from environment variables (set by build.rs or CI/CD)
        let build_date = option_env!("PLEXSPACES_BUILD_DATE")
            .or_else(|| option_env!("VERGEN_BUILD_TIMESTAMP"))
            .unwrap_or("unknown")
            .to_string();
        let git_commit = option_env!("PLEXSPACES_GIT_COMMIT")
            .or_else(|| option_env!("VERGEN_GIT_SHA"))
            .or_else(|| option_env!("GIT_COMMIT"))
            .unwrap_or("unknown")
            .to_string();

        // Get enabled features (compile-time feature flags)
        let features = {
            let mut enabled_features = Vec::new();
            #[cfg(feature = "sqlite-backend")]
            enabled_features.push("sqlite-backend".to_string());
            #[cfg(feature = "postgres-backend")]
            enabled_features.push("postgres-backend".to_string());
            #[cfg(feature = "redis-backend")]
            enabled_features.push("redis-backend".to_string());
            #[cfg(feature = "kafka-backend")]
            enabled_features.push("kafka-backend".to_string());
            enabled_features
        };

        let system_info = SystemInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_date,
            git_commit,
            uptime: Some(plexspaces_proto::prost_types::Duration {
                seconds: sysinfo::System::uptime() as i64,
                nanos: 0,
            }),
            hostname: std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
            platform: sysinfo::System::name().unwrap_or_default(),
            architecture: std::env::consts::ARCH.to_string(),
            cpu_cores: cpu_count as i32,
            total_memory_mb: (total_memory / (1024 * 1024)) as u64,
            features,
        };

        Ok(Response::new(GetSystemInfoResponse {
            system_info: Some(system_info),
        }))
    }

    async fn get_health(
        &self,
        request: Request<GetHealthRequest>,
    ) -> Result<Response<GetHealthResponse>, Status> {
        let req = request.into_inner();
        let components = req.components;

        // Record metrics
        metrics::counter!("plexspaces_node_health_requests_total",
            "components_count" => components.len().to_string()
        )
        .increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Health check requested (components: {:?})", components);
        }

        // Use detailed health check (same as get_detailed_health but simpler response)
        let health = self
            .health_reporter
            .get_detailed_health(true) // Include non-critical for comprehensive health
            .await;

        // Convert to GetHealthResponse (use component_checks, not dependency_checks)
        Ok(Response::new(GetHealthResponse {
            overall_status: health.overall_status,
            checks: health.component_checks,
        }))
    }

    async fn get_detailed_health(
        &self,
        request: Request<GetDetailedHealthRequest>,
    ) -> Result<Response<GetDetailedHealthResponse>, Status> {
        let req = request.into_inner();
        let include_non_critical = req.include_non_critical;

        let health = self
            .health_reporter
            .get_detailed_health(include_non_critical)
            .await;

        Ok(Response::new(GetDetailedHealthResponse {
            health: Some(health),
        }))
    }

    async fn liveness_probe(
        &self,
        _request: Request<LivenessProbeRequest>,
    ) -> Result<Response<LivenessProbeResponse>, Status> {
        let is_alive = self.health_reporter.is_alive().await;
        let http_status_code = if is_alive { 200 } else { 503 };

        Ok(Response::new(LivenessProbeResponse {
            is_alive,
            http_status_code,
        }))
    }

    async fn readiness_probe(
        &self,
        _request: Request<ReadinessProbeRequest>,
    ) -> Result<Response<ReadinessProbeResponse>, Status> {
        let (is_ready, not_ready_reason) = self.health_reporter.check_readiness().await;
        let http_status_code = if is_ready { 200 } else { 503 };

        Ok(Response::new(ReadinessProbeResponse {
            is_ready,
            http_status_code,
            not_ready_reason: not_ready_reason.unwrap_or_default(),
        }))
    }

    async fn startup_probe(
        &self,
        _request: Request<StartupProbeRequest>,
    ) -> Result<Response<StartupProbeResponse>, Status> {
        let (startup_complete, not_complete_reason) = self.health_reporter.check_startup().await;
        let http_status_code = if startup_complete { 200 } else { 503 };

        Ok(Response::new(StartupProbeResponse {
            startup_complete,
            http_status_code,
            not_complete_reason: not_complete_reason.unwrap_or_default(),
        }))
    }

    async fn get_node_readiness(
        &self,
        _request: Request<GetNodeReadinessRequest>,
    ) -> Result<Response<GetNodeReadinessResponse>, Status> {
        let readiness = self.health_reporter.get_readiness().await;

        Ok(Response::new(GetNodeReadinessResponse {
            readiness: Some(readiness),
        }))
    }

    async fn mark_startup_complete(
        &self,
        request: Request<MarkStartupCompleteRequest>,
    ) -> Result<Response<MarkStartupCompleteResponse>, Status> {
        let req = request.into_inner();
        let duration = self
            .health_reporter
            .mark_startup_complete(Some(req.message))
            .await;

        let state = self.health_reporter.get_state().await;
        let status = if state.startup_complete {
            ServingStatus::ServingStatusServing
        } else {
            ServingStatus::ServingStatusNotServing
        };

        Ok(Response::new(MarkStartupCompleteResponse {
            status: status as i32,
            startup_duration: Some(duration),
        }))
    }

    async fn begin_shutdown(
        &self,
        request: Request<BeginShutdownRequest>,
    ) -> Result<Response<BeginShutdownResponse>, Status> {
        let req = request.into_inner();
        let drain_timeout = req.drain_timeout;

        let (requests_drained, drain_duration, drain_completed) =
            self.health_reporter.begin_shutdown(drain_timeout).await;

        Ok(Response::new(BeginShutdownResponse {
            requests_drained,
            drain_duration: Some(drain_duration),
            drain_completed,
        }))
    }

    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let _req = request.into_inner();

        // Record metrics
        metrics::counter!("plexspaces_node_metrics_requests_total").increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Metrics requested");
        }

        // Get node stats if available
        // TODO: Access node stats through ServiceLocator instead of direct Node reference
        let _stats: Option<()> = None; // Node stats not available without Node dependency

        // Get system info using sysinfo
        use sysinfo::System;
        let mut system = System::new();
        system.refresh_all();

        let total_memory = system.total_memory();
        let used_memory = system.used_memory();
        let available_memory = system.available_memory();
        let cpu_count = system.cpus().len() as u32;
        let cpu_usage = if cpu_count > 0 {
            system
                .cpus()
                .iter()
                .map(|cpu| cpu.cpu_usage() as f64)
                .sum::<f64>()
                / cpu_count as f64
        } else {
            0.0
        };

        // Build SystemMetrics from node stats and system info
        use plexspaces_proto::metrics::v1::{
            ComponentMetrics, CpuMetrics, MemoryMetrics, SystemMetrics,
        };
        use plexspaces_proto::prost_types;
        use std::collections::HashMap;

        // Get load averages (sysinfo supports this on Unix systems)
        let (load_avg_1m, load_avg_5m, load_avg_15m) = if cfg!(unix) {
            // sysinfo doesn't have load_average() method, so we'll use 0.0 for now
            // TODO: Use system load average if sysinfo adds support
            (0.0, 0.0, 0.0)
        } else {
            (0.0, 0.0, 0.0) // Windows doesn't support load averages
        };

        // Get active processes count
        system.refresh_processes();
        let active_processes = system.processes().len() as i32;

        // Build CPU metrics
        let cpu_metrics = CpuMetrics {
            usage_percent: cpu_usage,
            load_average_1m: load_avg_1m,
            load_average_5m: load_avg_5m,
            load_average_15m: load_avg_15m,
            active_processes,
        };

        // Get cached memory (available on Unix systems)
        let cached_mb = if cfg!(unix) {
            // On Unix, cached memory is part of available memory
            // For simplicity, we'll use a portion of available memory as cached
            (available_memory / (1024 * 1024)) / 2 // Approximate cached as half of available
        } else {
            0 // Windows doesn't expose cached memory separately
        };

        // Build memory metrics (convert bytes to MB)
        let memory_metrics = MemoryMetrics {
            total_mb: total_memory / (1024 * 1024),
            used_mb: used_memory / (1024 * 1024),
            free_mb: available_memory / (1024 * 1024),
            cached_mb,
            usage_percent: if total_memory > 0 {
                (used_memory as f64 / total_memory as f64) * 100.0
            } else {
                0.0
            },
        };

        // TupleSpace was removed per refactoring - set to 0
        let tuplespace_size = 0u64;

        // Get VM count (check if node has VM registry)
        // TODO: Add VM registry accessor method when VM tracking is implemented
        // For now, return 0 as VM tracking is not yet integrated
        let active_vms = 0;

        // Build component metrics
        let mut active_actors_by_type = HashMap::new();
        active_actors_by_type.insert("standard".to_string(), 0); // TODO: Get from ServiceLocator

        // Get subscription and transaction counts (TODO: Track these when subscriptions/transactions are implemented)
        let active_subscriptions = 0; // TODO: Track subscriptions when subscription system is implemented
        let active_transactions = 0; // TODO: Track transactions when transaction system is implemented

        let component_metrics = ComponentMetrics {
            active_actors_by_type,
            active_vms,
            tuplespace_size,
            active_subscriptions,
            active_transactions,
        };

        let system_metrics = SystemMetrics {
            timestamp: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            cpu: Some(cpu_metrics),
            memory: Some(memory_metrics),
            disk: {
                // Get disk metrics using sysinfo
                use sysinfo::Disks;
                let disks = Disks::new_with_refreshed_list();
                let total_disk_bytes: u64 = disks.iter().map(|d| d.total_space()).sum();
                let available_disk_bytes: u64 = disks.iter().map(|d| d.available_space()).sum();
                let used_disk_bytes = total_disk_bytes.saturating_sub(available_disk_bytes);

                if total_disk_bytes > 0 {
                    Some(plexspaces_proto::metrics::v1::DiskMetrics {
                        total_gb: total_disk_bytes / (1024 * 1024 * 1024),
                        used_gb: used_disk_bytes / (1024 * 1024 * 1024),
                        free_gb: available_disk_bytes / (1024 * 1024 * 1024),
                        usage_percent: if total_disk_bytes > 0 {
                            (used_disk_bytes as f64 / total_disk_bytes as f64) * 100.0
                        } else {
                            0.0
                        },
                        read_ops_per_sec: 0,  // sysinfo doesn't provide this
                        write_ops_per_sec: 0, // sysinfo doesn't provide this
                    })
                } else {
                    None
                }
            },
            network: {
                // Get network metrics using sysinfo
                use sysinfo::Networks;
                let mut networks = Networks::new_with_refreshed_list();
                networks.refresh();

                // Note: sysinfo doesn't provide packet counts directly
                // We calculate bytes for potential future use, but don't use them in return value
                // (per-second metrics would need delta calculation from previous values)
                let mut _total_bytes_received = 0u64;
                let mut _total_bytes_sent = 0u64;
                let _total_packets_received = 0u64;
                let _total_packets_sent = 0u64;

                for (_interface_name, network) in networks.iter() {
                    _total_bytes_received += network.received();
                    _total_bytes_sent += network.transmitted();
                    // sysinfo doesn't provide packet counts directly
                }

                // Note: sysinfo provides cumulative stats, not per-second
                // For per-second, we'd need to track previous values and calculate delta
                // For now, return 0 for per-second metrics
                Some(plexspaces_proto::metrics::v1::NetworkMetrics {
                    bytes_received_per_sec: 0,   // Would need delta calculation
                    bytes_sent_per_sec: 0,       // Would need delta calculation
                    packets_received_per_sec: 0, // sysinfo doesn't provide
                    packets_sent_per_sec: 0,     // sysinfo doesn't provide
                    active_connections: 0,       // TODO: Get from ServiceLocator
                })
            },
            components: Some(component_metrics),
        };

        Ok(Response::new(GetMetricsResponse {
            metrics: vec![system_metrics],
        }))
    }

    async fn get_config(
        &self,
        request: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let req = request.into_inner();
        let key_pattern = req.key_pattern;
        let _include_secrets = req.include_secrets;

        // Record metrics
        metrics::counter!("plexspaces_node_config_requests_total",
            "key_pattern" => key_pattern.clone()
        )
        .increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Config retrieval requested (pattern: {})", key_pattern);
        }

        // Get node config if available via ServiceLocator::get_node_config()
        let mut settings: Vec<plexspaces_proto::system::v1::ConfigSetting> = vec![];

        // Filter by pattern if provided
        if !key_pattern.is_empty() {
            // Simple glob matching (supports * wildcard)
            let pattern = key_pattern.replace("*", ".*");
            let regex = Regex::new(&format!("^{}$", pattern))
                .map_err(|e| Status::invalid_argument(format!("Invalid key pattern: {}", e)))?;

            settings.retain(|s: &_| regex.is_match(&s.key));
        }

        Ok(Response::new(GetConfigResponse { settings }))
    }

    async fn set_config(
        &self,
        request: Request<SetConfigRequest>,
    ) -> Result<Response<SetConfigResponse>, Status> {
        let req = request.into_inner();

        // Record metrics
        metrics::counter!("plexspaces_node_config_updates_total",
            "settings_count" => req.settings.len().to_string()
        )
        .increment(1);
        tracing::info!("Config update requested ({} settings)", req.settings.len());

        // For now, config updates are read-only (NodeConfig is immutable after creation)
        // In the future, we could support dynamic config updates
        // For now, return error indicating config is read-only
        Err(Status::failed_precondition(
            "Configuration is read-only. Node configuration must be set at startup.",
        ))
    }

    async fn get_logs(
        &self,
        request: Request<GetLogsRequest>,
    ) -> Result<Response<GetLogsResponse>, Status> {
        let _req = request.into_inner();

        // Record metrics
        metrics::counter!("plexspaces_node_logs_requests_total").increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Log retrieval requested");
        }

        // For now, log retrieval is not implemented
        // In the future, we could integrate with a log aggregation system
        // or return recent logs from memory buffer
        Ok(Response::new(GetLogsResponse {
            entries: vec![], // Empty for now
            page_response: None,
        }))
    }

    async fn create_backup(
        &self,
        request: Request<CreateBackupRequest>,
    ) -> Result<Response<CreateBackupResponse>, Status> {
        let req = request.into_inner();

        // Record metrics
        use plexspaces_proto::system::v1::BackupInfo;
        metrics::counter!("plexspaces_node_backup_requests_total",
            "backup_type" => req.r#type.to_string()
        )
        .increment(1);
        tracing::info!("Backup creation requested (type: {})", req.r#type);

        // For now, backup creation is not implemented
        // In the future, we could backup actor state, journal entries, etc.
        let backup_id = format!("backup-{}", ulid::Ulid::new());

        let backup = BackupInfo {
            id: backup_id,
            r#type: req.r#type,
            status: 0, // BackupStatus::BackupStatusUnspecified
            created_at: Some(plexspaces_proto::prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            completed_at: None,
            size_bytes: {
                // Estimate backup size based on components
                let estimated_size = if req.components.is_empty() {
                    // Full backup - estimate (use ServiceLocator::get_node_metrics_accessor() if needed)
                    0
                } else {
                    // Partial backup - estimate based on component count
                    req.components.len() as u64 * 1024 * 1024 // 1MB per component estimate
                };
                estimated_size
            },
            location: req.destination.clone(),
            checksum: String::new(),
            components: req.components.clone(),
            error: String::new(),
        };

        Ok(Response::new(CreateBackupResponse {
            backup: Some(backup),
        }))
    }

    async fn list_backups(
        &self,
        _request: Request<ListBackupsRequest>,
    ) -> Result<Response<ListBackupsResponse>, Status> {
        // Record metrics
        metrics::counter!("plexspaces_node_backup_list_requests_total").increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Backup listing requested");
        }

        // For now, return empty list (no backups implemented yet)
        use plexspaces_proto::common::v1::PageResponse;
        Ok(Response::new(ListBackupsResponse {
            backups: vec![],
            page_response: Some(PageResponse {
                offset: 0,
                limit: 0,
                has_next: false,
                total_size: 0,
            }),
        }))
    }

    async fn get_shutdown_status(
        &self,
        _request: Request<GetShutdownStatusRequest>,
    ) -> Result<Response<GetShutdownStatusResponse>, Status> {
        // Record metrics
        metrics::counter!("plexspaces_node_shutdown_status_requests_total").increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Shutdown status requested");
        }

        // Get shutdown state from health reporter
        let state = self.health_reporter.get_state().await;

        use plexspaces_proto::system::v1::{ShutdownPhase, ShutdownSignal, ShutdownStatus};

        use std::time::SystemTime;

        let phase = if state.shutdown_in_progress {
            ShutdownPhase::ShutdownPhaseDraining as i32
        } else {
            ShutdownPhase::ShutdownPhaseRunning as i32
        };

        // Calculate elapsed time if shutdown has started
        let elapsed = if let Some(started_at) = &state.state_entered_at {
            if state.shutdown_in_progress {
                // Calculate elapsed from shutdown start time
                let start_time = SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs(started_at.seconds as u64)
                    + std::time::Duration::from_nanos(started_at.nanos as u64);
                let now = SystemTime::now();
                if let Ok(duration) = now.duration_since(start_time) {
                    Some(prost_types::Duration {
                        seconds: duration.as_secs() as i64,
                        nanos: duration.subsec_nanos() as i32,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let status = ShutdownStatus {
            phase,
            signal: ShutdownSignal::ShutdownSignalUnspecified as i32,
            shutdown_started_at: state.state_entered_at.clone(),
            elapsed,
            health_not_serving_status: None,
            draining_status: None,
            stopping_actors_status: None,
            closing_connections_status: None,
            shutting_down_tuplespace_status: None,
            final_cleanup_status: None,
            completed: !state.shutdown_in_progress, // Completed if not in progress
            error: String::new(),
        };

        Ok(Response::new(GetShutdownStatusResponse {
            status: Some(status),
        }))
    }

    async fn restore_backup(
        &self,
        request: Request<RestoreBackupRequest>,
    ) -> Result<Response<RestoreBackupResponse>, Status> {
        let req = request.into_inner();

        // Record metrics
        metrics::counter!("plexspaces_node_backup_restore_requests_total",
            "backup_id" => req.backup_id.clone()
        )
        .increment(1);
        tracing::info!(
            "Backup restoration requested (backup_id: {})",
            req.backup_id
        );

        // For now, backup restoration is not implemented
        // In the future, we could restore actor state, journal entries, etc.
        Err(Status::unimplemented(
            "Backup restoration not yet implemented",
        ))
    }

    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        let req = request.into_inner();

        // Record metrics
        metrics::counter!("plexspaces_node_shutdown_requests_total",
            "graceful" => req.graceful.to_string()
        )
        .increment(1);
        tracing::info!("Shutdown requested (graceful: {})", req.graceful);

        // Trigger shutdown via health reporter
        let drain_timeout = req.timeout;

        let (requests_drained, drain_duration, drain_completed) =
            self.health_reporter.begin_shutdown(drain_timeout).await;
        // Node shutdown is handled via ServiceLocator::request_shutdown()

        Ok(Response::new(ShutdownResponse {
            success: drain_completed,
            message: if drain_completed {
                format!(
                    "Shutdown completed successfully. Drained {} requests in {:?}",
                    requests_drained, drain_duration
                )
            } else {
                format!(
                    "Shutdown initiated. Drained {} requests (timeout reached)",
                    requests_drained
                )
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::PlexSpacesHealthReporter;
    use plexspaces_proto::system::v1::{DependencyRegistrationConfig, HealthProbeConfig};

    #[tokio::test]
    async fn test_liveness_probe() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
            }),
            ..Default::default()
        };
        // Create HealthService without ServiceLocator for standalone tests
        let (reporter, _) = PlexSpacesHealthReporter::with_config_and_service_locator(config, None);
        let reporter = Arc::new(reporter);
        let service = SystemServiceImpl::new(reporter.clone());

        let request = Request::new(LivenessProbeRequest {});
        let response = service.liveness_probe(request).await.unwrap();
        let resp = response.into_inner();

        assert!(resp.is_alive);
        assert_eq!(resp.http_status_code, 200);
    }

    #[tokio::test]
    async fn test_readiness_probe_not_ready() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
            }),
            ..Default::default()
        };
        // Create HealthService without ServiceLocator for standalone tests
        let (reporter, _) = PlexSpacesHealthReporter::with_config_and_service_locator(config, None);
        let reporter = Arc::new(reporter);
        let service = SystemServiceImpl::new(reporter.clone());

        // Initially not ready (startup not complete)
        let request = Request::new(ReadinessProbeRequest {});
        let response = service.readiness_probe(request).await.unwrap();
        let resp = response.into_inner();

        assert!(!resp.is_ready);
        assert_eq!(resp.http_status_code, 503);
    }

    #[tokio::test]
    async fn test_readiness_probe_ready() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
            }),
            ..Default::default()
        };
        // Create HealthService without ServiceLocator for standalone tests
        let (reporter, _) = PlexSpacesHealthReporter::with_config_and_service_locator(config, None);
        let reporter = Arc::new(reporter);

        // Mark startup complete
        reporter.mark_startup_complete(None).await;

        let service = SystemServiceImpl::new(reporter.clone());

        let request = Request::new(ReadinessProbeRequest {});
        let response = service.readiness_probe(request).await.unwrap();
        let resp = response.into_inner();

        assert!(resp.is_ready);
        assert_eq!(resp.http_status_code, 200);
    }

    #[tokio::test]
    async fn test_startup_probe() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
            }),
            ..Default::default()
        };
        // Create HealthService without ServiceLocator for standalone tests
        let (reporter, _) = PlexSpacesHealthReporter::with_config_and_service_locator(config, None);
        let reporter = Arc::new(reporter);
        let service = SystemServiceImpl::new(reporter.clone());

        // Initially not complete
        let request = Request::new(StartupProbeRequest {});
        let response = service.startup_probe(request).await.unwrap();
        let resp = response.into_inner();

        assert!(!resp.startup_complete);
        assert_eq!(resp.http_status_code, 503);

        // Mark complete
        reporter.mark_startup_complete(None).await;

        let request = Request::new(StartupProbeRequest {});
        let response = service.startup_probe(request).await.unwrap();
        let resp = response.into_inner();

        assert!(resp.startup_complete);
        assert_eq!(resp.http_status_code, 200);
    }

    #[tokio::test]
    async fn test_get_detailed_health() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
            }),
            ..Default::default()
        };
        // Create HealthService without ServiceLocator for standalone tests
        let (reporter, _) = PlexSpacesHealthReporter::with_config_and_service_locator(config, None);
        let reporter = Arc::new(reporter);
        let service = SystemServiceImpl::new(reporter.clone());

        let request = Request::new(GetDetailedHealthRequest {
            include_non_critical: true,
        });
        let response = service.get_detailed_health(request).await.unwrap();
        let resp = response.into_inner();

        assert!(resp.health.is_some());
        let health = resp.health.unwrap();
        assert!(health.dependency_checks.is_empty()); // No checkers registered yet
    }
}
