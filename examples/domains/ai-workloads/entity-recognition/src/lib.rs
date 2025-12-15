// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Entity Recognition AI Workload Example
//!
//! Demonstrates resource-aware scheduling with a realistic AI workload,
//! similar to Ray's entity recognition example.
//!
//! ## Architecture
//!
//! ```
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Application: Entity Recognition         │
//! │                                                             │
//! │  ┌──────────────┐     ┌──────────────┐     ┌─────────────┐ │
//! │  │  Loader      │────▶│  Processor   │────▶│  Aggregator │ │
//! │  │  (CPU-bound) │     │  (GPU-bound) │     │  (CPU-bound)│ │
//! │  └──────────────┘     └──────────────┘     └─────────────┘ │
//! │         │                    │                    │         │
//! │         ▼                    ▼                    ▼         │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │         Scheduling Layer (Resource-Aware)            │   │
//! │  │  - Loader → CPU nodes                               │   │
//! │  │  - Processor → GPU nodes                            │   │
//! │  │  - Aggregator → CPU nodes                           │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Components
//!
//! 1. **Loader Actor**: Reads documents, CPU-intensive
//!    - Resource requirements: CPU=2, Memory=1GB
//!    - Labels: `workload=cpu-intensive`, `service=loader`
//!    - Group: `entity-recognition-loaders`
//!
//! 2. **Processor Actor**: Runs LLM inference, GPU-intensive
//!    - Resource requirements: CPU=1, Memory=4GB, GPU=1
//!    - Labels: `workload=gpu-intensive`, `service=processor`
//!    - Group: `entity-recognition-processors`
//!
//! 3. **Aggregator Actor**: Aggregates results, CPU-intensive
//!    - Resource requirements: CPU=2, Memory=1GB
//!    - Labels: `workload=cpu-intensive`, `service=aggregator`
//!    - Group: `entity-recognition-aggregators`

pub mod config;
pub mod loader;
pub mod processor;
pub mod aggregator;
pub mod application;

pub use config::{EntityRecognitionConfig, AppConfig, BackendType, NodeConfig, ResourceConfig};
pub use loader::{LoaderBehavior, LoaderRequest, LoaderResponse};
pub use processor::{ProcessorBehavior, ProcessorRequest, ProcessorResponse, Entity};
pub use aggregator::{AggregatorBehavior, AggregatorRequest, AggregatorResponse};
pub use application::EntityRecognitionApplication;

