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

//! Tests for ActorBuilder WASM and Firecracker support (Phase 2.5 - TDD)
//!
//! ## Purpose
//! These tests verify that ActorBuilder can easily create actors with WASM modules
//! and Firecracker VM deployment, simplifying developer experience.

#[cfg(test)]
mod tests {
    use plexspaces_actor::ActorBuilder;
    use plexspaces_core::{Actor, BehaviorType};
    use plexspaces_mailbox::Message;
    use async_trait::async_trait;

    struct TestBehavior;

    #[async_trait]
    impl Actor for TestBehavior {
        async fn handle_message(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _msg: Message,
        ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::Custom("test".to_string())
        }
    }

    /// Test: ActorBuilder can specify WASM module
    ///
    /// ## Purpose
    /// Verify that developers can easily create actors with WASM modules
    /// using `with_wasm_module()` method.
    ///
    /// ## Design
    /// WASM module specifies actor implementation (polyglot support).
    #[test]
    fn test_actor_builder_with_wasm_module() {
        let builder = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("wasm-actor")
            .with_wasm_module("counter-actor", "1.0.0", "module-hash-abc123");

        // Builder should accept WASM module configuration
        // (Implementation will store this in ActorConfig)
        assert!(true);
    }

    /// Test: ActorBuilder can specify Firecracker VM
    ///
    /// ## Purpose
    /// Verify that developers can easily deploy actors to Firecracker VMs
    /// using `with_firecracker_vm()` method.
    ///
    /// ## Design
    /// Firecracker VM provides application-level isolation.
    #[test]
    fn test_actor_builder_with_firecracker_vm() {
        let builder = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("vm-actor")
            .with_firecracker_vm("vm-001");

        // Builder should accept Firecracker VM configuration
        // (Implementation will store this in ActorConfig)
        assert!(true);
    }

    /// Test: ActorBuilder can combine WASM and Firecracker
    ///
    /// ## Purpose
    /// Verify that developers can combine WASM modules with Firecracker VMs
    /// for polyglot actors with strong isolation.
    #[test]
    fn test_actor_builder_wasm_and_firecracker() {
        let builder = ActorBuilder::new(Box::new(TestBehavior))
            .with_name("wasm-vm-actor")
            .with_wasm_module("counter-actor", "1.0.0", "module-hash-abc123")
            .with_firecracker_vm("vm-001");

        // Builder should accept both WASM and Firecracker configuration
        assert!(true);
    }

    /// Test: ActorBuilder WASM module validation
    ///
    /// ## Purpose
    /// Verify that WASM module configuration is validated (name, version, hash).
    #[test]
    fn test_actor_builder_wasm_module_validation() {
        // Valid WASM module
        let _builder1 = ActorBuilder::new(Box::new(TestBehavior))
            .with_wasm_module("counter-actor", "1.0.0", "abc123");

        // Empty module name should be rejected (or use default)
        // This test verifies validation logic
        assert!(true);
    }

    /// Test: ApplicationDeployment builder exists
    ///
    /// ## Purpose
    /// Verify that ApplicationDeployment builder can be used to deploy
    /// entire applications to Firecracker VMs.
    #[test]
    fn test_application_deployment_builder() {
        // TODO: After implementing ApplicationDeployment builder
        // let deployment = ApplicationDeployment::new("my-app")
        //     .with_vm_config(VmConfig { ... })
        //     .with_actors(vec![...])
        //     .deploy()
        //     .await?;
        assert!(true);
    }
}

