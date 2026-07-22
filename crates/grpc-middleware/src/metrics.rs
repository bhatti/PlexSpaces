// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public
// General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! gRPC server metrics via the unified `metrics` facade (same recorder as MetricsService).

use async_trait::async_trait;
use plexspaces_proto::grpc::v1::{
    InterceptorDecision, InterceptorRequest, InterceptorResponse, InterceptorResult,
};
use tonic::Status;

use crate::chain::{Interceptor, InterceptorError};

/// Metrics interceptor: records standard gRPC server metrics into the process-wide `metrics` recorder.
pub struct MetricsInterceptor;

impl MetricsInterceptor {
    /// Create a new metrics interceptor.
    pub fn new() -> Self {
        Self
    }

    fn parse_method(method: &str) -> (&str, &str) {
        if let Some(slash_pos) = method.rfind('/') {
            let service_path = &method[1..slash_pos];
            let method_name = &method[slash_pos + 1..];
            let service_name = if let Some(dot_pos) = service_path.rfind('.') {
                &service_path[dot_pos + 1..]
            } else {
                service_path
            };
            (service_name, method_name)
        } else {
            ("unknown", method)
        }
    }
}

impl Default for MetricsInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Interceptor for MetricsInterceptor {
    async fn before_request(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        let (service, method) = Self::parse_method(&context.method);
        let svc = service.to_string();
        let mth = method.to_string();

        metrics::counter!(
            "grpc_server_started_total",
            "grpc_service" => svc.clone(),
            "grpc_method" => mth.clone(),
        )
        .increment(1);

        metrics::gauge!(
            "grpc_server_active_requests",
            "grpc_service" => svc,
            "grpc_method" => mth,
        )
        .increment(1.0);

        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    async fn after_response(
        &self,
        context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError> {
        let (service, method) = Self::parse_method(&context.method);
        let svc = service.to_string();
        let mth = method.to_string();

        metrics::gauge!(
            "grpc_server_active_requests",
            "grpc_service" => svc.clone(),
            "grpc_method" => mth.clone(),
        )
        .decrement(1.0);

        let status_code = context.status_code.to_string();
        metrics::counter!(
            "grpc_server_handled_total",
            "grpc_service" => svc.clone(),
            "grpc_method" => mth.clone(),
            "grpc_code" => status_code,
        )
        .increment(1);

        if let Some(duration) = &context.duration {
            let seconds = duration.seconds as f64 + (duration.nanos as f64 / 1_000_000_000.0);
            metrics::histogram!(
                "grpc_server_handling_seconds",
                "grpc_service" => svc,
                "grpc_method" => mth,
            )
            .record(seconds);
        }

        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    async fn on_error(&self, error: &Status) {
        let _ = error;
    }

    fn name(&self) -> &str {
        "metrics"
    }

    fn priority(&self) -> i32 {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics_exporter_prometheus::PrometheusHandle;
    use std::sync::OnceLock;

    static TEST_PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

    fn test_prom_handle() -> &'static PrometheusHandle {
        TEST_PROM_HANDLE.get_or_init(|| {
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .install_recorder()
                .expect("install metrics recorder for grpc-middleware tests")
        })
    }

    #[test]
    fn test_metrics_interceptor_creation() {
        let interceptor = MetricsInterceptor::new();
        assert_eq!(interceptor.name(), "metrics");
        assert_eq!(interceptor.priority(), 10);
    }

    #[test]
    fn test_parse_method() {
        let (service, method) =
            MetricsInterceptor::parse_method("/plexspaces.actor.v1.ActorService/SpawnActor");
        assert_eq!(service, "ActorService");
        assert_eq!(method, "SpawnActor");

        let (service, method) = MetricsInterceptor::parse_method("/Service/Method");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[tokio::test]
    async fn test_before_request() {
        let _ = test_prom_handle();
        let interceptor = MetricsInterceptor::new();
        let context = InterceptorRequest {
            method: "/plexspaces.actor.v1.ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = interceptor.before_request(&context).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(
            result.decision,
            InterceptorDecision::InterceptorDecisionAllow as i32
        );
    }

    #[tokio::test]
    async fn test_after_response() {
        let _ = test_prom_handle();
        let interceptor = MetricsInterceptor::new();
        let context = InterceptorResponse {
            status_code: 0,
            headers: std::collections::HashMap::new(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            duration: Some(plexspaces_proto::prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000,
            }),
            request_id: ulid::Ulid::new().to_string(),
            method: "/plexspaces.actor.v1.ActorService/SpawnActor".to_string(),
        };

        let result = interceptor.after_response(&context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_grpc_metrics_in_prometheus_render() {
        let handle = test_prom_handle();
        let interceptor = MetricsInterceptor::new();
        let context = InterceptorRequest {
            method: "/test.Service/Method".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        interceptor.before_request(&context).await.unwrap();
        let text = handle.render();
        assert!(
            text.contains("grpc_server_started_total"),
            "render={}",
            text
        );
    }
}
