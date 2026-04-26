// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-node crate
// All test files are included as modules to compile into a single binary

// Shared test helpers (must be first for other modules to use)
pub mod test_helpers;

// Test modules
pub mod actor_context_integration;
pub mod application_deployment_supervisor_tests;
pub mod application_facet_comprehensive_tests;
pub mod application_facet_lifecycle_tests;
pub mod config_loader_tests;
pub mod distributed_barrier_test;
pub mod distributed_supervision;
pub mod distributed_tuplespace_test;
pub mod facet_tests;
pub mod firecracker_integration_test;
pub mod firecracker_service_test;
pub mod get_or_activate_tests;
pub mod grpc_tests;
pub mod http_wasm_deployment;
pub mod initialization_tests;
pub mod integration_health_metrics_tests;
pub mod lifecycle_observability;
pub mod link_semantics;
pub mod monitor_link_multinode_tests;
pub mod node_service_locator_tests;
pub mod remote_spawn_test;
pub mod service_wrappers_tests;
pub mod supervisor_tree_spawning_tests;
pub mod system_service_test;
pub mod tuplespace_service_test;
pub mod virtual_actor_comprehensive_tests;
pub mod virtual_actor_named_definition_tests;
pub mod virtual_actor_get_or_activate_tests;
pub mod virtual_actor_supervisor_hierarchy_tests;
pub mod virtual_actor_suspension_tests;
pub mod wasm_app_facet_toml_integration;
pub mod wasm_application_integration;
pub mod wasm_application_spec_integration_tests;
pub mod wasm_apps_loader_tests;
pub mod wasm_ask_message_id_tests;
pub mod wasm_checkpoint_durability_tests;
pub mod wasm_component_deployment_integration;
