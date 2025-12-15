# gRPC Middleware - Production-Ready Interceptors

**Purpose**: Production-ready middleware (interceptors) for gRPC services with observability, security, and reliability features.

## Overview

This crate implements **Phase 3: gRPC Middleware** of PlexSpaces. It provides:
- **Observability**: Metrics (Prometheus), tracing (OpenTelemetry), logging
- **Security**: mTLS, authentication, authorization (RBAC)
- **Reliability**: Rate limiting, circuit breaking, retries, compression

## Key Components

### InterceptorChain

Composable chain of interceptors:

```rust
pub struct InterceptorChain {
    interceptors: Vec<Box<dyn Interceptor>>,
}
```

### Middleware Types

- **MetricsInterceptor**: Always enabled, tracks request/response metrics
- **TracingInterceptor**: Distributed tracing with OpenTelemetry
- **AuthInterceptor**: JWT or mTLS authentication
- **RateLimitInterceptor**: Rate limiting per client/service
- **RetryInterceptor**: Automatic retries with exponential backoff
- **CompressionInterceptor**: Request/response compression
- **CircuitBreakerInterceptor**: Circuit breaker pattern

## Usage Examples

### Basic Usage with Metrics

```rust
use plexspaces_grpc_middleware::{InterceptorChain, MetricsInterceptor};
use plexspaces_proto::grpc::v1::{MiddlewareConfig, MiddlewareSpec};

// Create interceptor chain with metrics (always enabled)
let chain = InterceptorChain::from_config(&MiddlewareConfig::default())?;

// Apply to gRPC service
// Server::builder()
//     .layer(chain)
//     .add_service(ActorService::new(...))
//     .serve(addr).await?;
```

### Full Middleware Stack

```rust
use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{MiddlewareConfig, MiddlewareSpec, MiddlewareType, AuthMethod};

let config = MiddlewareConfig {
    middleware: vec![
        MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeMetrics as i32,
            enabled: true,  // Always enabled
            priority: 10,   // First to see requests
            config: None,
        },
        MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeAuth as i32,
            enabled: true,
            priority: 30,
            config: Some(/* auth config */),
        },
    ],
};

let chain = InterceptorChain::from_config(&config)?;
```

## Configuration

### Configuration File (release.toml)

```toml
[grpc.middleware]
middleware = [
    { type = "metrics", enabled = true, priority = 10 },
    { type = "tracing", enabled = true, priority = 20 },
    { type = "auth", enabled = true, priority = 30, config = { method = "jwt", key = "secret" } },
    { type = "rate_limit", enabled = true, priority = 40, config = { requests_per_second = 100 } },
]
```

## Design Principles

1. **Composable**: Chain multiple interceptors in any order
2. **Configurable**: Enable/disable via config (metrics always on)
3. **Proto-First**: All configuration defined in proto
4. **Zero-Cost When Disabled**: Feature flags prevent overhead
5. **Observable**: All interceptors emit metrics

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `tonic`: gRPC framework
- `tracing`: Distributed tracing
- `metrics`: Metrics collection

## Dependents

This crate is used by:
- `plexspaces_node`: Node applies middleware to all gRPC services
- All gRPC services: ActorService, TupleSpaceService, etc.

## References

- Implementation: `crates/grpc-middleware/src/`
- Tests: `crates/grpc-middleware/tests/`
- Proto definitions: `proto/plexspaces/v1/grpc.proto`

