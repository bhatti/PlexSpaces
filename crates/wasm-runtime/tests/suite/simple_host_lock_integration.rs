// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for the actor-world lock API.
// Validates protobuf lock-acquire/renew responses and result-based errors.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_actor::ActorId;
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_proto::locks::prv::Lock as ProtoLock;
    use plexspaces_wasm_runtime::simple_component_host::plexspaces::actor::host::Host;
    use plexspaces_wasm_runtime::simple_component_host::SimpleHostImpl;
    use plexspaces_wasm_runtime::HostFunctions;
    use prost::Message as _;
    use std::sync::Arc;

    async fn create_simple_host_with_lock_manager() -> SimpleHostImpl {
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let actor_id = ActorId::new("test-actor", "test-type", "test-ns", "test-node").unwrap();
        let host_functions = Arc::new(HostFunctions::new());
        SimpleHostImpl::with_services(actor_id, host_functions, None, Some(lock_manager), None)
    }

    #[tokio::test]
    async fn test_simple_host_lock_acquire_returns_protobuf() {
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
            .await
            .expect("lock_acquire should succeed");
        let parsed = ProtoLock::decode(out.as_slice()).expect("valid protobuf lock expected");
        assert_eq!(parsed.lock_key, "my-lock");
        assert_eq!(parsed.holder_id, "holder-1");
        assert!(parsed.locked);
        assert!(!parsed.version.is_empty());
        assert!(parsed.lease_duration_secs >= 30);
        assert!(parsed.expires_at.is_some());
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
            .await
            .expect("acquire should succeed");
        let parsed = ProtoLock::decode(out.as_slice()).expect("valid protobuf lock expected");
        let lock_id = parsed.lock_key.clone();
        let version = parsed.version.clone();

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
            release_out.is_ok(),
            "lock_release should succeed: {release_out:?}"
        );
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
            .await
            .expect("acquire should succeed");
        let parsed = ProtoLock::decode(out.as_slice()).expect("valid protobuf lock expected");
        let lock_id = parsed.lock_key.clone();
        let version = parsed.version.clone();

        let renew_out = host
            .lock_renew(
                lock_id.clone(),
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                version.clone(),
                60,
            )
            .await
            .expect("lock_renew should succeed");
        let renewed =
            ProtoLock::decode(renew_out.as_slice()).expect("valid protobuf lock expected");
        assert_ne!(
            renewed.version, version,
            "renew should return a new version"
        );
        assert!(
            !renewed.version.is_empty(),
            "renew should return non-empty version"
        );

        // Release with the new version
        let release_out = host
            .lock_release(
                lock_id,
                "tenant1".to_string(),
                "ns1".to_string(),
                "holder-1".to_string(),
                renewed.version,
            )
            .await;
        assert!(
            release_out.is_ok(),
            "release after renew should succeed: {release_out:?}"
        );
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
            .await
            .expect("acquire should succeed");
        let parsed = ProtoLock::decode(out.as_slice()).expect("valid protobuf lock expected");
        let lock_id = parsed.lock_key.clone();
        let version = parsed.version.clone();

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
            release_out.is_err(),
            "release with wrong holder should fail"
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
            .await
            .expect("acquire should succeed");
        let parsed = ProtoLock::decode(out.as_slice()).expect("valid protobuf lock expected");
        let lock_id = parsed.lock_key.clone();
        let version = parsed.version.clone();

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
        assert!(renew_out.is_err(), "renew with wrong holder should fail");
    }

    #[tokio::test]
    async fn test_simple_host_lock_no_manager_returns_error() {
        let actor_id = ActorId::new("test-actor", "test-type", "test-ns", "test-node").unwrap();
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
        assert!(out.is_err(), "lock_acquire without LockManager should fail");
    }
}
