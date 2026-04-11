# Metrics and Observability

PlexSpaces uses a unified metrics pipeline built on the Rust [`metrics`](https://docs.rs/metrics) crate with a Prometheus backend. All counters, gauges, and histograms across the framework flow through a single process-wide recorder and are accessible via gRPC API, Prometheus text exposition, and the dashboard.

## Architecture

```
Call sites (metrics::counter!, gauge!, histogram!)
        │
        ▼
┌─────────────────────────────────┐
│  metrics-exporter-prometheus    │  ← single process-wide recorder
│  (OnceLock<PrometheusHandle>)   │    installed at node startup
└─────────────────────────────────┘
        │
        ├──► MetricsService gRPC API     (ExportPrometheus, GetMetrics, ...)
        ├──► HTTP /metrics endpoint      (via MetricsPrometheusRenderer)
        ├──► DashboardService            (overlay on NodeMetrics)
        └──► NodeService.GetMetrics      (operational counters from exposition)
```

The recorder is installed once during `ServiceLocator` initialization by calling `metrics_service::install_metrics_recorder()`. Every `metrics::counter!` / `gauge!` / `histogram!` call site across all crates records into this shared backend with zero additional wiring.

## Accessing metrics

### Prometheus text format

**gRPC**: `MetricsService.ExportPrometheus`
**HTTP**: `GET /metrics`

Returns standard Prometheus text exposition format, ready for scraping by Prometheus, Grafana Agent, or any compatible collector.

```bash
# via grpcurl
grpcurl -plaintext localhost:50051 plexspaces.metrics.v1.MetricsService/ExportPrometheus

# via HTTP (when gateway is enabled)
curl http://localhost:8080/metrics
```

### Structured query

**gRPC**: `MetricsService.GetMetrics`
**HTTP**: `GET /api/v1/metrics`

Returns filtered metrics as structured protobuf `Metric` messages. Supports filtering by name pattern (glob) and label key-value pairs.

### Metric definitions

**gRPC**: `MetricsService.ListMetricDefinitions`
**HTTP**: `GET /api/v1/metrics/definitions`

Returns the catalog of all registered metric definitions with name, type, help text, labels, and histogram buckets.

### Typed recording RPCs

For remote or cross-service recording, three typed RPCs accept structured requests with namespace dimensions and map them to R.E.D. series:

| RPC | Records |
|-----|---------|
| `RecordMessageRouting` | Message routing rate, errors, and duration |
| `RecordActorActivation` | Actor activation rate, errors, and duration |
| `RecordChannelMetrics` | Channel operation rate, errors, and duration |

### In-process access

Any crate that depends on `metrics` (workspace dependency) can emit metrics directly:

```rust
metrics::counter!("plexspaces_messages_routed_total", "namespace" => ns).increment(1);
metrics::histogram!("plexspaces_message_routing_duration_seconds", "namespace" => ns).record(secs);
```

For reading metrics programmatically, `ServiceLocator` exposes:
- `get_metrics_prometheus_renderer()` returns a `MetricsPrometheusRenderer` trait object for rendering Prometheus text
- `get_metrics_service_access()` returns a `MetricsServiceAccess` trait object for structured queries

---

## Metric reference

All metrics follow Prometheus naming conventions: `snake_case`, `_total` suffix for counters, `_seconds` suffix for duration histograms. Namespace labels (`namespace`) correspond to the application/tenant ID for multi-tenant isolation.

### Message routing

Follows R.E.D. methodology (Rate, Errors, Duration).

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_messages_routed_total` | counter | `namespace` | Messages routed (success + failure) |
| `plexspaces_messages_failed_total` | counter | `namespace`, `error_type` | Failed message routings |
| `plexspaces_message_routing_duration_seconds` | histogram | `namespace` | Routing latency |

Buckets: 0.0001, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0

### Actor lifecycle

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_actor_spawn_total` | counter | `namespace` | Actors spawned/registered |
| `plexspaces_actor_active` | gauge | `namespace` | Currently active actors |
| `plexspaces_actor_activations_total` | counter | `namespace`, `activation_type` | Actor activations (lazy, eager, virtual) |
| `plexspaces_actor_activation_errors_total` | counter | `namespace` | Failed activations |
| `plexspaces_actor_activation_duration_seconds` | histogram | `namespace`, `activation_type` | Activation latency |
| `plexspaces_actor_init_total` | counter | `actor_type` | Actor initializations |
| `plexspaces_actor_init_duration_seconds` | histogram | `actor_type` | Initialization latency |
| `plexspaces_actor_terminate_total` | counter | `actor_type`, `reason` | Actor terminations |
| `plexspaces_actor_terminate_duration_seconds` | histogram | `actor_type` | Termination latency |
| `plexspaces_actor_exit_handled_total` | counter | `actor_type`, `exit_reason` | Exit signals handled |
| `plexspaces_actor_exit_propagated_total` | counter | | Exit signals propagated to parent |
| `plexspaces_actor_children_count` | gauge | `actor_id` | Number of children |
| `plexspaces_actor_subtree_size` | gauge | `actor_id` | Total subtree size |
| `plexspaces_actor_parent_child_registered_total` | counter | | Parent-child links created |
| `plexspaces_actor_parent_child_unregistered_total` | counter | | Parent-child links removed |

### Actor registry internals

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_actor_registry_local_tell_total` | counter | `result` | Local tell() operations (ok/error) |
| `plexspaces_actor_registry_local_tell_duration_seconds` | histogram | | Local tell() latency |
| `plexspaces_actor_registry_temporary_sender_mappings` | gauge | | Active temporary sender mappings |
| `plexspaces_actor_registry_temporary_sender_expired_total` | counter | | Expired temporary sender entries |

### Channel operations

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_channel_ack_total` | counter | `channel`, `backend` | Acknowledged messages |
| `plexspaces_channel_nack_total` | counter | `channel`, `backend`, `requeue` | Negatively acknowledged messages |
| `plexspaces_channel_dlq_total` | counter | `channel`, `backend`, `reason` | Dead-letter queue events |
| `plexspaces_channel_error_total` | counter | `channel`, `backend`, `operation` | Channel errors |
| `plexspaces_channel_operation_duration_seconds` | histogram | `channel`, `backend`, `operation` | Operation latency |
| `plexspaces_channel_messages_pending` | gauge | `channel`, `backend` | Pending message count |

R.E.D. variants (emitted via `RecordChannelMetrics` RPC):

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_channel_operations_total` | counter | `namespace`, `operation`, `backend` | Channel operations |
| `plexspaces_channel_errors_total` | counter | `namespace`, `operation`, `backend` | Channel operation errors |
| `plexspaces_channel_operation_duration_seconds` | histogram | `namespace`, `operation`, `backend` | Channel operation latency |
| `plexspaces_channel_delivery_attempts_total` | counter | `namespace`, `operation`, `backend`, `delivery_count` | Delivery attempts |
| `plexspaces_channel_operation_reason_total` | counter | `namespace`, `operation`, `backend`, `reason` | Operation reasons |

### gRPC server

Emitted by the `MetricsInterceptor` in the gRPC middleware chain (priority 10). Labels are extracted from the method path (e.g. `/plexspaces.actor.v1.ActorService/SpawnActor` yields `grpc_service=ActorService`, `grpc_method=SpawnActor`).

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `grpc_server_started_total` | counter | `grpc_service`, `grpc_method` | Requests started |
| `grpc_server_handled_total` | counter | `grpc_service`, `grpc_method`, `grpc_code` | Requests completed |
| `grpc_server_handling_seconds` | histogram | `grpc_service`, `grpc_method` | Request handling latency |
| `grpc_server_active_requests` | gauge | `grpc_service`, `grpc_method` | In-flight requests |
| `grpc_server_msg_received_total` | counter | `grpc_service`, `grpc_method` | Stream messages received |
| `grpc_server_msg_sent_total` | counter | `grpc_service`, `grpc_method` | Stream messages sent |

Buckets: 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0

### Node and delivery

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_node_messages_routed_total` | counter | `node_id` | Messages routed on this node |
| `plexspaces_node_local_deliveries_total` | counter | `node_id` | Successful local deliveries |
| `plexspaces_node_remote_deliveries_total` | counter | `node_id` | Successful remote deliveries |
| `plexspaces_node_failed_deliveries_total` | counter | `node_id` | Failed deliveries |
| `plexspaces_connections_total` | counter | `node_id`, `remote_node_id`, `event_type` | Connection events |
| `plexspaces_connection_duration_seconds` | histogram | `node_id`, `remote_node_id` | Connection duration |
| `plexspaces_node_health_requests_total` | counter | `components_count` | Health check requests |
| `plexspaces_node_readiness_checks_total` | counter | | Readiness checks |
| `plexspaces_node_liveness_checks_total` | counter | | Liveness checks |
| `plexspaces_node_application_deploy_attempts_total` | counter | | Application deployment attempts |

### Shard operations

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_node_shard_groups_created_total` | counter | `node_id` | Shard groups created |
| `plexspaces_node_shard_messages_sent_total` | counter | `node_id` | Messages sent to shard actors |
| `plexspaces_node_shard_messages_received_total` | counter | `node_id` | Messages received from shards |
| `plexspaces_node_shard_operations_total` | counter | `node_id` | Shard collective operations |
| `plexspaces_node_shard_operations_failed_total` | counter | `node_id` | Failed shard operations |

### Supervision

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_supervisor_child_started_total` | counter | `supervisor_id`, `child_type` | Child starts |
| `plexspaces_supervisor_child_stopped_total` | counter | `supervisor_id`, `child_type` | Child stops |
| `plexspaces_supervisor_child_restarted_total` | counter | `supervisor_id`, `child_type`, `restart_policy` | Child restarts |
| `plexspaces_supervisor_startup_duration_seconds` | histogram | `supervisor_id` | Supervisor startup latency |
| `plexspaces_supervisor_shutdown_duration_seconds` | histogram | `supervisor_id` | Supervisor shutdown latency |

### Application lifecycle

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_application_startup_total` | counter | `application` | Startup attempts |
| `plexspaces_application_startup_duration_seconds` | histogram | `application` | Startup latency |
| `plexspaces_application_startup_success_total` | counter | `application` | Successful startups |
| `plexspaces_application_startup_errors_total` | counter | `application` | Failed startups |
| `plexspaces_application_shutdown_total` | counter | `application` | Shutdown attempts |
| `plexspaces_application_shutdown_duration_seconds` | histogram | `application` | Shutdown latency |
| `plexspaces_application_shutdown_success_total` | counter | `application` | Successful shutdowns |
| `plexspaces_application_shutdown_errors_total` | counter | `application`, `error_type` | Failed shutdowns |

Buckets: 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0

### Facet system

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `plexspaces_facet_attach_attempts_total` | counter | `facet_type` | Attach attempts |
| `plexspaces_facet_attached_total` | counter | `facet_type` | Successful attachments |
| `plexspaces_facet_attach_errors_total` | counter | `facet_type`, `error` | Attach errors |
| `plexspaces_facet_attach_duration_seconds` | histogram | `facet_type` | Attach latency |
| `plexspaces_facet_active_total` | gauge | `facet_type` | Currently active facets |
| `plexspaces_facet_detach_attempts_total` | counter | `facet_type` | Detach attempts |
| `plexspaces_facet_detached_total` | counter | `facet_type` | Successful detachments |
| `plexspaces_facet_detach_errors_total` | counter | `facet_type`, `error` | Detach errors |
| `plexspaces_facet_detach_duration_seconds` | histogram | `facet_type` | Detach latency |
| `plexspaces_facet_down_total` | counter | `facet_type` | Teardowns |
| `plexspaces_facet_down_errors_total` | counter | `facet_type` | Teardown errors |
| `plexspaces_facet_down_duration_seconds` | histogram | `facet_type` | Teardown latency |
| `plexspaces_facet_intercept_before_total` | counter | `method` | Pre-interceptions |
| `plexspaces_facet_intercept_before_duration_seconds` | histogram | `method` | Pre-interception latency |
| `plexspaces_facet_intercept_after_total` | counter | `method` | Post-interceptions |
| `plexspaces_facet_intercept_after_duration_seconds` | histogram | `method` | Post-interception latency |
| `plexspaces_facet_intercept_shortcircuit_total` | counter | `method`, `facet_type` | Short-circuit interceptions |

---

## Design principles

**R.E.D. methodology** -- Core business operations (message routing, actor activation, channel operations) emit three series each: Rate (total counter), Errors (error counter), Duration (histogram). This provides the minimum observability needed for alerting and SLO tracking.

**Namespace isolation** -- Actor and message metrics carry a `namespace` label corresponding to the application/tenant ID. This enables per-tenant dashboards, alerting, and capacity planning without requiring separate metric endpoints.

**Single recorder** -- All `metrics::` macro calls across all crates flow through one `metrics-exporter-prometheus` recorder. There is no secondary in-memory metrics store; `NodeService.GetMetrics` and the dashboard read from the Prometheus text exposition output, not from duplicate counters.

**Zero-cost when unused** -- The `metrics` crate is a facade. If no recorder is installed (e.g. in unit tests that don't call `install_metrics_recorder`), all macro calls are no-ops with no allocation or synchronization overhead.

---

## Verification

```bash
# Metrics service unit tests
cargo test -p plexspaces-services --lib -- metrics_service

# gRPC middleware metrics tests
cargo test -p plexspaces-grpc-middleware --lib -- metrics

# Channel observability tests
cargo test -p plexspaces-channel --lib -- observability
```

## Related documentation

- [Services reference -- MetricsService](services.md#metricsservice)
- [Actor system -- Observability](actor-system.md#observability)
- [Architecture -- Metrics, dashboard, and APIs](architecture.md#metrics-dashboard-and-apis)
- [Proto definition](../proto/plexspaces/v1/metrics/metrics.proto)
- [Implementation](../crates/services/src/metrics_service/mod.rs)
