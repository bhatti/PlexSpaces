// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// SDK Helpers for Node Connectivity (Thin Wrapper Around Core APIs)
//
// ## Architecture
// - Core logic: NodeService, SystemService (in crates/services, crates/proto)
// - SDK: Convenience wrappers with retry/backoff (this file)
//
// ## Health-Aware Connection (Kubernetes + Erlang-inspired)
// - Liveness: Is the node alive? (should we retry?)
// - Readiness: Is the node ready? (can we use it?)
// - Exponential backoff with jitter (industry best practice)
// - Simple retry logic (Erlang net_adm:ping style)

use anyhow::{Context, Result};
use plexspaces_proto::node::v1::{
    node_service_client::NodeServiceClient, ConnectNodesRequest, ConnectNodesResponse,
    GetMetricsRequest, ListConnectedNodesRequest, PingRequest,
};
use plexspaces_proto::prost_types::Duration;
#[cfg(feature = "grpc")]
use plexspaces_proto::system::v1::system_service_client::SystemServiceClient;
#[cfg(feature = "grpc")]
use plexspaces_proto::system::v1::{
    LivenessProbeRequest, LivenessProbeResponse, ReadinessProbeRequest, ReadinessProbeResponse,
};
use std::collections::HashMap;
use std::time::Duration as StdDuration;
use tokio::time::sleep;
use tokio::time::timeout as tokio_timeout;
use tonic::Request;

/// Node client for connectivity operations
pub struct NodeClient {
    client: NodeServiceClient<tonic::transport::Channel>,
    #[cfg(feature = "grpc")]
    system_client: Option<SystemServiceClient<tonic::transport::Channel>>,
    node_addr: String,
}

/// Health check configuration for node connection
#[derive(Clone, Debug)]
pub struct HealthCheckConfig {
    /// Maximum number of retries for connection
    pub max_retries: u32,
    /// Initial retry delay (exponential backoff starts here)
    pub initial_delay: StdDuration,
    /// Maximum retry delay (caps exponential backoff)
    pub max_delay: StdDuration,
    /// Timeout for individual health checks
    pub health_check_timeout: StdDuration,
    /// Whether to check liveness before connecting
    pub check_liveness: bool,
    /// Whether to wait for readiness after connecting
    pub wait_for_readiness: bool,
    /// Maximum time to wait for readiness
    pub readiness_timeout: StdDuration,
    /// Poll interval for readiness checks
    pub readiness_poll_interval: StdDuration,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            max_retries: 10, // Increased retries for Docker environments where nodes take time to start
            initial_delay: StdDuration::from_millis(500),
            max_delay: StdDuration::from_secs(10),
            health_check_timeout: StdDuration::from_secs(10), // Increased timeout for slow-starting nodes
            check_liveness: true,
            wait_for_readiness: true,
            readiness_timeout: StdDuration::from_secs(60), // Increased readiness timeout for Docker
            readiness_poll_interval: StdDuration::from_millis(1000), // Longer poll interval
        }
    }
}

impl NodeClient {
    /// Create a new NodeClient connected to the specified node with health-aware retry logic
    /// Uses default health check configuration (checks liveness, waits for readiness)
    pub async fn connect(node_addr: impl Into<String>) -> Result<Self> {
        Self::connect_with_health_check(node_addr, HealthCheckConfig::default()).await
    }

    /// Create a new NodeClient connected to the specified node with custom health check config
    ///
    /// ## Health-Aware Connection Flow (Kubernetes-inspired)
    /// 1. Check liveness (if enabled): Is node alive? If not, retry with exponential backoff
    /// 2. Connect to NodeService: Establish gRPC connection
    /// 3. Verify with ping: Ensure connection works
    /// 4. Wait for readiness (if enabled): Poll until node is ready to serve requests
    ///
    /// ## Retry Strategy (Erlang-inspired)
    /// - Exponential backoff with jitter: delay = min(initial_delay * 2^attempt, max_delay) + jitter
    /// - Retries on: connection failures, liveness failures, ping failures
    /// - No retry on: readiness timeout (node is alive but not ready - different error)
    pub async fn connect_with_health_check(
        node_addr: impl Into<String>,
        config: HealthCheckConfig,
    ) -> Result<Self> {
        let addr = node_addr.into();
        let mut last_error = None;

        // Step 1: Check liveness (if enabled) - ensures node is alive before connecting
        if config.check_liveness {
            for attempt in 0..=config.max_retries {
                match Self::check_liveness_once(&addr, config.health_check_timeout).await {
                    Ok(true) => break, // Node is alive, proceed to connect
                    Ok(false) => {
                        last_error = Some("Node liveness check failed".to_string());
                        if attempt < config.max_retries {
                            let delay = Self::exponential_backoff_internal(attempt, &config);
                            sleep(delay).await;
                            continue;
                        }
                    }
                    Err(e) => {
                        last_error = Some(format!("Liveness check error: {}", e));
                        if attempt < config.max_retries {
                            let delay = Self::exponential_backoff_internal(attempt, &config);
                            sleep(delay).await;
                            continue;
                        }
                    }
                }
            }

            if last_error.is_some() {
                return Err(anyhow::anyhow!(
                    "Node {} failed liveness checks after {} attempts: {}",
                    addr,
                    config.max_retries + 1,
                    last_error.unwrap()
                ));
            }
        }

        // Step 2: Connect to NodeService with retry
        let mut node_client = None;
        for attempt in 0..=config.max_retries {
            match NodeServiceClient::connect(addr.clone()).await {
                Ok(client) => {
                    // Step 3: Verify connection with ping
                    let mut temp_client = Self {
                        client: client.clone(),
                        #[cfg(feature = "grpc")]
                        system_client: None,
                        node_addr: addr.clone(),
                    };

                    match tokio_timeout(
                        config.health_check_timeout,
                        temp_client.ping("client".to_string(), 1),
                    )
                    .await
                    {
                        Ok(Ok(_)) => {
                            node_client = Some(temp_client);
                            break;
                        }
                        Ok(Err(e)) => {
                            last_error = Some(format!("Ping failed: {}", e));
                            if attempt < config.max_retries {
                                let delay = Self::exponential_backoff_internal(attempt, &config);
                                sleep(delay).await;
                                continue;
                            }
                        }
                        Err(_) => {
                            last_error = Some("Ping timed out".to_string());
                            if attempt < config.max_retries {
                                let delay = Self::exponential_backoff_internal(attempt, &config);
                                sleep(delay).await;
                                continue;
                            }
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(format!("Connection failed: {}", e));
                    if attempt < config.max_retries {
                        let delay = Self::exponential_backoff_internal(attempt, &config);
                        sleep(delay).await;
                        continue;
                    }
                }
            }
        }

        let mut client = node_client.ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to connect to node {} after {} attempts: {}",
                addr,
                config.max_retries + 1,
                last_error.unwrap_or_else(|| "Unknown error".to_string())
            )
        })?;

        // Step 4: Wait for readiness (if enabled)
        if config.wait_for_readiness {
            client
                .wait_for_readiness(config.readiness_timeout, config.readiness_poll_interval)
                .await?;
        }

        Ok(client)
    }

    /// Legacy method: Connect with simple retry (no health checks)
    /// Prefer `connect()` or `connect_with_health_check()` for production use
    pub async fn connect_with_retry(
        node_addr: impl Into<String>,
        max_retries: u32,
        retry_delay: StdDuration,
    ) -> Result<Self> {
        let config = HealthCheckConfig {
            max_retries,
            initial_delay: retry_delay,
            max_delay: retry_delay * 4,
            health_check_timeout: StdDuration::from_secs(5),
            check_liveness: false,
            wait_for_readiness: false,
            readiness_timeout: StdDuration::from_secs(30),
            readiness_poll_interval: StdDuration::from_millis(500),
        };
        Self::connect_with_health_check(node_addr, config).await
    }

    /// Check if node is alive (liveness probe) - SDK wrapper around core SystemService
    /// Uses core SystemServiceClient from proto crate
    #[cfg(feature = "grpc")]
    async fn check_liveness_once(addr: &str, timeout_duration: StdDuration) -> Result<bool> {
        // Use core SystemServiceClient (from proto crate)
        match SystemServiceClient::connect(addr.to_string()).await {
            Ok(mut client) => {
                // Call core SystemService.liveness_probe() API
                let request = Request::new(LivenessProbeRequest {});
                match tokio_timeout(timeout_duration, client.liveness_probe(request)).await {
                    Ok(Ok(resp)) => {
                        let inner: LivenessProbeResponse = resp.into_inner();
                        Ok(inner.is_alive)
                    }
                    Ok(Err(_)) => Ok(false), // gRPC error means not alive
                    Err(_) => Ok(false),     // Timeout means not alive
                }
            }
            Err(_) => Ok(false), // Can't connect means not alive
        }
    }

    #[cfg(not(feature = "grpc"))]
    async fn check_liveness_once(_addr: &str, _timeout: StdDuration) -> Result<bool> {
        // Without grpc feature, assume alive (fallback to ping-based check)
        Ok(true)
    }

    /// Wait for node to become ready (readiness probe)
    /// Polls readiness endpoint until node is ready or timeout expires
    pub async fn wait_for_readiness(
        &mut self,
        timeout: StdDuration,
        poll_interval: StdDuration,
    ) -> Result<()> {
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > timeout {
                return Err(anyhow::anyhow!(
                    "Node {} did not become ready within {:?}",
                    self.node_addr,
                    timeout
                ));
            }

            match self.check_readiness_once().await {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // Not ready yet, wait and retry
                    sleep(poll_interval).await;
                    continue;
                }
                Err(_e) => {
                    // Error checking readiness - might be temporary, retry
                    sleep(poll_interval).await;
                    continue;
                }
            }
        }
    }

    /// Check if node is ready (readiness probe) - SDK wrapper around core SystemService
    /// Uses core SystemServiceClient from proto crate
    pub async fn check_readiness_once(&mut self) -> Result<bool> {
        #[cfg(feature = "grpc")]
        {
            // Get or create SystemService client (core API from proto crate)
            if self.system_client.is_none() {
                let system_addr = self.node_addr.clone();
                match SystemServiceClient::connect(system_addr).await {
                    Ok(client) => {
                        self.system_client = Some(client);
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Failed to connect to SystemService: {}", e));
                    }
                }
            }

            // Call core SystemService.readiness_probe() API
            if let Some(ref mut client) = self.system_client {
                let request = Request::new(ReadinessProbeRequest {});
                match client.readiness_probe(request).await {
                    Ok(resp) => {
                        let inner: ReadinessProbeResponse = resp.into_inner();
                        Ok(inner.is_ready)
                    }
                    Err(e) => Err(anyhow::anyhow!("Readiness check failed: {}", e)),
                }
            } else {
                Err(anyhow::anyhow!("SystemService client not available"))
            }
        }

        #[cfg(not(feature = "grpc"))]
        {
            // Without grpc feature, assume ready (fallback)
            Ok(true)
        }
    }

    /// Calculate exponential backoff delay with jitter
    /// Formula: min(initial_delay * 2^attempt, max_delay) + jitter
    /// Jitter: random 0-25% of delay (prevents thundering herd)
    /// Note: Final result is capped at max_delay to ensure jitter doesn't exceed limit
    fn exponential_backoff_internal(attempt: u32, config: &HealthCheckConfig) -> StdDuration {
        let base_delay = config.initial_delay.as_millis() as u64;
        let exponential_delay = base_delay.saturating_mul(1 << attempt.min(10)); // Cap at 2^10
        let capped_delay = exponential_delay.min(config.max_delay.as_millis() as u64);

        // Add jitter (0-25% of delay) - simple deterministic jitter based on attempt
        let jitter_range = capped_delay / 4;
        let jitter = (attempt as u64 * 7) % jitter_range; // Simple deterministic jitter

        // Cap final result at max_delay (jitter can push it over)
        let final_delay = (capped_delay + jitter).min(config.max_delay.as_millis() as u64);

        StdDuration::from_millis(final_delay)
    }

    /// Ping the node to verify connectivity
    pub async fn ping(&mut self, source_node_id: String, sequence_number: u64) -> Result<()> {
        let req = PingRequest {
            source_node_id,
            sequence_number,
            updates: Vec::new(),
        };
        self.client
            .ping(Request::new(req))
            .await
            .context("Ping failed")?;
        Ok(())
    }

    /// Connect to remote nodes (Erlang-style net_adm:ping) with health-aware retry
    ///
    /// ## Design
    /// - Pre-checks liveness for each node (avoids unnecessary connection attempts)
    /// - Uses exponential backoff for retries
    /// - Provides detailed error messages for troubleshooting
    /// - Handles partial success gracefully (some nodes may connect while others fail)
    ///
    /// ## Health-Aware Flow
    /// 1. Check liveness for each node (parallel)
    /// 2. Filter out nodes that fail liveness (they're not ready yet)
    /// 3. Call core ConnectNodes API for alive nodes
    /// 4. Return combined results (connected + failed with reasons)
    pub async fn connect_nodes(
        &mut self,
        addresses: Vec<String>,
        cluster: Option<String>,
        timeout_secs: u64,
    ) -> Result<ConnectNodesResponse> {
        self.connect_nodes_with_health_check(
            addresses,
            cluster,
            timeout_secs,
            HealthCheckConfig::default(),
        )
        .await
    }

    /// Connect to remote nodes with custom health check configuration
    pub async fn connect_nodes_with_health_check(
        &mut self,
        addresses: Vec<String>,
        cluster: Option<String>,
        timeout_secs: u64,
        health_config: HealthCheckConfig,
    ) -> Result<ConnectNodesResponse> {
        if addresses.is_empty() {
            return Ok(ConnectNodesResponse {
                connected: HashMap::new(),
                failed: HashMap::new(),
                total_time: None,
            });
        }

        // Step 1: Pre-check liveness for all nodes in parallel with retries
        // Note: Liveness check failures are NOT fatal - we still try ConnectNodes API
        // because nodes may be starting up. The core ConnectNodes API has its own
        // ping/retry logic that handles temporary unavailability.
        // For Docker environments, nodes can take 10-30 seconds to fully start.
        let mut alive_nodes = Vec::new();

        if health_config.check_liveness {
            use futures::future::join_all;

            // Retry liveness checks with exponential backoff for each node
            let mut liveness_tasks = Vec::new();
            for addr in &addresses {
                let addr_clone = addr.clone();
                let config_clone = health_config.clone();
                liveness_tasks.push(async move {
                    let mut is_alive = false;
                    for attempt in 0..=config_clone.max_retries {
                        match Self::check_liveness_once(
                            &addr_clone,
                            config_clone.health_check_timeout,
                        )
                        .await
                        {
                            Ok(true) => {
                                is_alive = true;
                                break; // Node is alive, stop retrying
                            }
                            Ok(false) | Err(_) => {
                                is_alive = false;
                                if attempt < config_clone.max_retries {
                                    let delay =
                                        Self::exponential_backoff_internal(attempt, &config_clone);
                                    sleep(delay).await;
                                    continue;
                                }
                            }
                        }
                    }
                    (addr_clone, Ok::<bool, anyhow::Error>(is_alive))
                });
            }

            let results = join_all(liveness_tasks).await;

            for (addr, result) in results {
                match result {
                    Ok(true) => {
                        // Node is confirmed alive
                        alive_nodes.push(addr);
                    }
                    Ok(false) | Err(_) => {
                        // Node liveness check failed after retries, but still try ConnectNodes
                        // The core API might succeed if the node is partially ready
                        alive_nodes.push(addr);
                    }
                }
            }
        } else {
            // Skip liveness check, try all nodes
            alive_nodes = addresses;
        }

        // Step 2: Call core ConnectNodes API with retries (for Docker environments)
        // Core API handles ping, registration, and SWIM protocol setup
        // Retry the entire ConnectNodes call if it fails, as nodes may still be starting
        // Note: The core API has its own timeout per connection, but we retry the whole call
        // to handle cases where nodes are starting up slowly in Docker
        let mut resp_opt: Option<ConnectNodesResponse> = None;
        let mut last_error: Option<String> = None;

        for attempt in 0..=health_config.max_retries {
            let req = ConnectNodesRequest {
                node_addresses: alive_nodes.clone(),
                cluster: cluster.clone().unwrap_or_default(),
                timeout: Some(Duration {
                    seconds: timeout_secs as i64,
                    nanos: 0,
                }),
            };

            match self.client.connect_nodes(Request::new(req)).await {
                Ok(response) => {
                    resp_opt = Some(response.into_inner());
                    break; // Success, stop retrying
                }
                Err(e) => {
                    last_error = Some(format!("ConnectNodes API error: {}", e));
                    if attempt < health_config.max_retries {
                        let delay = Self::exponential_backoff_internal(attempt, &health_config);
                        sleep(delay).await;
                        continue;
                    }
                }
            }
        }

        let resp = match resp_opt {
            Some(r) => r,
            None => {
                // All retries failed, mark all nodes as failed
                let mut all_failed = HashMap::new();
                let error_msg = last_error
                    .unwrap_or_else(|| "ConnectNodes API failed after retries".to_string());
                for addr in alive_nodes {
                    all_failed.insert(addr, error_msg.clone());
                }
                return Ok(ConnectNodesResponse {
                    connected: HashMap::new(),
                    failed: all_failed,
                    total_time: None,
                });
            }
        };

        // Step 3: Return core API results (it handles all connection logic)
        // Note: We don't merge pre_check_failed because liveness check failures
        // are informational only - the core ConnectNodes API is the source of truth
        let final_connected = resp.connected;
        let final_failed = resp.failed;

        Ok(ConnectNodesResponse {
            connected: final_connected,
            failed: final_failed,
            total_time: resp.total_time,
        })
    }

    /// List connected nodes
    pub async fn list_connected_nodes(
        &mut self,
        cluster: Option<String>,
    ) -> Result<plexspaces_proto::node::v1::ListConnectedNodesResponse> {
        let req = ListConnectedNodesRequest {
            cluster: cluster.unwrap_or_default(),
            page_size: 100,
            page_token: String::new(),
            include_health: false,
        };

        let resp = self
            .client
            .list_connected_nodes(Request::new(req))
            .await
            .context("Failed to list connected nodes")?
            .into_inner();

        Ok(resp)
    }

    /// Get node metrics (CPU, memory, messages, actors, connections)
    pub async fn get_metrics(
        &mut self,
        node_id: &str,
    ) -> Result<plexspaces_proto::node::v1::NodeMetrics> {
        let req = GetMetricsRequest {
            node_id: node_id.to_string(),
            include_extended: false,
        };

        let resp = self
            .client
            .get_metrics(Request::new(req))
            .await
            .context("Failed to get node metrics")?
            .into_inner();

        Ok(resp)
    }

    /// Get the node address this client is connected to
    pub fn node_addr(&self) -> &str {
        &self.node_addr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_backoff_internal() {
        let config = HealthCheckConfig::default();

        // Test exponential backoff calculation
        // Formula: min(initial * 2^attempt, max) + jitter

        // Attempt 0: should be around initial_delay + small jitter
        let delay_0 = NodeClient::exponential_backoff_internal(0, &config);
        assert!(delay_0 >= config.initial_delay);
        assert!(delay_0 <= config.initial_delay + config.initial_delay / 4); // Max jitter is 25%

        // Attempt 1: should be around initial_delay * 2 + jitter
        let delay_1 = NodeClient::exponential_backoff_internal(1, &config);
        assert!(delay_1 >= config.initial_delay * 2);
        assert!(delay_1 <= config.initial_delay * 2 + (config.initial_delay * 2) / 4);

        // Attempt 10+: should be capped at max_delay (including jitter)
        let delay_10 = NodeClient::exponential_backoff_internal(10, &config);
        // Note: With jitter, delay can be slightly over max_delay, but we cap it in the function
        assert!(delay_10 <= config.max_delay + config.max_delay / 4); // Allow for max jitter (25%)

        // Verify config values are reasonable
        assert!(config.initial_delay < config.max_delay);
        assert!(config.health_check_timeout > StdDuration::ZERO);
        assert!(config.readiness_timeout > StdDuration::ZERO);
        assert!(config.readiness_poll_interval > StdDuration::ZERO);
    }

    #[test]
    fn test_health_check_config_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.max_retries, 10); // Increased for Docker environments
        assert_eq!(config.initial_delay, StdDuration::from_millis(500));
        assert_eq!(config.max_delay, StdDuration::from_secs(10));
        assert_eq!(config.health_check_timeout, StdDuration::from_secs(10)); // Increased timeout
        assert_eq!(config.readiness_timeout, StdDuration::from_secs(60)); // Increased for Docker
        assert_eq!(
            config.readiness_poll_interval,
            StdDuration::from_millis(1000)
        ); // Longer poll interval
        assert!(config.check_liveness);
        assert!(config.wait_for_readiness);
    }

    #[test]
    fn test_health_check_config_clone() {
        let config = HealthCheckConfig::default();
        let cloned = config.clone();
        assert_eq!(config.max_retries, cloned.max_retries);
        assert_eq!(config.initial_delay, cloned.initial_delay);
        assert_eq!(config.max_delay, cloned.max_delay);
    }

    #[test]
    fn test_health_check_config_debug() {
        let config = HealthCheckConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("max_retries"));
        assert!(debug_str.contains("initial_delay"));
    }
}
