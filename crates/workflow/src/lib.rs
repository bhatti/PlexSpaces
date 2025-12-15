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

//! # PlexSpaces Workflow Orchestration
//!
//! ## Purpose
//! Provides simple, durable workflow orchestration with Temporal/Step Functions-like patterns.
//! Workflows are actors with journaling enabled - no separate persistence needed.
//!
//! ## Architecture Context
//! This crate implements workflow orchestration as part of PlexSpaces core capabilities:
//! - **Workflows = Actors + DurabilityFacet**: Reuse existing actor infrastructure
//! - **Hybrid Storage**: Journal for state (recovery), DB for queries (monitoring)
//! - **Step Functions Patterns**: Map/reduce, parallel execution, conditional logic, retries
//! - **Saga Pattern**: Compensation with on_error handlers
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                  Workflow System                        │
//! ├─────────────────────────────────────────────────────────┤
//! │                                                         │
//! │  ┌──────────────────┐        ┌──────────────────┐      │
//! │  │ Workflow Metadata│◄───────│  gRPC Service    │      │
//! │  │   (Queryable)    │        │  (API Layer)     │      │
//! │  └────────┬─────────┘        └──────────────────┘      │
//! │           │                                             │
//! │           │  ┌──────────────────────────────────┐      │
//! │           ├─►│   Workflow Actor                 │      │
//! │           │  │   + DurabilityFacet              │      │
//! │           │  └────────────┬─────────────────────┘      │
//! │           │               │                             │
//! │           │               ▼                             │
//! │  ┌────────┴─────┐  ┌──────────────┐                   │
//! │  │ Workflow DB  │  │   Journal    │                   │
//! │  │ - definitions│  │ - entries    │                   │
//! │  │ - executions │  │ - snapshots  │                   │
//! │  │ (metadata)   │  │ (state)      │                   │
//! │  └──────────────┘  └──────────────┘                   │
//! │      Query          Source of Truth                    │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`WorkflowBehavior`]: GenServer implementation for workflow execution
//! - [`WorkflowService`]: gRPC service for workflow management
//! - [`storage`]: SQL migrations and metadata storage layer
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_core`]: Common types (ActorId, ActorRef, ActorContext)
//! - [`plexspaces_actor`]: Actor system and lifecycle
//! - [`plexspaces_behavior`]: GenServer trait for workflow behavior
//! - [`plexspaces_persistence`]: Journal storage for state durability
//! - [`plexspaces_proto`]: Proto definitions (workflow.proto)
//!
//! ## Examples
//!
//! ### Define Workflow in TOML
//! ```toml
//! [workflow]
//! id = "credit-approval"
//! name = "Credit Approval"
//! version = "1.0"
//! default_timeout = "1h"
//!
//! [[step]]
//! id = "check-credit"
//! type = "task"
//! actor = "CreditCheckWorker"
//! method = "check"
//! retry.max_attempts = 3
//! ```
//!
//! ### Start Workflow Execution
//! ```ignore
//! // This example shows the future API once WorkflowService is implemented
//! use plexspaces_workflow::*;
//! use serde_json::json;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create storage
//! let storage = WorkflowStorage::new_in_memory().await?;
//!
//! // Create and save workflow definition
//! let definition = WorkflowDefinition {
//!     id: "credit-approval".to_string(),
//!     name: "Credit Approval".to_string(),
//!     version: "1.0".to_string(),
//!     steps: vec![],
//!     ..Default::default()
//! };
//! storage.save_definition(&definition).await?;
//!
//! // Start workflow execution
//! let execution_id = WorkflowExecutor::start_execution(
//!     &storage,
//!     "credit-approval",
//!     "1.0",
//!     json!({"applicant_id": "APP-123", "loan_amount": 50000})
//! ).await?;
//!
//! // Query workflow status
//! let execution = storage.get_execution(&execution_id).await?;
//! println!("Status: {:?}", execution.status);
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All workflow data models and service contracts defined in `workflow.proto`:
//! - WorkflowDefinition: Versioned workflow template
//! - WorkflowExecution: Running instance metadata
//! - Step types: Task, Parallel, Map, Choice, Wait, Signal
//! - RetryConfig: Exponential backoff retry policy
//!
//! ### Hybrid Storage Architecture
//! - **Journal** (via DurabilityFacet): Source of truth for workflow state
//!   - All state changes journaled for replay and recovery
//!   - Deterministic execution on restart
//!   - Exactly-once semantics
//!
//! - **Workflow DB** (SQL): Queryable metadata for monitoring
//!   - Workflow definitions (versioned templates)
//!   - Workflow executions (status, current step, timestamps)
//!   - Step executions (history for debugging)
//!   - Labels for filtering and search
//!
//! ### Simplicity
//! Workflows are just actors with DurabilityFacet - no separate workflow engine needed:
//! - Reuse existing actor infrastructure (mailbox, supervision, journaling)
//! - Fewer moving parts = Higher performance
//! - Single source of truth for state (journal)
//!
//! ## Features
//!
//! ### Step Types
//! - **Task**: Execute actor method
//! - **Parallel**: Execute steps concurrently
//! - **Map**: Parallel iteration over collection (map/reduce)
//! - **Choice**: Conditional branching
//! - **Wait**: Delay execution
//! - **Signal**: Wait for external event (human-in-the-loop)
//!
//! ### Retry Policy
//! - Exponential backoff with configurable rate
//! - Error type matching (retry specific errors only)
//! - Per-step or workflow-level retry config
//!
//! ### Compensation (Saga Pattern)
//! - `on_error` field per step
//! - Execute compensation step on failure
//! - Enables distributed transactions with rollback
//!
//! ### Signals and Queries
//! - Send signals to running workflows (external events)
//! - Query workflow state without side effects
//! - Supports human-in-the-loop workflows
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-workflow
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-workflow
//! ```
//!
//! ## Performance Characteristics
//! - **Latency**: < 10ms for step transitions (local execution)
//! - **Throughput**: 10K+ concurrent workflows per node
//! - **Recovery**: < 1s for most workflows (with checkpoints)
//! - **Storage**: ~100 bytes per step execution in metadata DB
//!
//! ## Known Limitations
//! - Maximum workflow size: 10MB serialized state
//! - Maximum step count: 10,000 steps per workflow
//! - Signal timeout: 7 days maximum
//! - Parallel step limit: 1000 concurrent steps

#![warn(missing_docs)]
#![warn(clippy::all)]

// TDD: Modules enabled as we implement them
pub mod executor;
pub mod service;
pub mod storage;
pub mod types;
pub mod workflow_actor;

// Re-exports for convenience
pub use executor::WorkflowExecutor;
pub use service::WorkflowServiceImpl;
pub use storage::WorkflowStorage;
pub use types::*;
pub use workflow_actor::{WorkflowActor, WorkflowMessage, WorkflowResponse};
