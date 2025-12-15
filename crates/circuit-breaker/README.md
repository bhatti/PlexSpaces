# Circuit Breaker - Fault Tolerance and Resilience

**Purpose**: Provides fault tolerance and resilience patterns for microservices through the circuit breaker pattern, preventing cascading failures in distributed systems.

## Overview

CircuitBreaker is essential for resilient microservices in PlexSpaces:
- **Fault Isolation**: Prevent failing services from bringing down the whole system
- **Fast Failure**: Fail fast instead of waiting for timeouts
- **Automatic Recovery**: Self-healing through half-open state testing
- **Resource Protection**: Prevent resource exhaustion from repeated failures

## State Machine

```
Closed → (failures≥threshold) → Open
  ↑                              ↓
  └── (reset/timeout) ←──────────┘
        ↓
   Half-Open
        ↓
   (success≥threshold) → Closed
   (failures>0) → Open
```

## Usage Examples

### Basic Usage

```rust
use plexspaces_circuit_breaker::*;
use plexspaces_proto::circuitbreaker::prv::*;

// Configure circuit breaker
let config = CircuitBreakerConfig {
    name: "payment-service".to_string(),
    failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
    failure_threshold: 5,  // Trip after 5 consecutive failures
    success_threshold: 2,  // Close after 2 successes in half-open
    timeout_secs: 60,      // Open for 60 seconds
    ..Default::default()
};

let breaker = CircuitBreaker::new(config);

// Execute request through circuit breaker
match breaker.execute(|| async {
    // Call external service
    payment_service.process_payment(amount).await
}).await {
    Ok(result) => Ok(result),
    Err(CircuitBreakerError::CircuitOpen) => {
        // Circuit is open, fail fast
        Err("Service unavailable".into())
    }
    Err(e) => Err(e),
}
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `tokio`: Async runtime

## Dependents

This crate is used by:
- `plexspaces_grpc_middleware`: Circuit breaker interceptor
- Applications: Protect external service calls

## References

- Implementation: `crates/circuit-breaker/src/`
- Tests: `crates/circuit-breaker/tests/`
- Proto definitions: `proto/plexspaces/v1/circuitbreaker.proto`

