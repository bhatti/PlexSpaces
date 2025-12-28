// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Integration tests for messaging host functions (link, unlink, monitor, demonitor)
//
// NOTE: These tests are designed to run offline without network access or SSL.
// All tests use in-memory mocks and do not require external services.

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::MessagingImpl;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::messaging::Host;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::types as actor_types;
    use plexspaces_core::ActorId;
    use plexspaces_wasm_runtime::HostFunctions;
    use std::sync::Arc;
    use async_trait::async_trait;
    use plexspaces_wasm_runtime::MessageSender;

    /// Mock MessageSender for testing
    struct MockMessageSender {
        link_calls: Arc<tokio::sync::Mutex<Vec<(String, String, String)>>>,
        unlink_calls: Arc<tokio::sync::Mutex<Vec<(String, String, String)>>>,
        monitor_calls: Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
        monitor_refs: Arc<tokio::sync::Mutex<std::collections::HashMap<(String, String), u64>>>,
    }

    impl MockMessageSender {
        fn new() -> Self {
            Self {
                link_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
                unlink_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
                monitor_calls: Arc::new(tokio::sync::Mutex::new(Vec::new())),
                monitor_refs: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl MessageSender for MockMessageSender {
        async fn send_message(&self, _from: &str, _to: &str, _message: &str) -> Result<(), String> {
            Ok(())
        }

        async fn ask(
            &self,
            _from: &str,
            _to: &str,
            _message_type: &str,
            _payload: Vec<u8>,
            _timeout_ms: u64,
        ) -> Result<Vec<u8>, String> {
            Ok(vec![])
        }

        async fn spawn_actor(
            &self,
            _from: &str,
            _module_ref: &str,
            _initial_state: Vec<u8>,
            _actor_id: Option<String>,
            _labels: Vec<(String, String)>,
            _durable: bool,
        ) -> Result<String, String> {
            Ok("spawned-actor".to_string())
        }

        async fn stop_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _timeout_ms: u64,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn link_actor(
            &self,
            from: &str,
            actor_id: &str,
            linked_actor_id: &str,
        ) -> Result<(), String> {
            let mut calls = self.link_calls.lock().await;
            calls.push((from.to_string(), actor_id.to_string(), linked_actor_id.to_string()));
            Ok(())
        }

        async fn unlink_actor(
            &self,
            from: &str,
            actor_id: &str,
            linked_actor_id: &str,
        ) -> Result<(), String> {
            let mut calls = self.unlink_calls.lock().await;
            calls.push((from.to_string(), actor_id.to_string(), linked_actor_id.to_string()));
            Ok(())
        }

        async fn monitor_actor(
            &self,
            from: &str,
            actor_id: &str,
        ) -> Result<u64, String> {
            let mut calls = self.monitor_calls.lock().await;
            calls.push((from.to_string(), actor_id.to_string()));
            
            let monitor_ref = (calls.len() as u64) * 1000;
            let mut refs = self.monitor_refs.lock().await;
            refs.insert((from.to_string(), actor_id.to_string()), monitor_ref);
            
            Ok(monitor_ref)
        }

        async fn demonitor_actor(
            &self,
            _from: &str,
            _actor_id: &str,
            _monitor_ref: u64,
        ) -> Result<(), String> {
            // Mock implementation - returns error as documented
            Err("demonitor_actor requires monitor_ref string mapping - not yet fully implemented".to_string())
        }
    }


    #[tokio::test]
    async fn test_messaging_impl_link() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mock_sender = Arc::new(MockMessageSender::new());
        let host_functions = Arc::new(HostFunctions::with_message_sender(mock_sender.clone()));
        
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Link two actors
        let result = messaging.link("target-actor".to_string()).await;

        // ASSERT
        assert!(result.is_ok(), "link should succeed");
        
        // Verify that link_actor was called
        let calls = mock_sender.link_calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test-actor");
        assert_eq!(calls[0].1, "test-actor");
        assert_eq!(calls[0].2, "target-actor");
    }

    #[tokio::test]
    async fn test_messaging_impl_unlink() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mock_sender = Arc::new(MockMessageSender::new());
        let host_functions = Arc::new(HostFunctions::with_message_sender(mock_sender.clone()));
        
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Unlink two actors
        let result = messaging.unlink("target-actor".to_string()).await;

        // ASSERT
        assert!(result.is_ok(), "unlink should succeed");
        
        // Verify that unlink_actor was called
        let calls = mock_sender.unlink_calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test-actor");
        assert_eq!(calls[0].1, "test-actor");
        assert_eq!(calls[0].2, "target-actor");
    }

    #[tokio::test]
    async fn test_messaging_impl_monitor() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mock_sender = Arc::new(MockMessageSender::new());
        let host_functions = Arc::new(HostFunctions::with_message_sender(mock_sender.clone()));
        
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Monitor an actor
        let result = messaging.monitor("target-actor".to_string()).await;

        // ASSERT
        assert!(result.is_ok(), "monitor should succeed");
        let monitor_ref = result.unwrap();
        assert!(monitor_ref > 0, "monitor_ref should be non-zero");
        
        // Verify that monitor_actor was called
        let calls = mock_sender.monitor_calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "test-actor");
        assert_eq!(calls[0].1, "target-actor");
    }

    #[tokio::test]
    async fn test_messaging_impl_demonitor_not_implemented() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mock_sender = Arc::new(MockMessageSender::new());
        let host_functions = Arc::new(HostFunctions::with_message_sender(mock_sender.clone()));
        
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Demonitor with non-existent monitor_ref (should return ActorNotFound error)
        let result = messaging.demonitor(12345).await;

        // ASSERT
        assert!(result.is_err(), "demonitor should return error for non-existent monitor_ref");
        let error = result.unwrap_err();
        assert_eq!(error.code, actor_types::ErrorCode::ActorNotFound);
    }

    #[tokio::test]
    async fn test_messaging_impl_link_no_message_sender() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = Arc::new(HostFunctions::new()); // No message sender
        
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Try to link (should fail)
        let result = messaging.link("target-actor".to_string()).await;

        // ASSERT
        assert!(result.is_err(), "link should fail when message sender not configured");
        let error = result.unwrap_err();
        assert_eq!(error.code, actor_types::ErrorCode::Internal);
        assert!(error.message.contains("not configured"));
    }
}

