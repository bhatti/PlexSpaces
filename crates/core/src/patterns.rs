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

//! Common Actor Patterns
//!
//! This module provides helper functions and builders for common actor patterns
//! to reduce boilerplate and improve developer experience.
//!
//! ## Patterns
//!
//! - **Counter**: Simple stateful counter actor
//! - **Worker**: Generic worker actor for processing tasks
//! - **Coordinator**: Actor that coordinates multiple workers

use crate::{Actor, ActorContext, BehaviorError, BehaviorType};
use async_trait::async_trait;
use plexspaces_mailbox::Message;
use std::sync::{Arc, Mutex};

/// Counter actor pattern
///
/// A simple stateful counter that can be incremented, decremented, and queried.
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_core::patterns::CounterActor;
/// # use plexspaces_actor::ActorBuilder;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let actor = ActorBuilder::new(Box::new(CounterActor::new()))
///     .with_name("counter")
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
pub struct CounterActor {
    count: Arc<Mutex<i64>>,
}

impl CounterActor {
    /// Create a new counter actor starting at 0
    pub fn new() -> Self {
        Self {
            count: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new counter actor with initial value
    pub fn with_initial_value(initial: i64) -> Self {
        Self {
            count: Arc::new(Mutex::new(initial)),
        }
    }

    /// Get current count value
    pub fn get_count(&self) -> i64 {
        *self.count.lock().unwrap()
    }
}

#[async_trait]
impl Actor for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        match msg.message_type.as_str() {
            "increment" => {
                let mut count = self.count.lock().unwrap();
                *count += 1;
            }
            "decrement" => {
                let mut count = self.count.lock().unwrap();
                *count -= 1;
            }
            "add" => {
                if let Ok(value_str) = String::from_utf8(msg.payload.clone()) {
                    if let Ok(value) = value_str.parse::<i64>() {
                        let mut count = self.count.lock().unwrap();
                        *count += value;
                    }
                }
            }
            "get" => {
                let count = *self.count.lock().unwrap();
                // Send reply using ActorService if sender is available
                // Note: ActorService is accessed via ServiceLocator, but we can't easily get it in core
                // due to circular dependencies. For now, reply sending is disabled in patterns.
                // Actors should handle replies directly using ctx.service_locator or ActorRef.
                if let Some(_sender_id) = msg.sender_id() {
                    // TODO: Implement reply sending via ServiceLocator or ActorRef
                    // For now, replies are not sent from patterns
                    // let reply = Message::new(count.to_string().into_bytes())
                    //     .with_message_type("count".to_string());
                    // Use ActorRef or ActorService from ServiceLocator to send reply
                }
            }
            _ => {
                // Unknown message type - ignore
            }
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("counter".to_string())
    }
}

impl Default for CounterActor {
    fn default() -> Self {
        Self::new()
    }
}

/// Worker actor pattern
///
/// A generic worker actor that processes tasks. Users provide a work function
/// that processes each message.
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_core::patterns::WorkerActor;
/// # use plexspaces_actor::ActorBuilder;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let worker = WorkerActor::new(|msg| {
///     println!("Processing: {:?}", msg);
///     Ok(())
/// });
///
/// let actor = ActorBuilder::new(Box::new(worker))
///     .with_name("worker")
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
pub struct WorkerActor {
    work_fn: Arc<dyn Fn(Message) -> Result<(), String> + Send + Sync>,
}

impl WorkerActor {
    /// Create a new worker actor with a work function
    pub fn new<F>(work_fn: F) -> Self
    where
        F: Fn(Message) -> Result<(), String> + Send + Sync + 'static,
    {
        Self {
            work_fn: Arc::new(work_fn),
        }
    }
}

#[async_trait]
impl Actor for WorkerActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        (self.work_fn)(msg).map_err(|e| BehaviorError::ProcessingError(e))?;
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("worker".to_string())
    }
}

/// Coordinator actor pattern
///
/// An actor that coordinates multiple workers. Useful for task distribution
/// and result aggregation.
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_core::patterns::CoordinatorActor;
/// # use plexspaces_actor::ActorBuilder;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let coordinator = CoordinatorActor::new();
///
/// let actor = ActorBuilder::new(Box::new(coordinator))
///     .with_name("coordinator")
///     .with_tuplespace()  // Coordinator often needs TupleSpace
///     .build()
///     .await;
/// # Ok(())
/// # }
/// ```
pub struct CoordinatorActor {
    workers: Arc<Mutex<Vec<String>>>, // Worker actor IDs
}

impl CoordinatorActor {
    /// Create a new coordinator actor
    pub fn new() -> Self {
        Self {
            workers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Register a worker actor
    pub fn register_worker(&self, worker_id: String) {
        let mut workers = self.workers.lock().unwrap();
        workers.push(worker_id);
    }

    /// Get all registered workers
    pub fn get_workers(&self) -> Vec<String> {
        self.workers.lock().unwrap().clone()
    }
}

#[async_trait]
impl Actor for CoordinatorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        match msg.message_type.as_str() {
            "register_worker" => {
                if let Ok(worker_id) = String::from_utf8(msg.payload) {
                    self.register_worker(worker_id);
                }
            }
            "get_workers" => {
                let _workers = self.get_workers();
                // Send reply using ActorService if sender is available
                // Note: ActorService is accessed via ServiceLocator, but we can't easily get it in core
                // due to circular dependencies. For now, reply sending is disabled in patterns.
                // Actors should handle replies directly using ctx.service_locator or ActorRef.
                if let Some(_sender_id) = msg.sender_id() {
                    // TODO: Implement reply sending via ServiceLocator or ActorRef
                    // For now, replies are not sent from patterns
                    // let reply_payload = serde_json::to_vec(&workers).unwrap_or_default();
                    // let reply = Message::new(reply_payload)
                    //     .with_message_type("workers".to_string());
                    // Use ActorRef or ActorService from ServiceLocator to send reply
                }
            }
            _ => {
                // Unknown message type - ignore
            }
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("coordinator".to_string())
    }
}

impl Default for CoordinatorActor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_actor_new() {
        let counter = CounterActor::new();
        assert_eq!(counter.get_count(), 0);
    }

    #[test]
    fn test_counter_actor_with_initial_value() {
        let counter = CounterActor::with_initial_value(10);
        assert_eq!(counter.get_count(), 10);
    }

    #[test]
    fn test_worker_actor_new() {
        let _worker = WorkerActor::new(|_msg| Ok(()));
    }

    #[test]
    fn test_coordinator_actor_new() {
        let coordinator = CoordinatorActor::new();
        assert_eq!(coordinator.get_workers().len(), 0);
    }

    #[test]
    fn test_coordinator_register_worker() {
        let coordinator = CoordinatorActor::new();
        coordinator.register_worker("worker1".to_string());
        coordinator.register_worker("worker2".to_string());
        assert_eq!(coordinator.get_workers().len(), 2);
    }
}

