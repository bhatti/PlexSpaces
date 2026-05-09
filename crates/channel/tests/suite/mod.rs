// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated test suite for plexspaces-channel crate


pub mod ack_nack_integration_test;
pub mod backpressure_test;
pub mod channel_service_integration_tests;
pub mod kafka_integration_test;
pub mod nats_integration_test;
pub mod process_group_integration_test;
pub mod redis_integration_test;
pub mod shutdown_restart_test;
pub mod sqlite_shutdown_resume_test;
pub mod sqs_integration;
pub mod udp_integration_test;
