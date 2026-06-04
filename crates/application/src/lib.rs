// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! # PlexSpaces Application Crate
//!
//! ## Purpose
//! Application lifecycle management for PlexSpaces (Erlang/OTP-inspired).
//! Provides ApplicationManager, ApplicationController, and SupervisorBuilder.
//!
//! ## Architecture Context
//! Applications are the fundamental unit of code organization in PlexSpaces:
//! - Library applications provide reusable modules (no processes)
//! - Active applications have supervision trees and long-running processes
//! - Applications have dependencies (topological startup order)
//! - Applications have lifecycle hooks (start, stop)
//!
//! ## Design Notes
//! - Application trait is defined in `plexspaces-core` (core types)
//! - This crate provides the management and controller implementations
//! - Uses proto-generated types from `plexspaces_proto`

// Application trait is defined in this crate (application_trait.rs)

pub mod application_trait;
pub use application_trait::{Application, ApplicationError, ApplicationNode};

// Application management modules
pub mod application_manager;
pub use application_manager::ApplicationManagerImpl;

// Type alias for convenience (ApplicationManagerImpl implements ApplicationManager trait)
pub type ApplicationManager = ApplicationManagerImpl;
pub mod controller;

// Re-export main types (ApplicationManager is the type alias above)
pub use controller::ApplicationController;


pub mod child_spec_util;

// Application implementations
pub mod application_impl;
pub mod application_manager_ext;
pub mod service_wrappers;
pub mod wasm_application;
pub mod wasm_message_sender;

// Re-export application implementations
pub use application_impl::SpecApplication;
pub use application_manager_ext::ApplicationManagerExt;
pub use wasm_application::WasmApplication;

// Re-export proto types for convenience
pub use plexspaces_proto::application::v1::{
    ApplicationRuntimeState, ApplicationSpec, ApplicationStatus, ApplicationType,
};
pub use plexspaces_proto::v1::application::{
    ApplicationState, ApplicationStatistics, HealthStatus, ShutdownStrategy,
};

// ApplicationManager is NOT registered in ServiceLocator.
// It is managed directly by the application crate and accessed through Node or other application-specific APIs.
