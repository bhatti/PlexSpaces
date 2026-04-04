# Archived: Service links / WIT design notes

**Superseded by:** [`docs/wit-design.md`](../docs/wit-design.md) (canonical WIT interface and service-link architecture), [`docs/services.md`](../docs/services.md) (operations), [`component-services-design.md`](component-services-design.md) (internal design), and [`component-services-roadmap.md`](component-services-roadmap.md) (implementation phases).

The remainder of this file is kept for historical reference only.

---

# Service links, outbound clients, and Wasm boundaries

## Purpose

This document describes how PlexSpaces models **declarative external dependencies** (wasmCloud-style *service links*), how the node **resolves** them through `RuntimeConfig` and optionally the **ObjectRegistry**, and how **HTTP** outbound calls are executed with shared **resilience and observability** policy. It also positions **WIT** and the optional **WebAssembly Component Model** relative to today’s JSON-oriented host boundary.

## Non-goals

- Replacing internal gRPC services between node components.
- Building a full service mesh or external control plane inside the node.
- Mandating Component Model or wRPC in the first shippable milestones.

## Naming (proto and runtime)

| Concept | Proto / runtime home |
|--------|----------------------|
| Static catalog entry | `ServiceLinkConfig` in `RuntimeConfig.service_links` |
| App requirement | `ApplicationServiceLinkRequirement` in `ApplicationSpec.required_service_links` |
| Merged timeouts / retry / breaker | `ClientTransportPolicy`; optional `RuntimeConfig.outbound_policy_templates` |
| Wire transport | `OutboundTransport` (`HTTP`, `GRPC`, `CHANNEL`) — **HTTP client path is implemented**; gRPC channel-by-link is planned (see archived `wit-plan.md`). |

`ObjectRegistration.grpc_address` is the **primary connect URI** (HTTP(S) origin or gRPC endpoint), not “gRPC-only” in meaning.

## Data flow

1. **Declare** — Operators add `[[runtime.service_links]]` in `release.toml` (see `plexspaces_common::release_parser`). Applications declare `[[applications.required_service_links]]`.
2. **Validate** — Deploy merges app spec with node `RuntimeConfig`; `plexspaces_http_client::validate_application_service_links` ensures every required link exists and optional policy templates are defined.
3. **Resolve** — `ServiceLocator` exposes `OutboundHttpClient` built from merged `ClientTransportPolicy` per link (`plexspaces-http-client`).
4. **Optional discovery** — If `publish_to_registry` is true, the node registers an `ObjectTypeService` row via `object_registry_helpers::register_outbound_service_link` (metadata labels `plexspaces.link_name`, `plexspaces.transport`).

## Traits and layering (Rust)

- **Core** (`plexspaces-core`): `OutboundHttpClient` trait, request/response types, errors; `ServiceLocator` extension for registration and lookup.
- **Implementation** (`plexspaces-http-client`): `ResilientOutboundHttpClient` — reqwest, retries (idempotent methods by default), circuit breaker integration, metrics, tracing spans.
- **SDKs** (Rust / Python / TypeScript / Go): decorators and ergonomics only; **policy execution for WASM-hosted calls stays on the host** so behavior does not fork per language.

Avoid duplicate registries: one **ObjectRegistry** with documented labels for link discovery.

## HTTP client behavior

- Separate **connect** and **request** timeouts from `ClientTransportPolicy`.
- Retries: exponential backoff with **full jitter**; non-idempotent methods are not retried unless explicitly allowed in policy.
- Circuit breaker: per-link, configured from `CircuitBreakerConfig` in merged policy.
- Observability: metrics and tracing on the outbound path (see `crates/http-client`).

## Security

- Secrets are referenced by **environment variable names** in proto (`api_key_env_var`, `bearer_token_env_var`), not inlined in configuration artifacts.
- Align conceptual auth shapes with OpenAPI-style schemes (`apiKey`, `http` bearer, etc.); values come from env or future secret services.

## WIT and Wasm (phased)

- **Today**: JSON payloads at the WIT boundary (`docs/polyglot.md`); host functions remain the integration surface.
- **Next**: Narrow WIT types for outbound HTTP (method, path, headers, body bytes) implemented in the host using the same core client and policy.
- **Optional later**: Component Model + Canonical ABI for guests; explicit declaration in `ApplicationSpec` rather than relying on opaque “inspect wasm” tooling.

## Tradeoffs (summary)

| Topic | Option A | Option B |
|-------|----------|----------|
| Guest payload | JSON at WIT boundary (current) | Canonical ABI / components |
| Policy enforcement | Central host (preferred) | Per-SDK divergent stacks |
| Link resolution | Config-only | Config + ObjectRegistry publish |

## References (historical)

- Polyglot model: [`docs/polyglot.md`](../docs/polyglot.md)
- Architecture: [`docs/architecture.md`](../docs/architecture.md)
- Examples: `examples/*/apps/components/`
