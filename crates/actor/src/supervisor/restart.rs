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

//! Restart strategy, backoff logic, and escalation policies.

use indexmap::IndexMap;
use metrics;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, instrument, warn};

use crate::core::{ActorContext, ActorId};
use crate::supervisor::tree::SupervisedChild;
use plexspaces_proto::actor::v1::ActorVisibility;
use plexspaces_proto::supervision::v1::SupervisorStats;

use super::{
    SupervisedSupervisor, SupervisionStrategy, Supervisor, SupervisorError, SupervisorEvent,
};

/// Restart policy for individual actors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RestartPolicy {
    /// Always restart
    Permanent,
    /// Restart only on abnormal exit
    Transient,
    /// Never restart
    Temporary,
    /// Exponential backoff
    ExponentialBackoff {
        /// Initial delay in milliseconds before the first restart attempt.
        initial_delay_ms: u64,
        /// Maximum delay in milliseconds between restart attempts.
        max_delay_ms: u64,
        /// Multiplier applied to the delay after each restart.
        factor: f64,
    },
}

/// Action decided during restart intensity check.
pub(crate) enum RestartAction {
    /// Intensity exceeded — emit event, escalate to parent.
    Exceeded,
    /// Temporary policy — do not restart.
    Skip,
    /// Restart after optional backoff delay.
    Restart { delay_ms: u64, restart_count: u32 },
}

/// Calculate exponential backoff delay
pub(crate) fn calculate_backoff_delay(
    restart_count: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
    factor: f64,
) -> u64 {
    let delay = initial_delay_ms as f64 * factor.powi(restart_count as i32);
    delay.min(max_delay_ms as f64) as u64
}

/// Core restart logic for a failed child supervisor.
///
/// Shared by both the inline monitor task (spawned per child supervisor) and the public
/// `Supervisor::restart_supervisor` method (called by a parent supervisor).  Keeping the
/// logic in one place means the restart policy, intensity window, and escalation path are
/// identical regardless of who triggers the restart.
///
/// Mirrors `restart_one` for child actors: tracks restart timestamps within the rolling
/// window, emits `MaxRestartsExceeded` and escalates to the parent when the threshold is
/// hit, otherwise restarts the child supervisor's task and emits `ChildRestarted`.
/// Returns `Ok(Some(handle))` when the child was restarted — the handle is the new child
/// supervisor task the caller should await next.  Returns `Ok(None)` when the restart
/// policy says not to restart (Temporary policy or entry removed).  Returns `Err` when
/// max restarts is exceeded.
pub(crate) async fn restart_supervisor_with_state(
    supervisor_id: &str,
    max_restarts: u32,
    within_seconds: u64,
    child_supervisors: &Arc<RwLock<IndexMap<String, SupervisedSupervisor>>>,
    event_tx: &mpsc::Sender<SupervisorEvent>,
    parent: Option<Arc<Supervisor>>,
    self_id: &str,
) -> Result<Option<tokio::task::JoinHandle<()>>, SupervisorError> {
    let now = tokio::time::Instant::now();
    let window = Duration::from_secs(within_seconds);

    // Decide the action under write lock; release before any async work.
    let action = {
        let mut sups = child_supervisors.write().await;
        let Some(s) = sups.get_mut(supervisor_id) else {
            return Ok(None); // already removed via remove_supervisor_child
        };
        s.restart_timestamps
            .retain(|&t| now.duration_since(t) < window);

        if s.restart_timestamps.len() >= max_restarts as usize {
            RestartAction::Exceeded
        } else {
            s.restart_timestamps.push(now);
            s.restart_count += 1;
            s.last_restart = Some(now);
            match s.restart {
                RestartPolicy::Temporary => RestartAction::Skip,
                RestartPolicy::ExponentialBackoff {
                    initial_delay_ms,
                    max_delay_ms,
                    factor,
                } => RestartAction::Restart {
                    delay_ms: calculate_backoff_delay(
                        s.restart_count,
                        initial_delay_ms,
                        max_delay_ms,
                        factor,
                    ),
                    restart_count: s.restart_count,
                },
                _ => RestartAction::Restart {
                    delay_ms: 0,
                    restart_count: s.restart_count,
                },
            }
        }
    };

    match action {
        RestartAction::Exceeded => {
            let _ = event_tx
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventMaxRestartsExceeded as i32,
                    actor_id: supervisor_id.to_string(),
                    ..Default::default()
                })
                .await;
            if let Some(p) = &parent {
                let _ = p
                    .handle_failure(
                        &self_id.to_string().into(),
                        format!("Child supervisor {} exceeded max restarts", supervisor_id),
                        Some(crate::core::ExitReason::Error(
                            "Max restarts exceeded".to_string(),
                        )),
                    )
                    .await;
            }
            Err(SupervisorError::MaxRestartsExceeded)
        }
        RestartAction::Skip => Ok(None),
        RestartAction::Restart {
            delay_ms,
            restart_count,
        } => {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            let supervisor_arc = {
                let sups = child_supervisors.read().await;
                sups.get(supervisor_id).map(|s| s.supervisor.clone())
            };
            if let Some(arc) = supervisor_arc {
                match arc.write().await.start().await {
                    Ok(new_task_handle) => {
                        let _ = event_tx
                            .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                                event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted as i32,
                                actor_id: supervisor_id.to_string(),
                                restart_count,
                                ..Default::default()
                            })
                            .await;
                        // Return the new task handle; the monitor loop will await it.
                        return Ok(Some(new_task_handle));
                    }
                    Err(e) => {
                        warn!(
                            child_supervisor_id = %supervisor_id,
                            error = %e.message,
                            "Failed to restart child supervisor"
                        );
                    }
                }
            }
            Ok(None)
        }
    }
}

impl Supervisor {
    /// Handle child failure
    ///
    /// ## Arguments
    /// * `id` - Actor ID that failed
    /// * `reason` - Failure reason as a string (parsed when `exit_reason` is absent)
    /// * `exit_reason` - Exit reason (None if unknown, will be parsed from reason string)
    #[instrument(skip(self), fields(supervisor_id = %self.id, child_id = %id, reason = %reason))]
    pub async fn handle_failure(
        &self,
        id: &ActorId,
        reason: String,
        exit_reason: Option<crate::core::ExitReason>,
    ) -> Result<(), SupervisorError> {
        let exit_reason = exit_reason.or_else(|| reason.parse::<crate::core::ExitReason>().ok());

        warn!(
            supervisor_id = %self.id,
            child_id = %id,
            reason = %reason,
            exit_reason = ?exit_reason,
            "Handling child failure"
        );
        // Record failure pattern (in a separate scope to release lock immediately)
        {
            let mut stats = self.stats.write().await;
            *stats.failure_patterns.entry(reason.clone()).or_insert(0) += 1;
            let failure_count = stats.failure_patterns.get(&reason).copied().unwrap_or(0);
            tracing::trace!(
                supervisor_id = %self.id,
                child_id = %id,
                reason = %reason,
                failure_count = failure_count,
                "Recorded failure pattern"
            );
        } // Drop stats lock here

        // Send failure event
        let _ = self
            .event_tx
            .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildFailed as i32,
                actor_id: id.to_string(),
                reason: reason.clone(),
                ..Default::default()
            })
            .await;

        // Apply supervision strategy
        // NOTE: restart_* methods will acquire their own locks, so we must NOT hold any locks here
        // Read strategy and clone the relevant parts to avoid holding the lock
        let strategy = self.strategy.read().await.clone();
        match strategy {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_one(id, max_restarts, within_seconds, exit_reason)
                    .await?;
            }
            SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            } => {
                self.restart_all(max_restarts, within_seconds, exit_reason.clone())
                    .await?;
            }
            SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_rest_for_one(id, max_restarts, within_seconds, exit_reason.clone())
                    .await?;
            }
            SupervisionStrategy::Adaptive {
                initial_strategy,
                learning_rate,
            } => {
                // Apply adaptive strategy based on failure patterns
                self.apply_adaptive_strategy(id, &reason, &initial_strategy, learning_rate)
                    .await?;
            }
            SupervisionStrategy::Custom { name } => {
                return Err(SupervisorError::InvalidStrategy(format!(
                    "Custom supervision strategy '{}' is not registered; \
                     register a handler before using Custom strategy",
                    name
                )));
            }
        }

        Ok(())
    }

    /// Restart a failed child supervisor, applying the parent's supervision strategy.
    ///
    /// Called by a parent supervisor when it detects that a child supervisor has failed.
    /// Delegates to `restart_supervisor_with_state` which owns the restart logic so it
    /// can be shared with the per-supervisor monitor task.
    pub async fn restart_supervisor(
        &self,
        supervisor_id: &str,
        max_restarts: u32,
        within_seconds: u64,
    ) -> Result<(), SupervisorError> {
        restart_supervisor_with_state(
            supervisor_id,
            max_restarts,
            within_seconds,
            &self.child_supervisors,
            &self.event_tx,
            self.parent.clone(),
            &self.id,
        )
        .await
        .map(|_| ()) // discard the returned handle; monitor task already loops internally
    }

    /// Restart a single actor (one-for-one)
    ///
    /// ## Arguments
    /// * `id` - Actor ID to restart
    /// * `max_restarts` - Maximum number of restarts allowed
    /// * `within_seconds` - Time window for max_restarts
    /// * `exit_reason` - Exit reason for the termination (None if unknown)
    async fn restart_one(
        &self,
        id: &ActorId,
        max_restarts: u32,
        within_seconds: u64,
        exit_reason: Option<crate::core::ExitReason>,
    ) -> Result<(), SupervisorError> {
        let mut children = self.children.write().await;
        let mut stats = self.stats.write().await;

        if let Some(child) = children.get_mut(id) {
            // Track restart intensity using restart_timestamps
            let now = tokio::time::Instant::now();
            let window_duration = Duration::from_secs(within_seconds);

            // Remove old restarts outside the time window
            child
                .restart_timestamps
                .retain(|&restart_time| now.duration_since(restart_time) < window_duration);

            // Check if we've exceeded max_restarts within the time window
            if child.restart_timestamps.len() >= max_restarts as usize {
                error!(
                    supervisor_id = %self.id,
                    child_id = %id,
                    restart_count = child.restart_timestamps.len(),
                    max_restarts = max_restarts,
                    within_seconds = within_seconds,
                    "Max restarts exceeded for child"
                );
                let _ = self
                    .event_tx
                    .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                        event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventMaxRestartsExceeded as i32,
                        actor_id: id.to_string(),
                        ..Default::default()
                    })
                    .await;
                return Err(SupervisorError::MaxRestartsExceeded);
            }

            // Record this restart attempt in history
            child.restart_timestamps.push(now);

            // Apply restart policy
            use crate::child_spec::ProtoRestartPolicy;
            match child.spec.restart_policy() {
                ProtoRestartPolicy::RestartPolicyPermanent
                | ProtoRestartPolicy::RestartPolicyUnspecified => {
                    // Always restart (regardless of exit reason)
                    self.perform_restart(child, &mut stats).await?;
                }
                ProtoRestartPolicy::RestartPolicyTransient => {
                    // Erlang/OTP semantics: Only restart on abnormal exit
                    // Normal exits: Normal, Shutdown
                    // Abnormal exits: Error, Killed, Linked(abnormal)
                    let should_restart = match &exit_reason {
                        Some(reason) => {
                            // Check if exit was abnormal
                            match reason {
                                crate::core::ExitReason::Normal
                                | crate::core::ExitReason::Shutdown => {
                                    // Normal termination - don't restart
                                    false
                                }
                                crate::core::ExitReason::Error(_)
                                | crate::core::ExitReason::Killed => {
                                    // Abnormal termination - restart
                                    true
                                }
                                crate::core::ExitReason::Linked {
                                    reason: linked_reason,
                                    ..
                                } => {
                                    // Linked actor died - check if the linked reason was abnormal
                                    // If linked actor died with normal/shutdown, this is still normal
                                    // If linked actor died with error/killed, this is abnormal
                                    matches!(
                                        linked_reason.as_ref(),
                                        crate::core::ExitReason::Error(_)
                                            | crate::core::ExitReason::Killed
                                    )
                                }
                            }
                        }
                        None => {
                            // If exit reason is unknown, assume abnormal (conservative approach)
                            // This ensures we restart if we can't determine the reason
                            warn!(
                                supervisor_id = %self.id,
                                child_id = %id,
                                "Exit reason unknown for Transient actor, assuming abnormal (restarting)"
                            );
                            true
                        }
                    };

                    if should_restart {
                        self.perform_restart(child, &mut stats).await?;
                    } else {
                        info!(
                            supervisor_id = %self.id,
                            child_id = %id,
                            exit_reason = ?exit_reason,
                            "Transient actor terminated normally (Normal/Shutdown), not restarting"
                        );
                        // Don't restart - actor terminated normally (Erlang/OTP semantics)
                        return Ok(());
                    }
                }
                ProtoRestartPolicy::RestartPolicyTemporary => {
                    // Don't restart
                    return Ok(());
                }
                ProtoRestartPolicy::RestartPolicyExponentialBackoff => {
                    // Always restart (like Permanent) - backoff is applied by the caller
                    self.perform_restart(child, &mut stats).await?;
                }
            }

            // Send restart event
            info!(
                supervisor_id = %self.id,
                child_id = %id,
                restart_count = child.restart_count,
                restart_policy = ?child.spec.restart_policy(),
                "Child restarted successfully"
            );
            let _ = self
                .event_tx
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventChildRestarted as i32,
                    actor_id: id.to_string(),
                    restart_count: child.restart_count,
                    ..Default::default()
                })
                .await;
        }

        Ok(())
    }

    /// Restart all actors (one-for-all)
    async fn restart_all(
        &self,
        max_restarts: u32,
        within_seconds: u64,
        exit_reason: Option<crate::core::ExitReason>,
    ) -> Result<(), SupervisorError> {
        let children = self.children.read().await;
        let ids: Vec<ActorId> = children.keys().cloned().collect();
        drop(children);

        for id in ids {
            self.restart_one(&id, max_restarts, within_seconds, exit_reason.clone())
                .await?;
        }

        Ok(())
    }

    /// Restart failed actor and all started after it (rest-for-one)
    async fn restart_rest_for_one(
        &self,
        failed_id: &ActorId,
        max_restarts: u32,
        within_seconds: u64,
        exit_reason: Option<crate::core::ExitReason>,
    ) -> Result<(), SupervisorError> {
        // IndexMap preserves insertion order, so we can find position of failed actor
        // and restart all actors from that position onwards

        let children = self.children.read().await;

        // Find the index of the failed actor
        let failed_index = children.get_index_of(failed_id);

        if failed_index.is_none() {
            drop(children);
            return Err(SupervisorError::ChildNotFound(failed_id.to_string().into()));
        }

        let failed_idx = failed_index.unwrap();

        // Collect IDs of failed actor + all actors started after it
        let ids_to_restart: Vec<ActorId> = children
            .iter()
            .skip(failed_idx) // Skip actors before failed one
            .map(|(id, _)| id.clone())
            .collect();

        drop(children);

        // Restart all actors in order (failed + rest)
        for id in ids_to_restart {
            self.restart_one(&id, max_restarts, within_seconds, exit_reason.clone())
                .await?;
        }

        Ok(())
    }

    /// Apply adaptive supervision strategy
    ///
    /// ## Adaptation Logic
    /// - If failed_restarts > successful_restarts * 2: Switch to OneForAll (more conservative)
    /// - Otherwise: Use initial strategy
    /// - Emit StrategyAdapted event when strategy changes
    async fn apply_adaptive_strategy(
        &self,
        id: &ActorId,
        _reason: &str,
        initial_strategy: &SupervisionStrategy,
        _learning_rate: f64,
    ) -> Result<(), SupervisorError> {
        // Check stats to determine if we should adapt strategy
        let (should_adapt, new_strategy) = {
            let stats = self.stats.read().await;
            let should_be_conservative = stats.failed_restarts > stats.successful_restarts * 2;

            if should_be_conservative {
                // Adapt to more conservative strategy (OneForAll)
                match initial_strategy {
                    SupervisionStrategy::OneForOne {
                        max_restarts,
                        within_seconds,
                    } => {
                        // Switch from OneForOne to OneForAll
                        let new_strat = SupervisionStrategy::OneForAll {
                            max_restarts: *max_restarts,
                            within_seconds: *within_seconds,
                        };
                        (true, new_strat)
                    }
                    other => (false, other.clone()),
                }
            } else {
                // Keep initial strategy
                (false, initial_strategy.clone())
            }
        }; // Drop stats lock

        // If strategy changed, update it and emit event
        if should_adapt {
            {
                let mut strategy = self.strategy.write().await;
                *strategy = SupervisionStrategy::Adaptive {
                    initial_strategy: Box::new(new_strategy.clone()),
                    learning_rate: _learning_rate,
                };
            } // Drop strategy lock

            // Increment strategy_adaptations counter
            {
                let mut stats = self.stats.write().await;
                stats.strategy_adaptations += 1;
            }

            // Emit StrategyAdapted event
            let _ = self
                .event_tx
                .send(plexspaces_proto::supervision::v1::SupervisorEvent {
                    event_type: plexspaces_proto::supervision::v1::SupervisorEventType::SupervisorEventStrategyAdapted as i32,
                    ..Default::default()
                })
                .await;
        }

        // Apply the strategy (adapted or original)
        match new_strategy {
            SupervisionStrategy::OneForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_one(id, max_restarts, within_seconds, None)
                    .await?;
            }
            SupervisionStrategy::OneForAll {
                max_restarts,
                within_seconds,
            } => {
                self.restart_all(max_restarts, within_seconds, None).await?;
            }
            SupervisionStrategy::RestForOne {
                max_restarts,
                within_seconds,
            } => {
                self.restart_rest_for_one(id, max_restarts, within_seconds, None)
                    .await?;
            }
            _ => {
                // Fallback: use initial strategy
                if let SupervisionStrategy::OneForOne {
                    max_restarts,
                    within_seconds,
                } = initial_strategy
                {
                    self.restart_one(id, *max_restarts, *within_seconds, None)
                        .await?;
                }
            }
        }

        Ok(())
    }

    /// Perform the actual restart
    #[instrument(skip(self, child, stats), fields(supervisor_id = %self.id, child_id = %child.spec.actor_id))]
    pub(crate) async fn perform_restart(
        &self,
        child: &mut super::SupervisedActor,
        stats: &mut SupervisorStats,
    ) -> Result<(), SupervisorError> {
        use crate::child_spec::StartedChild;

        let child_actor_id = child.spec.actor_id.clone();
        let child_id = child_actor_id.to_string();
        debug!(
            supervisor_id = %self.id,
            child_id = %child_actor_id,
            "Performing child restart"
        );
        stats.total_restarts += 1;

        // Stop the old actor if it's still running
        if let Some(handle) = child.handle.take() {
            handle.abort();
        }

        // Create new actor via async factory
        let started_child = (child.spec.start_fn)().await.map_err(|e| {
            stats.failed_restarts += 1;
            error!(
                supervisor_id = %self.id,
                child_id = %child_id,
                error = %e,
                "Failed to create actor during restart"
            );
            SupervisorError::RestartFailed(e.to_string())
        })?;

        // Extract actor from StartedChild (must be Worker for actor restart)
        let mut new_actor = match started_child {
            StartedChild::Worker { actor, .. } => actor,
            StartedChild::Supervisor { .. } => {
                stats.failed_restarts += 1;
                return Err(SupervisorError::RestartFailed(
                    "Cannot restart actor as supervisor".to_string(),
                ));
            }
        };

        // Phase 1: Unified Lifecycle - Restore facets from ChildSpec during restart
        // Facets are stored in ChildSpec and restored here to ensure they're reattached
        if !child.spec.proto.facets.is_empty() {
            // Get FacetRegistry from ServiceLocator to create facets from proto
            if let Some(service_locator) = &self.service_locator {
                if let Some(facet_registry_wrapper) = service_locator.get_facet_registry().await {
                    let facet_registry = facet_registry_wrapper.inner_clone();
                    // Use facet_helpers to create facets from proto
                    use crate::create_facets_from_proto;
                    let facets =
                        create_facets_from_proto(&child.spec.proto.facets, &facet_registry).await;

                    // Attach facets to the new actor before starting
                    // Phase 2: Supervisor Facet Metrics - Record metrics for facet restoration
                    let facet_restore_start = std::time::Instant::now();
                    let mut restored_count = 0;

                    for facet in facets {
                        if let Err(e) = new_actor.attach_facet(facet).await {
                            warn!(
                                supervisor_id = %self.id,
                                child_id = %child_id,
                                error = %e,
                                "Failed to attach facet during restart (continuing with other facets)"
                            );
                            metrics::counter!("plexspaces_supervisor_facet_restore_errors_total",
                                "supervisor_id" => self.id.clone(),
                                "child_id" => child_id.to_string()
                            )
                            .increment(1);
                        } else {
                            restored_count += 1;
                        }
                    }

                    let facet_restore_duration = facet_restore_start.elapsed();
                    metrics::histogram!("plexspaces_supervisor_facet_restore_duration_seconds",
                        "supervisor_id" => self.id.clone(),
                        "child_id" => child_id.to_string()
                    )
                    .record(facet_restore_duration.as_secs_f64());
                    metrics::counter!("plexspaces_supervisor_facets_restored_total",
                        "supervisor_id" => self.id.clone(),
                        "child_id" => child_id.to_string()
                    )
                    .increment(restored_count);

                    debug!(
                        supervisor_id = %self.id,
                        child_id = %child_id,
                        facet_count = child.spec.proto.facets.len(),
                        restored_count = restored_count,
                        duration_ms = facet_restore_duration.as_millis(),
                        "Restored facets from ChildSpec during restart"
                    );
                } else {
                    debug!(
                        supervisor_id = %self.id,
                        child_id = %child_id,
                        facet_count = child.spec.proto.facets.len(),
                        "FacetRegistry not available - facets not restored (graceful degradation)"
                    );
                }
            }
        }

        // Inject supervisor's service_locator into actor context before restarting so the
        // restarted actor can be registered in ActorRegistry afterward.
        if let Some(service_locator) = &self.service_locator {
            let existing_ctx = new_actor.context().clone();
            let self_ref = crate::core::ActorRef::new(new_actor.id().clone())
                .map_err(|e| SupervisorError::RestartFailed(e.to_string()))?;
            let new_ctx = Arc::new(
                ActorContext::new(
                    existing_ctx.node_id.clone(),
                    existing_ctx.tenant_id.clone(),
                    existing_ctx.namespace.clone(),
                    service_locator.clone(),
                    existing_ctx.config.clone(),
                )
                .with_self_ref(self_ref),
            );
            new_actor = new_actor.set_context(new_ctx);
        }

        // Start the new actor
        // Facets are already attached, so facet lifecycle hooks will be called during start()
        let handle = new_actor.start().await.map_err(|e| {
            stats.failed_restarts += 1;
            error!(
                supervisor_id = %self.id,
                child_id = %child_id,
                error = %e,
                "Failed to start actor during restart"
            );
            SupervisorError::RestartFailed(e.to_string())
        })?;

        if let Some(service_locator) = &self.service_locator {
            if let Some(registry) = service_locator.actor_registry().await {
                new_actor
                    .register_started(&registry, ActorVisibility::ActorVisibilityPublic)
                    .await;
            }
        }

        // Update child state
        child.actor = Arc::new(tokio::sync::RwLock::new(new_actor));
        child.handle = Some(handle);
        child.restart_count += 1;
        child.last_restart = Some(tokio::time::Instant::now());
        stats.successful_restarts += 1;

        // OBSERVABILITY: Record metrics for child restarted (Phase 8)
        metrics::counter!("plexspaces_supervisor_child_restarted_total",
            "supervisor_id" => self.id.clone(),
            "child_id" => child_id.to_string()
        )
        .increment(1);

        debug!(
            supervisor_id = %self.id,
            child_id = %child_id,
            restart_count = child.restart_count,
            "Child restart completed successfully"
        );

        Ok(())
    }
}
