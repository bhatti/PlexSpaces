// SPDX-License-Identifier: AGPL-3.0-or-later

//! Merge [`ClientTransportPolicy`](plexspaces_proto::node::v1::ClientTransportPolicy) from runtime defaults and templates.

use plexspaces_proto::circuitbreaker::prv::{CircuitBreakerConfig, FailureStrategy};
use plexspaces_proto::node::v1::{ClientTransportPolicy, HttpRetryPolicy, RuntimeConfig};
use prost_types::Duration as ProtoDuration;

/// Effective max attempts (at least 1).
pub fn effective_max_attempts(retry: Option<&HttpRetryPolicy>) -> u32 {
    retry.map(|r| r.max_attempts.max(1)).unwrap_or(1)
}

/// Merge default runtime policy with an optional named template.
pub fn merge_client_transport_policy(
    default_policy: Option<&ClientTransportPolicy>,
    template: Option<&ClientTransportPolicy>,
) -> ClientTransportPolicy {
    let mut out = ClientTransportPolicy {
        connect_timeout: None,
        request_timeout: None,
        retry: None,
        circuit_breaker: None,
        max_redirects: 0,
    };
    if let Some(d) = default_policy {
        merge_layer(&mut out, d);
    }
    if let Some(t) = template {
        merge_layer(&mut out, t);
    }
    out
}

fn merge_layer(dst: &mut ClientTransportPolicy, src: &ClientTransportPolicy) {
    if src.connect_timeout.is_some() {
        dst.connect_timeout = src.connect_timeout;
    }
    if src.request_timeout.is_some() {
        dst.request_timeout = src.request_timeout;
    }
    if src.retry.is_some() {
        dst.retry = src.retry.clone();
    }
    if src.circuit_breaker.is_some() {
        dst.circuit_breaker = src.circuit_breaker.clone();
    }
    if src.max_redirects > 0 {
        dst.max_redirects = src.max_redirects;
    }
}

/// Build circuit breaker config for a link, using policy or production-safe defaults.
pub fn circuit_breaker_for_link(
    link_name: &str,
    policy: &ClientTransportPolicy,
) -> CircuitBreakerConfig {
    if let Some(cb) = policy.circuit_breaker.clone() {
        let mut c = cb;
        if c.name.is_empty() {
            c.name = format!("outbound-http-{link_name}");
        }
        return c;
    }
    CircuitBreakerConfig {
        name: format!("outbound-http-{link_name}"),
        failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
        failure_threshold: 5,
        success_threshold: 2,
        timeout: Some(ProtoDuration {
            seconds: 30,
            nanos: 0,
        }),
        half_open_config: None,
        sliding_window: None,
        request_timeout: None,
        max_half_open_requests: 5,
    }
}

/// Resolve named template from runtime config.
pub fn resolve_policy_for_link(
    runtime: &RuntimeConfig,
    link: &plexspaces_proto::node::v1::ServiceLinkConfig,
) -> ClientTransportPolicy {
    let default = runtime.default_outbound_client_policy.as_ref();
    let template_key = link.policy_template.as_deref();
    let template = template_key.and_then(|k| runtime.outbound_policy_templates.get(k));
    merge_client_transport_policy(default, template)
}

/// Validate `ApplicationSpec.required_service_links` against `RuntimeConfig.service_links` and templates.
pub fn validate_application_service_links(
    runtime: &RuntimeConfig,
    required: &[plexspaces_proto::application::v1::ApplicationServiceLinkRequirement],
) -> Result<(), String> {
    let names: std::collections::HashSet<&str> = runtime
        .service_links
        .iter()
        .map(|l| l.name.as_str())
        .collect();
    for req in required {
        if req.link_name.is_empty() {
            return Err("required_service_links: empty link_name".to_string());
        }
        if !names.contains(req.link_name.as_str()) {
            return Err(format!(
                "required_service_links: unknown link {:?} (not in runtime.service_links)",
                req.link_name
            ));
        }
        if let Some(tpl) = req.policy_template.as_deref() {
            if !tpl.is_empty() && !runtime.outbound_policy_templates.contains_key(tpl) {
                return Err(format!(
                    "required_service_links: unknown policy_template {:?} for link {:?}",
                    tpl, req.link_name
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn empty_transport_policy() -> ClientTransportPolicy {
    ClientTransportPolicy {
        connect_timeout: None,
        request_timeout: None,
        retry: None,
        circuit_breaker: None,
        max_redirects: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::node::v1::ServiceLinkConfig;

    #[test]
    fn merge_prefers_template_over_default() {
        let mut d = empty_transport_policy();
        d.connect_timeout = Some(ProtoDuration {
            seconds: 1,
            nanos: 0,
        });
        let mut t = empty_transport_policy();
        t.connect_timeout = Some(ProtoDuration {
            seconds: 5,
            nanos: 0,
        });
        let m = merge_client_transport_policy(Some(&d), Some(&t));
        assert_eq!(m.connect_timeout.unwrap().seconds, 5);
    }

    #[test]
    fn validate_links() {
        let mut rt = RuntimeConfig::default();
        rt.service_links.push(ServiceLinkConfig {
            name: "api".to_string(),
            ..Default::default()
        });
        let ok = [
            plexspaces_proto::application::v1::ApplicationServiceLinkRequirement {
                link_name: "api".to_string(),
                policy_template: None,
            },
        ];
        assert!(validate_application_service_links(&rt, &ok).is_ok());

        let bad = [
            plexspaces_proto::application::v1::ApplicationServiceLinkRequirement {
                link_name: "missing".to_string(),
                policy_template: None,
            },
        ];
        assert!(validate_application_service_links(&rt, &bad).is_err());
    }

    #[test]
    fn validate_template_key() {
        let mut rt = RuntimeConfig::default();
        rt.service_links.push(ServiceLinkConfig {
            name: "api".to_string(),
            ..Default::default()
        });
        rt.outbound_policy_templates
            .insert("fast".to_string(), empty_transport_policy());
        let ok = [
            plexspaces_proto::application::v1::ApplicationServiceLinkRequirement {
                link_name: "api".to_string(),
                policy_template: Some("fast".to_string()),
            },
        ];
        assert!(validate_application_service_links(&rt, &ok).is_ok());

        let bad = [
            plexspaces_proto::application::v1::ApplicationServiceLinkRequirement {
                link_name: "api".to_string(),
                policy_template: Some("nope".to_string()),
            },
        ];
        assert!(validate_application_service_links(&rt, &bad).is_err());
    }
}
