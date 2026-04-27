// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Ergonomic outbound HTTP client for Rust actors using service links.
//
// ## Purpose
// Zero-boilerplate HTTP client backed by a named service link declared in RuntimeConfig.service_links.
// Actors call external services by link name; the framework handles retries, circuit breaking, and
// auth injection transparently.
//
// ## Usage
// ```rust,ignore
// // In actor init():
// let http = ServiceHttpClient::from_locator(ctx.service_locator.clone(), "payments-api").await?;
//
// // GET JSON:
// let balance: serde_json::Value = http.get_json("/v1/balance?account=123").await?;
//
// // POST JSON:
// let result: serde_json::Value = http.post_json("/v1/transfer", &json!({"amount": 100})).await?;
// ```

#[cfg(feature = "native")]
use plexspaces_core::{
    OutboundHttpClient, OutboundHttpRequest, OutboundHttpResponse, ServiceLocator,
};
#[cfg(feature = "native")]
use serde::{de::DeserializeOwned, Serialize};
#[cfg(feature = "native")]
use std::sync::Arc;

/// Error type for service HTTP client operations.
#[derive(Debug, thiserror::Error)]
pub enum ServiceHttpClientError {
    /// The named service link is not configured in RuntimeConfig.service_links.
    #[error("service link '{0}' not configured (add it to RuntimeConfig.service_links)")]
    LinkNotConfigured(String),

    /// The outbound HTTP client is not registered (no service_links in RuntimeConfig).
    #[error("outbound HTTP client not available (no service_links configured)")]
    ClientNotAvailable,

    /// The HTTP request failed (network, circuit open, timeout).
    #[error("HTTP request to link '{link}' failed: {message}")]
    RequestFailed { link: String, message: String },

    /// The response body could not be deserialized as JSON.
    #[error("failed to deserialize response from link '{link}': {source}")]
    DeserializationError {
        link: String,
        #[source]
        source: serde_json::Error,
    },

    /// The request body could not be serialized as JSON.
    #[error("failed to serialize request body: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// The server returned a non-2xx status code.
    #[error("HTTP {status} from link '{link}': {body}")]
    HttpError {
        link: String,
        status: u16,
        body: String,
    },
}

/// Ergonomic outbound HTTP client backed by a named service link.
///
/// ## Architecture Context
/// - `link_name` must match a `ServiceLinkConfig.name` in `RuntimeConfig.service_links`.
/// - The underlying `OutboundHttpClient` handles retries, circuit breaking, timeouts.
/// - Auth headers declared in `ServiceLinkConfig` are injected transparently.
///
/// ## Example
/// ```rust,ignore
/// let http = ServiceHttpClient::from_locator(service_locator, "my-api").await?;
/// let data: serde_json::Value = http.get_json("/v1/items").await?;
/// ```
#[cfg(feature = "native")]
pub struct ServiceHttpClient {
    link_name: String,
    client: Arc<dyn OutboundHttpClient>,
}

#[cfg(feature = "native")]
impl ServiceHttpClient {
    /// Build a client from `ServiceLocator`, resolving the outbound HTTP client by link name.
    ///
    /// Returns `Err(ServiceHttpClientError::ClientNotAvailable)` if no outbound HTTP client
    /// is registered (i.e., `RuntimeConfig.service_links` is empty).
    pub async fn from_locator(
        service_locator: Arc<dyn ServiceLocator>,
        link_name: impl Into<String>,
    ) -> Result<Self, ServiceHttpClientError> {
        let client = service_locator
            .get_outbound_http_client()
            .await
            .ok_or(ServiceHttpClientError::ClientNotAvailable)?;
        Ok(Self {
            link_name: link_name.into(),
            client,
        })
    }

    /// Build a client from an `ActorContext`, resolving the outbound HTTP client by link name.
    ///
    /// Convenience wrapper around `from_locator` that uses `ctx.service_locator`.
    pub async fn from_context(
        ctx: &plexspaces_core::ActorContext,
        link_name: impl Into<String>,
    ) -> Result<Self, ServiceHttpClientError> {
        Self::from_locator(ctx.service_locator.clone(), link_name).await
    }

    /// GET JSON from the service link.
    ///
    /// Sends a GET request to `path_and_query`, deserializes the response body as JSON.
    ///
    /// ## Errors
    /// - `HttpError` if status is not 2xx.
    /// - `DeserializationError` if the response body is not valid JSON of type `T`.
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path_and_query: &str,
    ) -> Result<T, ServiceHttpClientError> {
        let req = OutboundHttpRequest {
            method: "GET".to_string(),
            path_and_query: path_and_query.to_string(),
            headers: vec![("Accept".to_string(), "application/json".to_string())],
            body: vec![],
        };
        let resp = self.execute(req).await?;
        self.parse_json_response(resp)
    }

    /// POST JSON to the service link.
    ///
    /// Serializes `body` as JSON, sends a POST request, deserializes the response body as JSON.
    ///
    /// ## Errors
    /// - `SerializationError` if `body` cannot be serialized.
    /// - `HttpError` if status is not 2xx.
    /// - `DeserializationError` if the response body is not valid JSON of type `T`.
    pub async fn post_json<B, T>(
        &self,
        path_and_query: &str,
        body: &B,
    ) -> Result<T, ServiceHttpClientError>
    where
        B: Serialize,
        T: DeserializeOwned,
    {
        let body_bytes = serde_json::to_vec(body)?;
        let req = OutboundHttpRequest {
            method: "POST".to_string(),
            path_and_query: path_and_query.to_string(),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            body: body_bytes,
        };
        let resp = self.execute(req).await?;
        self.parse_json_response(resp)
    }

    /// PUT JSON to the service link.
    ///
    /// Serializes `body` as JSON, sends a PUT request, deserializes the response body as JSON.
    pub async fn put_json<B, T>(
        &self,
        path_and_query: &str,
        body: &B,
    ) -> Result<T, ServiceHttpClientError>
    where
        B: Serialize,
        T: DeserializeOwned,
    {
        let body_bytes = serde_json::to_vec(body)?;
        let req = OutboundHttpRequest {
            method: "PUT".to_string(),
            path_and_query: path_and_query.to_string(),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "application/json".to_string()),
            ],
            body: body_bytes,
        };
        let resp = self.execute(req).await?;
        self.parse_json_response(resp)
    }

    /// DELETE request to the service link.
    ///
    /// Returns the response body as a string (may be empty).
    pub async fn delete(&self, path_and_query: &str) -> Result<String, ServiceHttpClientError> {
        let req = OutboundHttpRequest {
            method: "DELETE".to_string(),
            path_and_query: path_and_query.to_string(),
            headers: vec![],
            body: vec![],
        };
        let resp = self.execute(req).await?;
        Ok(String::from_utf8_lossy(&resp.body).into_owned())
    }

    /// Execute a raw HTTP request via the service link.
    ///
    /// ## Errors
    /// - `RequestFailed` if the request fails at the transport level.
    /// - `HttpError` if status is not 2xx.
    pub async fn execute(
        &self,
        req: OutboundHttpRequest,
    ) -> Result<OutboundHttpResponse, ServiceHttpClientError> {
        self.client
            .execute(&self.link_name, req)
            .await
            .map_err(|e| ServiceHttpClientError::RequestFailed {
                link: self.link_name.clone(),
                message: e.to_string(),
            })
            .and_then(|resp| {
                if resp.status >= 200 && resp.status < 300 {
                    Ok(resp)
                } else {
                    let body = String::from_utf8_lossy(&resp.body).into_owned();
                    Err(ServiceHttpClientError::HttpError {
                        link: self.link_name.clone(),
                        status: resp.status,
                        body,
                    })
                }
            })
    }

    /// Parse JSON from a response body.
    fn parse_json_response<T: DeserializeOwned>(
        &self,
        resp: OutboundHttpResponse,
    ) -> Result<T, ServiceHttpClientError> {
        serde_json::from_slice(&resp.body).map_err(|e| {
            ServiceHttpClientError::DeserializationError {
                link: self.link_name.clone(),
                source: e,
            }
        })
    }
}

#[cfg(test)]
#[cfg(feature = "native")]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_core::{OutboundHttpClientError, OutboundHttpRequest, OutboundHttpResponse};
    use std::sync::Arc;

    /// Mock HTTP client for testing.
    struct MockHttpClient {
        response: OutboundHttpResponse,
    }

    #[async_trait]
    impl OutboundHttpClient for MockHttpClient {
        async fn execute(
            &self,
            _link_name: &str,
            _request: OutboundHttpRequest,
        ) -> Result<OutboundHttpResponse, OutboundHttpClientError> {
            Ok(self.response.clone())
        }
    }

    fn mock_client(status: u16, body: &str) -> Arc<dyn OutboundHttpClient> {
        Arc::new(MockHttpClient {
            response: OutboundHttpResponse {
                status,
                headers: vec![("Content-Type".to_string(), "application/json".to_string())],
                body: body.as_bytes().to_vec(),
            },
        })
    }

    fn make_client(link_name: &str, http_client: Arc<dyn OutboundHttpClient>) -> ServiceHttpClient {
        ServiceHttpClient {
            link_name: link_name.to_string(),
            client: http_client,
        }
    }

    #[tokio::test]
    async fn test_get_json_success() {
        let client = make_client("test-api", mock_client(200, r#"{"value": 42}"#));
        let result: serde_json::Value = client.get_json("/v1/items").await.unwrap();
        assert_eq!(result["value"], 42);
    }

    #[tokio::test]
    async fn test_post_json_success() {
        let client = make_client("test-api", mock_client(200, r#"{"id": "abc"}"#));
        let body = serde_json::json!({ "name": "test" });
        let result: serde_json::Value = client.post_json("/v1/items", &body).await.unwrap();
        assert_eq!(result["id"], "abc");
    }

    #[tokio::test]
    async fn test_http_error_non_2xx() {
        let client = make_client("test-api", mock_client(404, r#"{"error": "not found"}"#));
        let result: Result<serde_json::Value, _> = client.get_json("/v1/items/999").await;
        match result {
            Err(ServiceHttpClientError::HttpError { status, .. }) => assert_eq!(status, 404),
            other => panic!("Expected HttpError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_delete_success() {
        let client = make_client("test-api", mock_client(204, ""));
        let result = client.delete("/v1/items/1").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_put_json_success() {
        let client = make_client("test-api", mock_client(200, r#"{"updated": true}"#));
        let body = serde_json::json!({ "field": "value" });
        let result: serde_json::Value = client.put_json("/v1/items/1", &body).await.unwrap();
        assert_eq!(result["updated"], true);
    }

    #[tokio::test]
    async fn test_client_not_available_error() {
        // Test that ClientNotAvailable is properly formatted
        let err = ServiceHttpClientError::ClientNotAvailable;
        assert!(err
            .to_string()
            .contains("outbound HTTP client not available"));
    }
}
