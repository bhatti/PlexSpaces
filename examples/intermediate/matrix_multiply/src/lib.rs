// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Matrix Multiplication Example - Library
//!
//! Exposes modules for testing and external use

pub mod matrix;
pub mod master_behavior;
pub mod worker_behavior;
pub mod config;

pub use config::MatrixMultiplyConfig;
pub use master_behavior::{MasterBehavior, MasterMessage};
pub use worker_behavior::{WorkerBehavior, WorkerMessage};
