// SPDX-License-Identifier: AGPL-3.0-or-later

//! Resilient outbound HTTP client: circuit breaker, retries, metrics, tracing.

use crate::error::OutboundHttpClientError;
use crate::policy::{circuit_breaker_for_link, effective_max_attempts, resolve_policy_for_link};
use crate::retry::{backoff_duration_for_attempt, method_allows_retry, status_is_retriable};
use async_trait::async_trait;
use plexspaces_actor::{
    HttpHeader, OutboundHttpClient, OutboundHttpClientError as CoreOutboundError,
    OutboundHttpRequest, OutboundHttpResponse,
};
use plexspaces_circuit_breaker::CircuitBreaker;
use plexspaces_proto::node::v1::{OutboundTransport, RuntimeConfig, ServiceLinkConfig};
use rand::thread_rng;
use reqwest::redirect::Policy;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{field::Empty, Instrument};

/// Maximum response body size (16 MiB) to bound memory from untrusted peers.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Per-link resolved configuration and breaker.
struct ResolvedLink {
    base_url: String,
    policy: plexspaces_proto::node::v1::ClientTransportPolicy,
    reqwest: reqwest::Client,
    breaker: Arc<CircuitBreaker>,
    extra_headers: Vec<HttpHeader>,
}

/// Outbound HTTP client built from [`RuntimeConfig`](plexspaces_proto::node::v1::RuntimeConfig).
#[derive(Clone)]
pub struct ResilientOutboundHttpClient {
    links: Arc<HashMap<String, ResolvedLink>>,
}

impl ResilientOutboundHttpClient {
    /// True if there are no resolvable HTTP links.
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// Build catalog from runtime config (HTTP links only). Skips invalid entries with tracing warnings.
    pub fn from_runtime_config(runtime: &RuntimeConfig) -> Result<Self, OutboundHttpClientError> {
        let mut map = HashMap::new();
        let mut any_http_declared = false;
        for link in &runtime.service_links {
            if link.name.is_empty() {
                tracing::warn!("service_links: skipping entry with empty name");
                continue;
            }
            let transport = OutboundTransport::try_from(link.transport)
                .unwrap_or(OutboundTransport::OutboundTransportUnspecified);
            if matches!(
                transport,
                OutboundTransport::OutboundTransportHttp
                    | OutboundTransport::OutboundTransportUnspecified
            ) {
                any_http_declared = true;
            }
            if !matches!(
                transport,
                OutboundTransport::OutboundTransportHttp
                    | OutboundTransport::OutboundTransportUnspecified
            ) {
                tracing::debug!(
                    link = %link.name,
                    ?transport,
                    "service link transport not HTTP; skipped for outbound HTTP client"
                );
                continue;
            }
            if link.base_url.is_empty() {
                tracing::warn!(link = %link.name, "service_links: empty base_url, skip");
                continue;
            }
            let policy = resolve_policy_for_link(runtime, link);
            let reqwest = build_reqwest_client(&policy).map_err(|e| {
                OutboundHttpClientError::RequestFailed(format!(
                    "build client for {}: {}",
                    link.name, e
                ))
            })?;
            let cb_cfg = circuit_breaker_for_link(&link.name, &policy);
            let breaker = Arc::new(CircuitBreaker::new(cb_cfg));
            let extra_headers = resolve_link_headers(link);
            map.insert(
                link.name.clone(),
                ResolvedLink {
                    base_url: link.base_url.trim_end_matches('/').to_string(),
                    policy,
                    reqwest,
                    breaker,
                    extra_headers,
                },
            );
        }
        if any_http_declared && map.is_empty() {
            return Err(OutboundHttpClientError::RequestFailed(
                "runtime.service_links lists HTTP or unspecified transport entries but none had a valid base_url"
                    .to_string(),
            ));
        }
        Ok(Self {
            links: Arc::new(map),
        })
    }
}

fn resolve_link_headers(link: &ServiceLinkConfig) -> Vec<HttpHeader> {
    let mut h = Vec::new();
    for (k, v) in &link.default_headers {
        h.push(HttpHeader {
            key: k.clone(),
            value: v.clone(),
        });
    }
    if let (Some(name), Some(env_var)) = (
        link.api_key_header_name.as_deref(),
        link.api_key_env_var.as_deref(),
    ) {
        if let Ok(val) = std::env::var(env_var) {
            h.push(HttpHeader {
                key: name.to_string(),
                value: val,
            });
        } else {
            tracing::warn!(
                link = %link.name,
                env_var,
                "api_key_env_var not set; link may fail auth"
            );
        }
    }
    if let Some(env_var) = link.bearer_token_env_var.as_deref() {
        if let Ok(val) = std::env::var(env_var) {
            h.push(HttpHeader {
                key: "Authorization".to_string(),
                value: format!("Bearer {val}"),
            });
        } else {
            tracing::warn!(
                link = %link.name,
                env_var,
                "bearer_token_env_var not set; link may fail auth"
            );
        }
    }
    h
}

fn build_reqwest_client(
    policy: &plexspaces_proto::node::v1::ClientTransportPolicy,
) -> Result<reqwest::Client, reqwest::Error> {
    let mut b = reqwest::Client::builder()
        .user_agent(concat!(
            "plexspaces-http-client/",
            env!("CARGO_PKG_VERSION")
        ))
        // Keep test and embedded-node initialization deterministic on macOS and avoid
        // reqwest/system-configuration proxy autodiscovery panics.
        .no_proxy();
    if let Some(d) = policy.connect_timeout.as_ref() {
        b = b.connect_timeout(proto_duration_to_std(d));
    }
    if policy.max_redirects > 0 {
        b = b.redirect(Policy::limited(policy.max_redirects as usize));
    }
    b.build()
}

fn proto_duration_to_std(d: &prost_types::Duration) -> Duration {
    let s = d.seconds.max(0) as u64;
    let ns = d.nanos.max(0) as u32;
    Duration::new(s, ns)
}

#[async_trait]
impl OutboundHttpClient for ResilientOutboundHttpClient {
    async fn execute(
        &self,
        link_name: &str,
        request: OutboundHttpRequest,
    ) -> Result<OutboundHttpResponse, CoreOutboundError> {
        let link = self
            .links
            .get(link_name)
            .ok_or_else(|| CoreOutboundError::UnknownLink(link_name.to_string()))?;

        let span = tracing::info_span!(
            "outbound_http",
            link = link_name,
            method = %request.method,
            path = %request.path_and_query,
            status = Empty,
            retry_attempt = Empty,
        );
        async move {
            if !link.breaker.is_request_allowed().await {
                metrics::counter!(
                    "plexspaces_outbound_http_circuit_reject_total",
                    "link" => link_name.to_string()
                )
                .increment(1);
                return Err(CoreOutboundError::CircuitOpen {
                    link: link_name.to_string(),
                    detail: "breaker rejected".to_string(),
                });
            }

            let method = match reqwest::Method::from_bytes(request.method.as_bytes()) {
                Ok(m) => m,
                Err(_) => {
                    link.breaker.record_failure().await;
                    return Err(CoreOutboundError::RequestFailed(format!(
                        "invalid HTTP method {}",
                        request.method
                    )));
                }
            };

            let retry_policy = link.policy.retry.as_ref();
            let max_attempts = effective_max_attempts(retry_policy);
            let allow_retry = retry_policy
                .map(|p| method_allows_retry(&request.method, p))
                .unwrap_or(false);

            let url = match join_url(&link.base_url, &request.path_and_query) {
                Ok(u) => u,
                Err(e) => {
                    link.breaker.record_failure().await;
                    return Err(CoreOutboundError::InvalidUrl(e));
                }
            };

            let mut last_err: Option<String> = None;
            for attempt in 1..=max_attempts {
                tracing::Span::current().record("retry_attempt", attempt);

                let req_timeout = link
                    .policy
                    .request_timeout
                    .as_ref()
                    .map(proto_duration_to_std);

                let mut rb = link
                    .reqwest
                    .request(method.clone(), &url)
                    .headers(collect_headers(&request, &link.extra_headers));

                if let Some(d) = req_timeout {
                    rb = rb.timeout(d);
                }

                if !request.body.is_empty() {
                    rb = rb.body(request.body.clone());
                }

                let t0 = std::time::Instant::now();
                let resp = rb.send().await;
                let elapsed = t0.elapsed().as_secs_f64();

                metrics::histogram!(
                    "plexspaces_outbound_http_request_seconds",
                    "link" => link_name.to_string()
                )
                .record(elapsed);

                match resp {
                    Ok(r) => {
                        let status = r.status().as_u16();
                        tracing::Span::current().record("status", status);
                        let retriable =
                            status_is_retriable(status) && allow_retry && attempt < max_attempts;

                        if retriable {
                            metrics::counter!(
                                "plexspaces_outbound_http_retry_total",
                                "link" => link_name.to_string()
                            )
                            .increment(1);
                            if let Some(p) = retry_policy {
                                let delay =
                                    backoff_duration_for_attempt(p, attempt, &mut thread_rng());
                                tokio::time::sleep(delay).await;
                            }
                            last_err = Some(format!("HTTP {status}"));
                            continue;
                        }

                        let headers: Vec<HttpHeader> = r
                            .headers()
                            .iter()
                            .filter_map(|(k, v)| {
                                v.to_str().ok().map(|s| HttpHeader {
                                    key: k.to_string(),
                                    value: s.to_string(),
                                })
                            })
                            .collect();
                        let body = match r.bytes().await {
                            Ok(b) => b,
                            Err(e) => {
                                let msg = e.to_string();
                                link.breaker.record_failure().await;
                                return Err(CoreOutboundError::RequestFailed(msg));
                            }
                        };

                        if body.len() > MAX_BODY_BYTES {
                            link.breaker.record_failure().await;
                            metrics::counter!(
                                "plexspaces_outbound_http_errors_total",
                                "link" => link_name.to_string(),
                                "kind" => "body_too_large"
                            )
                            .increment(1);
                            return Err(CoreOutboundError::BodyTooLarge);
                        }

                        if status >= 400 {
                            link.breaker.record_failure().await;
                            metrics::counter!(
                                "plexspaces_outbound_http_errors_total",
                                "link" => link_name.to_string(),
                                "kind" => "http_status"
                            )
                            .increment(1);
                        } else {
                            link.breaker.record_success().await;
                            metrics::counter!(
                                "plexspaces_outbound_http_success_total",
                                "link" => link_name.to_string()
                            )
                            .increment(1);
                        }

                        return Ok(OutboundHttpResponse {
                            request_id: request.request_id.clone(),
                            status: status as u32,
                            headers,
                            body: body.to_vec(),
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        tracing::Span::current().record("status", "error");
                        let transient = e.is_timeout() || e.is_connect() || e.is_request();
                        let can_retry = transient && allow_retry && attempt < max_attempts;
                        if can_retry {
                            metrics::counter!(
                                "plexspaces_outbound_http_retry_total",
                                "link" => link_name.to_string()
                            )
                            .increment(1);
                            if let Some(p) = retry_policy {
                                let delay =
                                    backoff_duration_for_attempt(p, attempt, &mut thread_rng());
                                tokio::time::sleep(delay).await;
                            }
                            last_err = Some(msg);
                            continue;
                        }
                        link.breaker.record_failure().await;
                        metrics::counter!(
                            "plexspaces_outbound_http_errors_total",
                            "link" => link_name.to_string(),
                            "kind" => "request"
                        )
                        .increment(1);
                        return Err(CoreOutboundError::RequestFailed(last_err.unwrap_or(msg)));
                    }
                }
            }

            link.breaker.record_failure().await;
            Err(CoreOutboundError::RequestFailed(
                last_err.unwrap_or_else(|| "exhausted retries".to_string()),
            ))
        }
        .instrument(span)
        .await
    }
}

fn join_url(base: &str, path: &str) -> Result<String, String> {
    let p = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let base = base.trim_end_matches('/');
    Ok(format!("{base}{p}"))
}

fn collect_headers(
    req: &OutboundHttpRequest,
    link_headers: &[HttpHeader],
) -> reqwest::header::HeaderMap {
    let mut m = reqwest::header::HeaderMap::new();
    for h in link_headers {
        if let (Ok(n), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(h.key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&h.value),
        ) {
            m.insert(n, val);
        }
    }
    for h in &req.headers {
        if let (Ok(n), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(h.key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&h.value),
        ) {
            m.insert(n, val);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_url_paths() {
        assert_eq!(join_url("https://a.com", "/x").unwrap(), "https://a.com/x");
        assert_eq!(join_url("https://a.com/", "y").unwrap(), "https://a.com/y");
    }
}
