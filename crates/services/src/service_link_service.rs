// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// ServiceLinkService — gRPC service for runtime management of outbound service links.
//
// ## Purpose
// Allows adding, removing, and querying service links at runtime without restarting
// the node. When a link is added or removed, the OutboundHttpClient registered in
// ServiceLocator is rebuilt and re-registered.
//
// ## Architecture
// - Holds Arc<dyn ServiceLocator> for accessing/updating the OutboundHttpClient
// - Maintains its own RwLock<HashMap<String, ServiceLinkConfig>> for the live link catalog
// - On startup (from the gRPC server setup), seeds the catalog from RuntimeConfig.service_links

use plexspaces_actor::InitializableServiceLocator;
use plexspaces_common::RequestContext;
use plexspaces_proto::node::v1::{
    service_link_service_server::ServiceLinkService, AddServiceLinkRequest, AddServiceLinkResponse,
    GetServiceLinkRequest, GetServiceLinkResponse, ListServiceLinksRequest,
    ListServiceLinksResponse, RemoveServiceLinkRequest, RuntimeConfig, ServiceLinkConfig,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

/// gRPC implementation of ServiceLinkService.
#[derive(Clone)]
pub struct ServiceLinkServiceImpl {
    service_locator: Arc<dyn InitializableServiceLocator>,
    /// Live catalog of service links (may differ from RuntimeConfig if changed at runtime).
    links: Arc<RwLock<HashMap<String, ServiceLinkConfig>>>,
}

impl ServiceLinkServiceImpl {
    /// Create a new ServiceLinkServiceImpl.
    ///
    /// Seeds the in-memory link catalog from `RuntimeConfig.service_links` if a
    /// RuntimeConfig is already registered in the ServiceLocator.
    pub async fn new(service_locator: Arc<dyn InitializableServiceLocator>) -> Self {
        let mut initial: HashMap<String, ServiceLinkConfig> = HashMap::new();
        if let Some(rc) = service_locator.get_runtime_config().await {
            for link in rc.service_links {
                if !link.name.is_empty() {
                    initial.insert(link.name.clone(), link);
                }
            }
        }
        Self {
            service_locator,
            links: Arc::new(RwLock::new(initial)),
        }
    }

    /// Rebuild and re-register the OutboundHttpClient from the current link catalog.
    ///
    /// When the catalog is empty (last link removed), the existing client is explicitly
    /// unregistered by registering a no-op empty client, so callers get a clear
    /// "no client" signal on the next `http_fetch` call.
    async fn rebuild_client(&self) {
        let links: Vec<ServiceLinkConfig> = {
            let guard = self.links.read().await;
            guard.values().cloned().collect()
        };
        if links.is_empty() {
            // Unregister the stale client so callers get "unavailable" on the next call.
            self.service_locator.unregister_outbound_http_client().await;
            return;
        }
        let runtime = RuntimeConfig {
            service_links: links,
            ..Default::default()
        };
        match plexspaces_http_client::ResilientOutboundHttpClient::from_runtime_config(&runtime) {
            Ok(client) if !client.is_empty() => {
                self.service_locator
                    .register_outbound_http_client(Arc::new(client))
                    .await;
                tracing::info!("ServiceLinkService: rebuilt OutboundHttpClient");
            }
            Ok(_) => {
                tracing::warn!("ServiceLinkService: rebuilt client is empty (no HTTP links)");
            }
            Err(e) => {
                tracing::error!(
                    "ServiceLinkService: failed to rebuild OutboundHttpClient: {}",
                    e
                );
            }
        }
    }
}

#[async_trait::async_trait]
impl ServiceLinkService for ServiceLinkServiceImpl {
    async fn add_service_link(
        &self,
        request: Request<AddServiceLinkRequest>,
    ) -> Result<Response<AddServiceLinkResponse>, Status> {
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let metadata = request.metadata().clone();
        let _ctx = crate::request_context_from_grpc_request(
            &metadata,
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let link = request
            .into_inner()
            .link
            .ok_or_else(|| Status::invalid_argument("link is required"))?;
        if link.name.is_empty() {
            return Err(Status::invalid_argument("link.name must not be empty"));
        }
        if link.base_url.is_empty() {
            return Err(Status::invalid_argument("link.base_url must not be empty"));
        }
        {
            let mut guard = self.links.write().await;
            guard.insert(link.name.clone(), link.clone());
        }
        self.rebuild_client().await;
        tracing::info!(name = %link.name, "ServiceLinkService: added service link");
        Ok(Response::new(AddServiceLinkResponse { request_id: ulid::Ulid::new().to_string(), link: Some(link) }))
    }

    async fn remove_service_link(
        &self,
        request: Request<RemoveServiceLinkRequest>,
    ) -> Result<Response<()>, Status> {
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let metadata = request.metadata().clone();
        let _ctx = crate::request_context_from_grpc_request(
            &metadata,
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let name = request.into_inner().name;
        if name.is_empty() {
            return Err(Status::invalid_argument("name must not be empty"));
        }
        let removed = {
            let mut guard = self.links.write().await;
            guard.remove(&name).is_some()
        };
        if !removed {
            return Err(Status::not_found(format!(
                "service link '{}' not found",
                name
            )));
        }
        self.rebuild_client().await;
        tracing::info!(%name, "ServiceLinkService: removed service link");
        Ok(Response::new(()))
    }

    async fn get_service_link(
        &self,
        request: Request<GetServiceLinkRequest>,
    ) -> Result<Response<GetServiceLinkResponse>, Status> {
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let metadata = request.metadata().clone();
        let _ctx = crate::request_context_from_grpc_request(
            &metadata,
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let name = request.into_inner().name;
        let guard = self.links.read().await;
        match guard.get(&name) {
            Some(link) => Ok(Response::new(GetServiceLinkResponse {
                request_id: ulid::Ulid::new().to_string(),
                link: Some(link.clone()),
            })),
            None => Err(Status::not_found(format!(
                "service link '{}' not found",
                name
            ))),
        }
    }

    async fn list_service_links(
        &self,
        request: Request<ListServiceLinksRequest>,
    ) -> Result<Response<ListServiceLinksResponse>, Status> {
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let metadata = request.metadata().clone();
        let _ctx = crate::request_context_from_grpc_request(
            &metadata,
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let guard = self.links.read().await;
        let mut all: Vec<ServiceLinkConfig> = guard.values().cloned().collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));

        let page_size = if req.page_size > 0 {
            req.page_size as usize
        } else {
            usize::MAX
        };
        let start = if req.page_token.is_empty() {
            0usize
        } else {
            all.iter()
                .position(|l| l.name > req.page_token)
                .unwrap_or(all.len())
        };
        let page: Vec<ServiceLinkConfig> =
            all.iter().skip(start).take(page_size).cloned().collect();
        let next_page_token = if page.len() == page_size && start + page_size < all.len() {
            page.last().map(|l| l.name.clone()).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(Response::new(ListServiceLinksResponse {
            request_id: req.request_id.clone(),
            links: page,
            next_page_token,
        }))
    }
}

#[async_trait::async_trait]
impl plexspaces_service_traits::ServiceLinkAccess for ServiceLinkServiceImpl {
    async fn list_links(
        &self,
        _ctx: &RequestContext,
    ) -> Result<Vec<ServiceLinkConfig>, Box<dyn std::error::Error + Send + Sync>> {
        let guard = self.links.read().await;
        let mut links: Vec<ServiceLinkConfig> = guard.values().cloned().collect();
        links.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(links)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::node::v1::OutboundTransport;

    fn make_service_locator() -> Arc<dyn InitializableServiceLocator> {
        Arc::new(crate::service_locator::ServiceLocatorImpl::new())
    }

    /// Build a tonic Request with the auth env var set to disabled so that
    /// `request_context_from_grpc_request` extracts a valid anonymous context.
    /// Uses PLEXSPACES_DISABLE_AUTH=1 — the canonical pattern for unit tests in this codebase.
    fn authed_request<T>(inner: T) -> Request<T> {
        // SAFETY: unit tests run in a tokio runtime; env mutation is safe here as
        // these tests do not run in parallel (each is an isolated tokio::test task).
        unsafe {
            std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");
        }
        Request::new(inner)
    }

    fn weather_link(name: &str) -> ServiceLinkConfig {
        ServiceLinkConfig {
            name: name.to_string(),
            transport: OutboundTransport::OutboundTransportHttp as i32,
            base_url: format!("https://{}.example.com", name),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_add_and_get_service_link() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;

        let req = authed_request(AddServiceLinkRequest {
            link: Some(weather_link("payments-api")),
            request_id: ulid::Ulid::new().to_string(),
        });
        let resp = svc.add_service_link(req).await.unwrap();
        assert_eq!(resp.into_inner().link.unwrap().name, "payments-api");

        let get_req = authed_request(GetServiceLinkRequest {
            name: "payments-api".to_string(),
            request_id: ulid::Ulid::new().to_string(),
        });
        let get_resp = svc.get_service_link(get_req).await.unwrap();
        assert_eq!(
            get_resp.into_inner().link.unwrap().base_url,
            "https://payments-api.example.com"
        );
    }

    #[tokio::test]
    async fn test_remove_service_link() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;

        svc.add_service_link(authed_request(AddServiceLinkRequest {
            link: Some(weather_link("svc-a")),
            request_id: ulid::Ulid::new().to_string(),
        }))
        .await
        .unwrap();
        svc.remove_service_link(authed_request(RemoveServiceLinkRequest {
            name: "svc-a".to_string(),
            request_id: ulid::Ulid::new().to_string(),
        }))
        .await
        .unwrap();

        let err = svc
            .get_service_link(authed_request(GetServiceLinkRequest {
                name: "svc-a".to_string(),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_remove_unregisters_client_when_catalog_empty() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl.clone()).await;

        // Add a link so a client gets registered
        svc.add_service_link(authed_request(AddServiceLinkRequest {
            link: Some(weather_link("temp-link")),
            request_id: ulid::Ulid::new().to_string(),
        }))
        .await
        .unwrap();
        assert!(sl.get_outbound_http_client().await.is_some());

        // Remove it — client must be unregistered
        svc.remove_service_link(authed_request(RemoveServiceLinkRequest {
            name: "temp-link".to_string(),
            request_id: ulid::Ulid::new().to_string(),
        }))
        .await
        .unwrap();
        assert!(sl.get_outbound_http_client().await.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_returns_not_found() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;
        let err = svc
            .remove_service_link(authed_request(RemoveServiceLinkRequest {
                name: "no-such".to_string(),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_list_service_links() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;

        for name in &["svc-z", "svc-a", "svc-m"] {
            svc.add_service_link(authed_request(AddServiceLinkRequest {
                link: Some(weather_link(name)),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap();
        }

        let resp = svc
            .list_service_links(authed_request(ListServiceLinksRequest {
                page_size: 0,
                page_token: String::new(),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(resp.links.len(), 3);
        // Sorted by name
        assert_eq!(resp.links[0].name, "svc-a");
        assert_eq!(resp.links[1].name, "svc-m");
        assert_eq!(resp.links[2].name, "svc-z");
    }

    #[tokio::test]
    async fn test_list_pagination() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;

        for name in &["alpha", "beta", "gamma", "delta", "epsilon"] {
            svc.add_service_link(authed_request(AddServiceLinkRequest {
                link: Some(weather_link(name)),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap();
        }

        // Page 1: first 2 entries (sorted: alpha, beta, delta, epsilon, gamma)
        let page1 = svc
            .list_service_links(authed_request(ListServiceLinksRequest {
                page_size: 2,
                page_token: String::new(),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(page1.links.len(), 2);
        assert_eq!(page1.links[0].name, "alpha");
        assert_eq!(page1.links[1].name, "beta");
        assert!(!page1.next_page_token.is_empty());

        // Page 2: next 2 entries
        let page2 = svc
            .list_service_links(authed_request(ListServiceLinksRequest {
                page_size: 2,
                page_token: page1.next_page_token,
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(page2.links.len(), 2);
    }

    #[tokio::test]
    async fn test_add_invalid_link_missing_name() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;
        let err = svc
            .add_service_link(authed_request(AddServiceLinkRequest {
                link: Some(ServiceLinkConfig {
                    name: String::new(),
                    base_url: "https://example.com".to_string(),
                    ..Default::default()
                }),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_add_invalid_link_missing_base_url() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;
        let err = svc
            .add_service_link(authed_request(AddServiceLinkRequest {
                link: Some(ServiceLinkConfig {
                    name: "my-link".to_string(),
                    base_url: String::new(),
                    ..Default::default()
                }),
                request_id: ulid::Ulid::new().to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_add_no_link_returns_invalid_argument() {
        let sl = make_service_locator();
        let svc = ServiceLinkServiceImpl::new(sl).await;
        let err = svc
            .add_service_link(authed_request(AddServiceLinkRequest { link: None, request_id: ulid::Ulid::new().to_string() }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_seeded_from_runtime_config() {
        // Use ServiceLocatorImpl to register a RuntimeConfig with one link before creating the service.
        let sl = crate::service_locator::ServiceLocatorImpl::new();
        let rc = RuntimeConfig {
            service_links: vec![weather_link("pre-seeded")],
            ..Default::default()
        };
        sl.register_runtime_config(rc).await;
        let svc = ServiceLinkServiceImpl::new(Arc::new(sl)).await;

        let get = authed_request(GetServiceLinkRequest {
            name: "pre-seeded".to_string(),
            request_id: ulid::Ulid::new().to_string(),
        });
        let resp = svc.get_service_link(get).await.unwrap();
        assert_eq!(resp.into_inner().link.unwrap().name, "pre-seeded");
    }
}
