// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Byzantine Generals Application
//
// Demonstrates Application framework pattern with:
// - Application trait (like Erlang application)
// - BehaviorRegistry for custom behaviors
// - SDK spawn helper for BehaviorRegistry-based actors
// - SDK message helpers (call_message, cast_message)
// - CoordinationComputeTracker for metrics

use anyhow::Result;
use async_trait::async_trait;
use plexspaces_actor::ActorRef;
use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_core::{BehaviorRegistry, RequestContext};
use plexspaces_node::CoordinationComputeTracker;
use plexspaces_sdk::{call_message, cast_message, json, spawn_with_behavior_type};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info};

use crate::config::ByzantineConfig;
use crate::general::{Decision, General, GeneralMessage, Value};

/// Byzantine Generals Application
pub struct ByzantineApplication {
    config: ByzantineConfig,
}

impl ByzantineApplication {
    /// Create from config
    pub fn from_config(config: ByzantineConfig) -> Result<Self, String> {
        config.validate()?;
        Ok(Self { config })
    }

    /// Create with defaults
    pub fn new(general_count: usize, byzantine_count: usize) -> Self {
        Self {
            config: ByzantineConfig {
                general_count,
                fault_count: byzantine_count,
                ..Default::default()
            },
        }
    }
}

#[async_trait]
impl Application for ByzantineApplication {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        // Initialize tracing - use try_init() to avoid panic if already initialized
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .try_init();

        info!("╔════════════════════════════════════════════════════════════╗");
        info!("║     Byzantine Generals - Consensus Example                ║");
        info!("╚════════════════════════════════════════════════════════════╝");
        println!();
        info!("Configuration:");
        info!("  Generals: {}", self.config.general_count);
        info!("  Byzantine (faulty): {}", self.config.fault_count);
        info!("  Consensus rounds: {}", self.config.num_rounds);
        println!();

        // Create metrics tracker
        let mut metrics_tracker =
            CoordinationComputeTracker::new("byzantine-consensus".to_string());
        let total_start = Instant::now();

        // Get ServiceLocator from node
        let service_locator = node.service_locator().ok_or_else(|| {
            ApplicationError::StartupFailed("ServiceLocator not available".to_string())
        })?;

        // =====================================================================
        // Step 1: Register ByzantineGeneral behavior
        // =====================================================================
        metrics_tracker.start_coordinate();
        let register_start = Instant::now();

        let behavior_registry = BehaviorRegistry::new();
        behavior_registry
            .register("ByzantineGeneral", |initial_state: &[u8]| {
                let initial_state = initial_state.to_vec();
                Box::pin(async move {
                    let state: serde_json::Value =
                        serde_json::from_slice(&initial_state).map_err(|e| {
                            plexspaces_core::BehaviorFactoryError::InvalidArguments(
                                "ByzantineGeneral".to_string(),
                                format!("Invalid JSON: {}", e),
                            )
                        })?;

                    let id = state["id"].as_u64().unwrap_or(0) as usize;
                    let source_id = state["source_id"].as_u64().unwrap_or(0) as usize;
                    let num_rounds = state["num_rounds"].as_u64().unwrap_or(1) as usize;

                    Ok(Box::new(General::new(id, source_id, num_rounds))
                        as Box<dyn plexspaces_core::Actor>)
                })
            })
            .await;

        service_locator
            .register_behavior_registry(Arc::new(behavior_registry))
            .await;

        let register_time = register_start.elapsed();
        metrics_tracker.end_coordinate();
        info!(
            "  Behavior registration: {:.2}ms",
            register_time.as_secs_f64() * 1000.0
        );
        println!();

        // =====================================================================
        // Step 2: Spawn generals using SDK spawn_with_behavior_type helper
        // =====================================================================
        metrics_tracker.start_coordinate();
        let spawn_start = Instant::now();

        let ctx =
            RequestContext::new_without_auth("byzantine".to_string(), "consensus".to_string());

        let source_id = 0usize;

        let mut general_refs: Vec<ActorRef> = Vec::new();

        for i in 0..self.config.general_count {
            let actor_name = format!("general-{}", i);
            let initial_state = serde_json::json!({
                "id": i,
                "source_id": source_id,
                "num_rounds": self.config.num_rounds,
            });

            // Use SDK spawn_with_behavior_type helper for BehaviorRegistry-based actors
            let general_ref = spawn_with_behavior_type(
                &ctx,
                service_locator.clone(),
                actor_name,
                "consensus",
                "ByzantineGeneral",
                serde_json::to_vec(&initial_state).unwrap(),
                vec![],
            )
            .await
            .map_err(|e| {
                ApplicationError::StartupFailed(format!("Failed to spawn general {}: {}", i, e))
            })?;

            general_refs.push(general_ref);
            metrics_tracker.increment_message();

            if i < 3 || i == self.config.general_count - 1 {
                info!("  ✓ Spawned general-{}", i);
            }
        }

        let spawn_time = spawn_start.elapsed();
        metrics_tracker.end_coordinate();
        info!(
            "  Spawned {} generals in {:.2}ms",
            self.config.general_count,
            spawn_time.as_secs_f64() * 1000.0
        );
        println!();

        // Wait for actors to be ready
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // =====================================================================
        // Step 3: Initialize generals with peer information
        // =====================================================================
        metrics_tracker.start_coordinate();
        let init_start = Instant::now();

        let peer_ids: Vec<usize> = (0..self.config.general_count).collect();

        for (i, general_ref) in general_refs.iter().enumerate() {
            let init_msg = cast_message(json!({
                "action": "Init",
                "peer_ids": peer_ids.iter().filter(|&&id| id != i).collect::<Vec<_>>(),
            }));

            general_ref.tell(init_msg).await.map_err(|e| {
                ApplicationError::StartupFailed(format!(
                    "Failed to initialize general {}: {}",
                    i, e
                ))
            })?;
            metrics_tracker.increment_message();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let init_time = init_start.elapsed();
        metrics_tracker.end_coordinate();
        info!(
            "  Initialized {} generals in {:.2}ms",
            self.config.general_count,
            init_time.as_secs_f64() * 1000.0
        );
        println!();

        // =====================================================================
        // Step 4: Run consensus protocol with message passing
        // =====================================================================
        info!(
            "Running consensus protocol ({} rounds)...",
            self.config.num_rounds
        );
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let mut consensus_results: Vec<HashMap<usize, Decision>> = Vec::new();

        for round in 0..self.config.num_rounds {
            info!("\nRound {}:", round + 1);

            metrics_tracker.start_coordinate();
            let round_start = Instant::now();

            // Phase 1: Source general broadcasts vote
            let source_value = if round % 2 == 0 {
                Value::Zero
            } else {
                Value::One
            };
            let source_msg = cast_message(json!({
                "action": "Vote",
                "from": source_id,
                "path": format!("{}", source_id),
                "value": match source_value {
                    Value::Zero => "Zero",
                    Value::One => "One",
                    Value::Retreat => "Retreat",
                },
            }));

            // Broadcast to all generals
            for general_ref in &general_refs {
                general_ref.tell(source_msg.clone()).await.map_err(|e| {
                    ApplicationError::StartupFailed(format!("Failed to send vote: {}", e))
                })?;
                metrics_tracker.increment_message();
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            // Phase 2: Generals relay votes to each other
            // Each general sends its vote to all other generals
            for (i, general_ref) in general_refs.iter().enumerate() {
                let vote_value = if i == source_id {
                    source_value
                } else if i == 2 || i == source_id {
                    // Byzantine generals send conflicting votes
                    Value::One
                } else {
                    // Honest generals relay what they received
                    source_value
                };

                for (j, target_ref) in general_refs.iter().enumerate() {
                    if i != j {
                        let relay_msg = cast_message(json!({
                            "action": "Vote",
                            "from": i,
                            "path": format!("{}-{}", source_id, i),
                            "value": match vote_value {
                                Value::Zero => "Zero",
                                Value::One => "One",
                                Value::Retreat => "Retreat",
                            },
                        }));

                        target_ref.tell(relay_msg).await.map_err(|e| {
                            ApplicationError::StartupFailed(format!("Failed to relay vote: {}", e))
                        })?;
                        metrics_tracker.increment_message();
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            // Phase 3: Collect decisions
            metrics_tracker.start_compute();
            let compute_start = Instant::now();

            let mut round_decisions: HashMap<usize, Decision> = HashMap::new();

            for (i, general_ref) in general_refs.iter().enumerate() {
                let decision_msg = call_message(json!({
                    "action": "GetDecision",
                }));

                let reply = general_ref
                    .ask(decision_msg, std::time::Duration::from_secs(5))
                    .await
                    .map_err(|e| {
                        ApplicationError::StartupFailed(format!(
                            "Failed to get decision from general {}: {}",
                            i, e
                        ))
                    })?;

                let decision: Decision = serde_json::from_slice(&reply.payload).map_err(|e| {
                    ApplicationError::StartupFailed(format!(
                        "Failed to deserialize decision: {}",
                        e
                    ))
                })?;

                round_decisions.insert(i, decision);
                metrics_tracker.increment_message();
            }

            let compute_time = compute_start.elapsed();
            metrics_tracker.end_compute();

            let round_time = round_start.elapsed();
            metrics_tracker.end_coordinate();

            consensus_results.push(round_decisions.clone());

            // Count consensus
            let zero_count = round_decisions
                .values()
                .filter(|d| d.value == Value::Zero && !d.is_faulty)
                .count();
            let one_count = round_decisions
                .values()
                .filter(|d| d.value == Value::One && !d.is_faulty)
                .count();

            info!(
                "  Round {} complete: {:.2}ms (compute: {:.2}ms)",
                round + 1,
                round_time.as_secs_f64() * 1000.0,
                compute_time.as_secs_f64() * 1000.0
            );
            info!(
                "  Decisions: {} Zero, {} One (honest generals)",
                zero_count, one_count
            );
        }

        println!();

        // =====================================================================
        // Step 5: Analyze results and display metrics
        // =====================================================================
        let total_time = total_start.elapsed();
        let metrics = metrics_tracker.finalize();

        // Calculate consensus success rate
        let mut consensus_reached = 0;
        for decisions in &consensus_results {
            let zero_count = decisions
                .values()
                .filter(|d| d.value == Value::Zero && !d.is_faulty)
                .count();
            let one_count = decisions
                .values()
                .filter(|d| d.value == Value::One && !d.is_faulty)
                .count();
            let honest_count = self.config.general_count - self.config.fault_count;
            let quorum = (2 * self.config.general_count) / 3;

            if zero_count >= quorum || one_count >= quorum {
                consensus_reached += 1;
            }
        }

        let consensus_rate = if self.config.num_rounds > 0 {
            (consensus_reached as f64 / self.config.num_rounds as f64) * 100.0
        } else {
            0.0
        };

        // Calculate benchmark metrics
        let total_messages = metrics.message_count;
        let total_time_secs = total_time.as_secs_f64();
        let messages_per_sec = if total_time_secs > 0.0 {
            total_messages as f64 / total_time_secs
        } else {
            0.0
        };

        // Print metrics prominently - use println! to ensure they're always visible
        println!();
        println!("{}", "=".repeat(80));
        println!("📊 PERFORMANCE METRICS & BENCHMARKS");
        println!("{}", "=".repeat(80));

        // Print metrics prominently - use println! to ensure they're always visible
        println!();
        println!("{}", "=".repeat(80));
        println!("📊 PERFORMANCE METRICS & BENCHMARKS");
        println!("{}", "=".repeat(80));

        println!("\nProblem Size:");
        println!("  Generals: {}", self.config.general_count);
        println!("  Byzantine (faulty): {}", self.config.fault_count);
        println!(
            "  Honest: {}",
            self.config.general_count - self.config.fault_count
        );
        println!("  Consensus rounds: {}", self.config.num_rounds);
        println!("  Total messages: {}", total_messages);

        println!("\n{}", "─".repeat(80));
        println!("⚡ LATENCY BREAKDOWN (Coordination vs Computation)");
        println!("{}", "─".repeat(80));
        println!(
            "  Behavior registration: {:>12.2} ms (coordination)",
            register_time.as_secs_f64() * 1000.0
        );
        println!(
            "  Actor spawning:        {:>12.2} ms (coordination)",
            spawn_time.as_secs_f64() * 1000.0
        );
        println!(
            "  Initialization:        {:>12.2} ms (coordination)",
            init_time.as_secs_f64() * 1000.0
        );
        println!(
            "  Message passing:       {:>12.2} ms (coordination)",
            metrics.coordinate_duration_ms as f64
                - register_time.as_secs_f64() * 1000.0
                - spawn_time.as_secs_f64() * 1000.0
                - init_time.as_secs_f64() * 1000.0
        );
        println!(
            "  Decision computation:  {:>12.2} ms (computation)",
            metrics.compute_duration_ms as f64
        );
        println!("  {}", "─".repeat(30));
        println!(
            "  Coordination: {:>10.2} ms (total)",
            metrics.coordinate_duration_ms as f64
        );
        println!(
            "  Computation:  {:>10.2} ms (total)",
            metrics.compute_duration_ms as f64
        );
        println!(
            "  Total Time:   {:>10.2} ms ({:.2} seconds)",
            total_time.as_secs_f64() * 1000.0,
            total_time_secs
        );

        println!("\n{}", "─".repeat(80));
        println!("📈 COORDINATION vs COMPUTATION ANALYSIS");
        println!("{}", "─".repeat(80));
        println!(
            "  Computation time:      {:>12.2} ms",
            metrics.compute_duration_ms as f64
        );
        println!(
            "  Coordination time:    {:>12.2} ms",
            metrics.coordinate_duration_ms as f64
        );
        println!(
            "  Granularity ratio:     {:>12.2}× (compute/coordinate)",
            metrics.granularity_ratio
        );
        println!(
            "  Efficiency:            {:>12.2}% (compute/total)",
            metrics.efficiency * 100.0
        );
        println!("  Message count:         {:>12}", metrics.message_count);
        println!("  Barrier count:         {:>12}", metrics.barrier_count);

        // Cost analysis
        let coord_cost_pct = if metrics.total_duration_ms > 0 {
            (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        };
        let compute_cost_pct = if metrics.total_duration_ms > 0 {
            (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
        } else {
            0.0
        };
        println!("\n  Cost Breakdown:");
        println!(
            "    Coordination overhead: {:>8.2}% of total time",
            coord_cost_pct
        );
        println!(
            "    Computation:           {:>8.2}% of total time",
            compute_cost_pct
        );

        println!("\n{}", "─".repeat(80));
        println!("🚀 BENCHMARK METRICS");
        println!("{}", "─".repeat(80));
        println!("  Messages/sec:          {:>12.2} msg/s", messages_per_sec);
        println!(
            "  Consensus success:     {:>12.2}% ({} of {} rounds)",
            consensus_rate, consensus_reached, self.config.num_rounds
        );
        println!(
            "  Avg messages/round:    {:>12.2}",
            total_messages as f64 / self.config.num_rounds as f64
        );

        println!("\n{}", "─".repeat(80));
        println!("💡 ANALYSIS & RECOMMENDATIONS");
        println!("{}", "─".repeat(80));
        if metrics.granularity_ratio < 10.0 {
            println!("  ⚠️  WARNING: Overhead too high! Consider:");
            println!("     - More consensus rounds (increases computation)");
            println!("     - Larger problem size (more generals)");
            println!(
                "     - Current ratio: {:.2}× (should be >= 10×)",
                metrics.granularity_ratio
            );
        } else if metrics.granularity_ratio < 100.0 {
            println!("  ✓  ACCEPTABLE: Reasonable granularity for this problem size");
            println!(
                "     - Ratio: {:.2}× (good for consensus algorithms)",
                metrics.granularity_ratio
            );
        } else {
            println!("  ✓  EXCELLENT: Good compute/coordinate ratio");
            println!(
                "     - Ratio: {:.2}× (ideal for parallel efficiency)",
                metrics.granularity_ratio
            );
        }

        if coord_cost_pct > 20.0 {
            println!(
                "  ⚠️  Coordination overhead is {:.1}% - consider more computation per round",
                coord_cost_pct
            );
        } else {
            println!(
                "  ✓  Coordination overhead is {:.1}% - acceptable",
                coord_cost_pct
            );
        }

        println!("{}", "=".repeat(80));
        println!();

        // Summary
        let honest_count = self.config.general_count - self.config.fault_count;
        let quorum = (2 * self.config.general_count) / 3;

        println!("Consensus Summary:");
        println!("  Honest generals: {}", honest_count);
        println!("  Byzantine (faulty): {}", self.config.fault_count);
        println!("  Quorum needed: {}", quorum);

        if honest_count >= quorum {
            println!("  ✅ Consensus protocol VALID (honest >= quorum)");
        } else {
            println!("  ❌ Consensus protocol INVALID (too many byzantine nodes)");
        }

        println!("\n✅ Byzantine Generals Application started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        info!("🛑 Stopping Byzantine Generals Application");
        info!("✅ Stopped");
        Ok(())
    }

    fn name(&self) -> &str {
        "byzantine-generals"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_application() {
        let app = ByzantineApplication::new(4, 1);
        assert_eq!(app.name(), "byzantine-generals");
    }

    #[test]
    fn test_invalid_config() {
        let config = ByzantineConfig {
            general_count: 3, // Invalid: needs at least 4
            fault_count: 0,
            ..Default::default()
        };
        assert!(ByzantineApplication::from_config(config).is_err());
    }
}
