// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-actor crate
// All test files are included as modules to compile into a single binary

pub mod actor_builder_wasm_firecracker_tests;
pub mod actor_factory_impl_tests;
pub mod actor_factory_no_register_local_tests;
pub mod actor_lifecycle_comprehensive_tests;
pub mod actor_ref_service_locator_tests;
pub mod actor_state_conversion_tests;
pub mod integration_actor_ref_grpc_comprehensive_tests;
pub mod integration_temporary_sender_pattern_tests;
pub mod lifecycle_comprehensive_tests;
pub mod lifecycle_edge_cases_tests;
pub mod pipeline_actors_test;
pub mod registration_reliability_tests;
pub mod ttl_support_tests;
