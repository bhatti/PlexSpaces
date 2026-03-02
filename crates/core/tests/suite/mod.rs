// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-core crate
// All test files are included as modules to compile into a single binary

pub mod actor_context_direct_service_tests;
pub mod actor_context_methods_tests;
pub mod actor_context_services_tests;
pub mod actor_context_tests;
pub mod actor_error_comprehensive_tests;
pub mod actor_ref_tests;
pub mod actor_registry_no_mailbox_exposure_tests;
pub mod actor_registry_parent_child_tests;
pub mod behavior_context_tests;
pub mod channel_service_tests;
pub mod exit_reason_tests;
pub mod link_monitor_comprehensive_tests;
pub mod object_registry_helpers_integration_tests;
pub mod process_group_service_tests;
pub mod reply_waiter_tests;
pub mod request_context_propagation_tests;
pub mod service_wrappers_tests;
pub mod stub_services_tests;
pub mod virtual_actor_lru_eviction_tests;