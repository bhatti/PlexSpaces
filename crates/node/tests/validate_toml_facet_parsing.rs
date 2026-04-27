// SPDX-License-Identifier: AGPL-3.0-or-later
// Standalone validation test for TOML facet parsing
// This test can run independently to verify TOML parsing works

#[cfg(test)]
mod validate_toml_parsing {
    use plexspaces_node::wasm_apps_loader::parse_app_config_toml;

    #[test]
    fn validate_task_queue_config_parsing() {
        let toml_config = include_str!("../../../examples/python/apps/task-queue/app-config.toml");

        // Parse the actual config file
        let result = parse_app_config_toml(toml_config, "task-queue");
        assert!(
            result.is_ok(),
            "Should parse TOML config successfully: {:?}",
            result.err()
        );

        let spec = result.unwrap();

        // Verify supervisor exists
        assert!(spec.supervisor.is_some(), "Supervisor should be present");
        let supervisor = spec.supervisor.unwrap();

        // Verify children
        assert_eq!(supervisor.children.len(), 1, "Should have 1 child");
        let child = &supervisor.children[0];
        let idn = child.actor_identity.as_ref().expect("actor_identity");
        assert_eq!(idn.name, "task-queue");
        assert_eq!(idn.actor_type, "task_queue_worker");

        // Verify facets were parsed
        assert_eq!(
            child.facets.len(),
            1,
            "Should have 1 facet parsed from TOML"
        );
        assert_eq!(
            child.facets[0].r#type, "locks",
            "Facet type should be 'locks'"
        );
        assert_eq!(child.facets[0].priority, 50, "Facet priority should be 50");

        println!("✅ TOML config parsed successfully");
        println!("   - Supervisor: {:?} children", supervisor.children.len());
        println!("   - Child: {} ({})", idn.name, idn.actor_type);
        println!("   - Facets: {} facet(s)", child.facets.len());
        for (i, facet) in child.facets.iter().enumerate() {
            println!(
                "     Facet {}: type={}, priority={}",
                i, facet.r#type, facet.priority
            );
        }
    }

    #[test]
    fn validate_behavior_kind_parsing() {
        let toml_config = r#"
[supervisor]
strategy = "one_for_one"
max_restarts = 10
max_restart_window_seconds = 60

[[supervisor.children]]
name = "order-fulfillment"
actor_type = "order_fulfillment_worker"
role = "worker"
restart = "permanent"
shutdown_timeout_seconds = 10
behavior_kind = "Workflow"
facets = [
  { type = "durability", priority = 90, config = {} }
]
"#;
        let result = parse_app_config_toml(toml_config, "temporal-order");
        assert!(
            result.is_ok(),
            "Should parse TOML with behavior_kind: {:?}",
            result.err()
        );
        let spec = result.unwrap();
        assert!(spec.supervisor.is_some());
        let child = &spec.supervisor.as_ref().unwrap().children[0];
        let idn = child.actor_identity.as_ref().expect("actor_identity");
        assert_eq!(idn.name, "order-fulfillment");
        assert_eq!(idn.actor_type, "order_fulfillment_worker");
        assert_eq!(child.behavior_kind.as_deref(), Some("Workflow"));
    }
}
