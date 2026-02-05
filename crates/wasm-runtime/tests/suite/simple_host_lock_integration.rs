// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for the simple host lock API (string-based WASM host interface).
// Validates lock-acquire (tenant-id, namespace, holder-id, lock-name) -> JSON,
// lock-release (lock-id, tenant-id, namespace, holder-id, lock-version),
// lock-renew (lock-id, tenant-id, namespace, holder-id, lock-version, lease-duration-secs).

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_core::ActorId;
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_wasm_runtime::simple_component_host::plexspaces::simple_actor::host::Host;
    use plexspaces_wasm_runtime::simple_component_host::SimpleHostImpl;
    use plexspaces_wasm_runtime::HostFunctions;
    use std::sync::Arc;

    async fn create_simple_host_with_lock_manager() -> SimpleHostImpl {
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = Arc::new(HostFunctions::new());
        SimpleHostImpl::with_services(
            actor_id,
            host_functions,
            None,
            Some(lock_manager),
            None,
        )
    }

    #[tokio::test]
    async fn test_simple_host_lock_acquire_returns_json() {
        let mut host = create_simple_host_with_lock_manager().await;
        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(
            !out.starts_with("ERROR:"),
            "lock_acquire should succeed, got: {}",
            out
        );
        let parsed: serde_json::Value =
            serde_json::from_str(&out).expect("lock_acquire must return valid JSON");
        assert_eq!(parsed["lock_key"], "my-lock");
        assert_eq!(parsed["holder_id"], "holder-1");
        assert_eq!(parsed["locked"], true);
        assert!(parsed["version"].as_str().unwrap().len() > 0);
        assert!(parsed["lease_duration_secs"].as_u64().unwrap() >= 30);
        assert!(parsed["expires_at_ms"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_simple_host_lock_release_success() {
        let mut host = create_simple_host_with_lock_manager().await;
        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(!out.starts_with("ERROR:"), "acquire failed: {}", out);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let lock_id = parsed["lock_key"].as_str().unwrap().to_string();
        let version = parsed["version"].as_str().unwrap().to_string();

        let release_out = host
            .lock_release(
                lock_id,
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                version,
            )
            .await;
        assert!(
            !release_out.starts_with("ERROR:"),
            "lock_release should succeed, got: {}",
            release_out
        );
        assert_eq!(release_out, "", "release should return empty string on success");
    }

    #[tokio::test]
    async fn test_simple_host_lock_renew_returns_new_version() {
        let mut host = create_simple_host_with_lock_manager().await;
        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(!out.starts_with("ERROR:"), "acquire failed: {}", out);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let lock_id = parsed["lock_key"].as_str().unwrap().to_string();
        let version = parsed["version"].as_str().unwrap().to_string();

        let renew_out = host
            .lock_renew(
                lock_id.clone(),
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                version.clone(),
                60,
            )
            .await;
        assert!(
            !renew_out.starts_with("ERROR:"),
            "lock_renew should succeed, got: {}",
            renew_out
        );
        assert_ne!(renew_out, version, "renew should return a new version");
        assert!(!renew_out.is_empty(), "renew should return non-empty version");

        // Release with the new version
        let release_out = host
            .lock_release(
                lock_id,
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                renew_out,
            )
            .await;
        assert_eq!(release_out, "", "release after renew should succeed");
    }

    #[tokio::test]
    async fn test_simple_host_lock_release_wrong_holder_fails() {
        let mut host = create_simple_host_with_lock_manager().await;
        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(!out.starts_with("ERROR:"), "acquire failed: {}", out);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let lock_id = parsed["lock_key"].as_str().unwrap().to_string();
        let version = parsed["version"].as_str().unwrap().to_string();

        let release_out = host
            .lock_release(
                lock_id,
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-2".to_string(), // wrong holder
                version,
            )
            .await;
        assert!(
            release_out.starts_with("ERROR:"),
            "release with wrong holder should fail, got: {}",
            release_out
        );
    }

    #[tokio::test]
    async fn test_simple_host_lock_renew_wrong_holder_fails() {
        let mut host = create_simple_host_with_lock_manager().await;
        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(!out.starts_with("ERROR:"), "acquire failed: {}", out);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let lock_id = parsed["lock_key"].as_str().unwrap().to_string();
        let version = parsed["version"].as_str().unwrap().to_string();

        let renew_out = host
            .lock_renew(
                lock_id,
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-2".to_string(), // wrong holder
                version,
                60,
            )
            .await;
        assert!(
            renew_out.starts_with("ERROR:"),
            "renew with wrong holder should fail, got: {}",
            renew_out
        );
    }

    #[tokio::test]
    async fn test_simple_host_lock_no_manager_returns_error() {
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = Arc::new(HostFunctions::new());
        let mut host = SimpleHostImpl::with_services(actor_id, host_functions, None, None, None);

        let out = host
            .lock_acquire(
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                "my-lock".to_string(),
                30,
                1000,
            )
            .await;
        assert!(
            out.starts_with("ERROR:"),
            "lock_acquire without LockManager should return ERROR, got: {}",
            out
        );
    }
}
