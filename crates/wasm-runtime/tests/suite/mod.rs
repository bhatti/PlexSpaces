// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-wasm-runtime crate

pub mod behavior_routing_tests;
pub mod blob_host_functions_integration;
pub mod simple_host_lock_integration;
pub mod channel_host_functions_tests;
pub mod component_host_functions_tests;
pub mod comprehensive_coverage_tests;
pub mod comprehensive_host_functions_integration;
pub mod durability_host_functions_integration;
pub mod grpc_integration;
pub mod integration_tests;
pub mod messaging_host_functions_integration;
pub mod new_host_functions_integration;
pub mod shared_wasm_module;
pub mod tuplespace_host_functions_tests;
pub mod wasm_actor_implementation_tests;
pub mod wasm_component_integration;
pub mod wasm_state_persistence_tests;
