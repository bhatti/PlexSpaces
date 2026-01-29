// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Message types for config service and data pipeline

use serde::{Deserialize, Serialize};

// ============================================================================
// Config Service Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConfig {
    pub version: u64,
    pub config_json: String,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigServiceMessage {
    RegisterWorker {
        worker_id: String,
        node_id: String,
        current_version: u64,
    },
    RegisterWorkerResponse {
        latest_version: u64,
        config: Option<WorkerConfig>,
    },
    GetConfig {
        worker_id: String,
        version: u64, // 0 for latest
    },
    GetConfigResponse {
        config: WorkerConfig,
    },
    NotifyConfigChange {
        version: u64,
        config: WorkerConfig,
    },
    NotifyConfigChangeResponse {
        workers_notified: u64,
    },
}

// ============================================================================
// Data Pipeline Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    pub fields: serde_json::Value,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEntry {
    pub id: String,
    pub timestamp: u64,
    pub name: String,
    pub value: f64,
    pub metric_type: String,
    pub tags: std::collections::HashMap<String, String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PipelineEvent {
    Log { data: LogEntry },
    Metric { data: MetricEntry },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PipelineMessage {
    Ingest {
        events: Vec<PipelineEvent>,
    },
    Process {
        events: Vec<PipelineEvent>,
    },
    Processed {
        events: Vec<PipelineEvent>,
        stage_name: String,
    },
    SendToDestination {
        destination_type: String,
        destination_config: String,
        events: Vec<PipelineEvent>,
    },
    SendToDestinationResponse {
        events_sent: u64,
        events_failed: u64,
        errors: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    pub pipeline_id: String,
    pub filter_function: String,
    pub enrichment_function: String,
    pub transform_function: String,
    pub destinations: Vec<DestinationConfig>,
    pub backpressure_threshold: u64,
    pub durability_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestinationConfig {
    pub destination_type: String,
    pub config_json: String,
    pub retry_config: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_backoff_sec: u32,
    pub max_backoff_sec: u32,
    pub backoff_multiplier: f64,
}















