# Order Processing Microservice - Design Document

**Last Updated**: November 13, 2025
**Status**: Design Phase
**Purpose**: Demonstrate PlexSpaces microservices framework with OTP-like patterns

---

## Overview

This example demonstrates a complete e-commerce order processing system built with PlexSpaces, showcasing:

- **Elastic Actor Pools**: Auto-scaling worker pools for different services
- **Channel Abstraction**: In-process and distributed messaging
- **Service Registry**: Health-check based service discovery
- **Circuit Breaker**: Fault tolerance for external service calls
- **GenServer Patterns**: Synchronous request/reply actors
- **Supervision Trees**: Fault-tolerant actor hierarchies

## Architecture

### Microservices

```
┌─────────────────────────────────────────────────────────────────┐
│                      Order Processing System                     │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  ┌──────────────┐      ┌──────────────┐      ┌──────────────┐  │
│  │   API        │      │   Order      │      │   Payment    │  │
│  │   Gateway    │─────▶│   Service    │─────▶│   Service    │  │
│  │              │      │              │      │              │  │
│  └──────────────┘      └──────────────┘      └──────────────┘  │
│         │                      │                      │          │
│         │                      ▼                      │          │
│         │              ┌──────────────┐              │          │
│         │              │  Inventory   │              │          │
│         └─────────────▶│   Service    │◀─────────────┘          │
│                        │              │                          │
│                        └──────────────┘                          │
│                                │                                  │
│                                ▼                                  │
│                        ┌──────────────┐                          │
│                        │  Shipping    │                          │
│                        │   Service    │                          │
│                        │              │                          │
│                        └──────────────┘                          │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

### Service Responsibilities

#### 1. **API Gateway Service**
- **Purpose**: Route requests to backend services
- **Pattern**: Request router with circuit breaker
- **Features**:
  - Service discovery via ServiceRegistry
  - Circuit breaker per backend service
  - Request validation
  - Load balancing across service instances

#### 2. **Order Service**
- **Purpose**: Orchestrate order processing workflow
- **Pattern**: Workflow coordinator with elastic pool
- **Features**:
  - Elastic pool of order processor actors
  - Channel-based work queue
  - Circuit breaker for payment/inventory calls
  - GenServer pattern for order state management

#### 3. **Payment Service**
- **Purpose**: Process payments via external gateway (Stripe/PayPal)
- **Pattern**: GenServer with circuit breaker
- **Features**:
  - Elastic pool of payment processors
  - Circuit breaker for external payment API
  - Retry logic with exponential backoff
  - Idempotency keys

#### 4. **Inventory Service**
- **Purpose**: Manage product inventory and reservations
- **Pattern**: GenServer with distributed state
- **Features**:
  - Elastic pool of inventory actors
  - Channel for inventory updates
  - Circuit breaker for database calls
  - Optimistic locking for stock updates

#### 5. **Shipping Service**
- **Purpose**: Create shipments via external carrier APIs (FedEx/UPS)
- **Pattern**: GenServer with circuit breaker
- **Features**:
  - Elastic pool of shipping actors
  - Circuit breaker for carrier APIs
  - Retry logic for transient failures
  - Shipping label generation

---

## Component Integration

### 1. Channels (Queuing/Topics)

**Order Queue** (InMemory for demo, Redis for production):
```rust
// Create order processing channel
let order_channel = ChannelConfig {
    name: "order-processing-queue".to_string(),
    backend: ChannelBackend::InMemory, // Or Redis for distributed
    capacity: 1000,
    delivery: DeliveryGuarantee::AtLeastOnce,
    ordering: OrderingGuarantee::Fifo,
    backend_config: InMemoryConfig {
        backpressure: BackpressureStrategy::Block,
        send_timeout: Duration::from_secs(5),
        receive_timeout: Duration::from_secs(10),
    },
    message_ttl: Some(Duration::from_hours(1)),
    dead_letter_queue: Some("order-dlq".to_string()),
};
```

**Channels Used**:
- `order-processing-queue`: Orders waiting to be processed
- `payment-queue`: Payment requests
- `inventory-queue`: Inventory reservation requests
- `shipping-queue`: Shipping label requests
- `order-events`: Pub/sub for order state changes

### 2. Elastic Actor Pools

**Order Processor Pool**:
```rust
let order_pool_config = PoolConfig {
    name: "order-processors".to_string(),
    min_size: 5,
    max_size: 50,
    initial_size: 10,
    scaling_threshold: 0.8, // Scale up when 80% busy
    scale_down_threshold: 0.3, // Scale down when 30% busy
    idle_timeout: Duration::from_minutes(5),
    checkout_timeout: Duration::from_secs(30),
    scaling_policy: ScalingPolicy {
        strategy: ScalingStrategy::Percentage,
        scale_factor: 0.5, // Add/remove 50% of current size
        cooldown: Duration::from_minutes(2),
        min_scale_step: 2,
        max_scale_step: 10,
    },
    actor_config: ActorConfig {
        actor_type: "OrderProcessorActor".to_string(),
        restart_policy: RestartPolicy::Permanent,
        max_restarts: 5,
        restart_window: Duration::from_minutes(1),
    },
    circuit_breaker: Some(CircuitBreakerConfig { /* ... */ }),
};
```

**Pools Used**:
- `order-processors`: Process orders (5-50 actors)
- `payment-processors`: Call payment gateway (3-20 actors)
- `inventory-actors`: Manage inventory (5-30 actors)
- `shipping-actors`: Create shipments (3-15 actors)

### 3. Service Registry

**Registration**:
```rust
// Order Service registers
let registration = ServiceRegistration {
    instance_id: ulid::Ulid::new().to_string(),
    service_name: "order-service".to_string(),
    version: "1.0.0".to_string(),
    endpoint: "grpc://localhost:9001".to_string(),
    capabilities: hashmap!{
        "region" => "us-west-2",
        "env" => "production",
        "protocol" => "grpc",
    },
    health_check: HealthCheckConfig {
        interval: Duration::from_secs(10),
        timeout: Duration::from_secs(5),
        healthy_threshold: 3,
        unhealthy_threshold: 2,
    },
    ttl: Duration::from_secs(60),
};

registry.register(registration).await?;
```

**Discovery**:
```rust
// API Gateway discovers order service instances
let query = ServiceQuery {
    service_name: "order-service".to_string(),
    required_capabilities: hashmap!{"env" => "production"},
    health_states: vec![HealthState::Healthy, HealthState::Degraded],
    strategy: LoadBalancingStrategy::LeastConnections,
    max_instances: 5,
};

let instances = registry.discover(query).await?;
```

### 4. Circuit Breaker

**Payment Service Circuit Breaker**:
```rust
let payment_circuit = CircuitBreakerConfig {
    name: "payment-gateway".to_string(),
    failure_strategy: FailureStrategy::ErrorRate,
    failure_threshold: 50, // 50% error rate
    success_threshold: 3, // 3 successes to close
    timeout: Duration::from_secs(30),
    half_open_config: HalfOpenConfig {
        max_requests: 5,
        duration: Duration::from_secs(10),
        selection_strategy: SelectionStrategy::FirstN,
    },
    sliding_window: SlidingWindowConfig {
        window_size: 100,
        window_duration: Duration::from_secs(60),
        minimum_requests: 10,
    },
    request_timeout: Duration::from_secs(10),
};
```

**Usage**:
```rust
// Execute payment through circuit breaker
match circuit_breaker.execute_request("payment-gateway", request_id, timeout).await? {
    Ok(allowed) => {
        // Call payment gateway
        let result = payment_client.charge(amount).await;

        // Record result
        circuit_breaker.record_result("payment-gateway", RequestResult {
            success: result.is_ok(),
            response_time_ms: elapsed.as_millis(),
            error_message: result.err().map(|e| e.to_string()),
        }).await?;
    }
    Err(circuit_open) => {
        // Fast fail - circuit is open
        return Err(Error::ServiceUnavailable(circuit_open.message));
    }
}
```

---

## Order Processing Flow

### Workflow Steps

```
1. Client → API Gateway: POST /orders
                ↓
2. API Gateway → Order Service: CreateOrder RPC
                ↓
3. Order Service → Channel: Publish to order-processing-queue
                ↓
4. Order Processor (from pool): Checkout from queue
                ↓
5. Order Processor → Payment Service: ProcessPayment RPC
                ↓ (via Circuit Breaker)
6. Payment Service → Stripe API: Charge card
                ↓
7. Order Processor → Inventory Service: ReserveItems RPC
                ↓ (via Circuit Breaker)
8. Inventory Service → Database: Update stock
                ↓
9. Order Processor → Shipping Service: CreateShipment RPC
                ↓ (via Circuit Breaker)
10. Shipping Service → FedEx API: Create label
                ↓
11. Order Processor → Channel: Publish order-completed event
                ↓
12. Order Service → Client: Order confirmation
```

### Error Handling

**Payment Failure**:
- Circuit breaker opens if > 50% error rate
- Retry with exponential backoff (1s, 2s, 4s, 8s)
- After 5 failures, mark order as "payment-failed"
- DLQ message for manual review

**Inventory Out of Stock**:
- Return error to client immediately
- No retry (deterministic failure)
- Cancel payment authorization
- Publish "order-cancelled" event

**Shipping API Failure**:
- Circuit breaker opens if carrier API down
- Retry with backoff
- Fallback to alternate carrier if available
- If all fail, order marked "pending-shipment"

---

## Actor Definitions

### 1. OrderProcessorActor (GenServer)

```rust
pub struct OrderProcessorActor {
    order_id: String,
    state: OrderState,
    payment_client: Arc<PaymentClient>,
    inventory_client: Arc<InventoryClient>,
    shipping_client: Arc<ShippingClient>,
}

#[async_trait]
impl ActorBehavior for OrderProcessorActor {
    async fn handle_call(&mut self, request: Message, _ctx: &ActorContext) -> Result<Message> {
        match request.message_type.as_str() {
            "process_order" => self.process_order(request).await,
            "get_status" => self.get_status(request).await,
            _ => Err(Error::UnknownMessage),
        }
    }
}

impl OrderProcessorActor {
    async fn process_order(&mut self, msg: Message) -> Result<Message> {
        // 1. Validate order
        let order = self.validate_order(&msg)?;

        // 2. Process payment (via circuit breaker)
        self.state = OrderState::ProcessingPayment;
        let payment_result = self.payment_client.charge(order.amount).await?;

        // 3. Reserve inventory (via circuit breaker)
        self.state = OrderState::ReservingInventory;
        let reservation = self.inventory_client.reserve(order.items).await?;

        // 4. Create shipment (via circuit breaker)
        self.state = OrderState::CreatingShipment;
        let shipment = self.shipping_client.create_label(order.address).await?;

        // 5. Mark complete
        self.state = OrderState::Completed;

        Ok(Message::new("order_processed", order.id))
    }
}
```

### 2. PaymentServiceActor (GenServer)

```rust
pub struct PaymentServiceActor {
    payment_gateway: Arc<StripeClient>,
    circuit_breaker: Arc<CircuitBreaker>,
}

#[async_trait]
impl ActorBehavior for PaymentServiceActor {
    async fn handle_call(&mut self, request: Message, _ctx: &ActorContext) -> Result<Message> {
        match request.message_type.as_str() {
            "charge" => self.charge(request).await,
            "refund" => self.refund(request).await,
            _ => Err(Error::UnknownMessage),
        }
    }
}

impl PaymentServiceActor {
    async fn charge(&mut self, msg: Message) -> Result<Message> {
        let charge_request: ChargeRequest = decode(&msg.payload)?;

        // Execute through circuit breaker
        self.circuit_breaker.execute(
            "stripe-charge",
            async {
                self.payment_gateway
                    .create_charge(charge_request.amount, charge_request.token)
                    .await
            }
        ).await
    }
}
```

### 3. InventoryServiceActor (GenServer)

```rust
pub struct InventoryServiceActor {
    inventory_db: Arc<InventoryDatabase>,
    circuit_breaker: Arc<CircuitBreaker>,
}

#[async_trait]
impl ActorBehavior for InventoryServiceActor {
    async fn handle_call(&mut self, request: Message, _ctx: &ActorContext) -> Result<Message> {
        match request.message_type.as_str() {
            "reserve_items" => self.reserve_items(request).await,
            "release_items" => self.release_items(request).await,
            "check_stock" => self.check_stock(request).await,
            _ => Err(Error::UnknownMessage),
        }
    }
}

impl InventoryServiceActor {
    async fn reserve_items(&mut self, msg: Message) -> Result<Message> {
        let request: ReserveRequest = decode(&msg.payload)?;

        // Execute with circuit breaker (DB call)
        self.circuit_breaker.execute(
            "inventory-db",
            async {
                // Optimistic locking
                self.inventory_db
                    .reserve_with_version(request.items, request.version)
                    .await
            }
        ).await
    }
}
```

---

## Supervision Tree

```
Application Supervisor (OneForAll)
├── Service Registry Supervisor (OneForOne)
│   ├── Service Registry Actor
│   └── Health Check Actor
│
├── API Gateway Supervisor (OneForOne)
│   ├── HTTP Server Actor
│   ├── Request Router Actor
│   └── Circuit Breaker Manager
│
├── Order Service Supervisor (OneForOne)
│   ├── Order Pool Supervisor (OneForAll)
│   │   ├── Order Processor 1 (from pool)
│   │   ├── Order Processor 2 (from pool)
│   │   └── ... (5-50 actors)
│   ├── Order Channel Actor
│   └── Order Event Publisher
│
├── Payment Service Supervisor (OneForOne)
│   ├── Payment Pool Supervisor (OneForAll)
│   │   ├── Payment Processor 1 (from pool)
│   │   └── ... (3-20 actors)
│   └── Payment Circuit Breaker
│
├── Inventory Service Supervisor (OneForOne)
│   ├── Inventory Pool Supervisor (OneForAll)
│   │   ├── Inventory Actor 1 (from pool)
│   │   └── ... (5-30 actors)
│   └── Inventory Circuit Breaker
│
└── Shipping Service Supervisor (OneForOne)
    ├── Shipping Pool Supervisor (OneForAll)
    │   ├── Shipping Actor 1 (from pool)
    │   └── ... (3-15 actors)
    └── Shipping Circuit Breaker
```

---

## File Structure

```
examples/order-processing/
├── Cargo.toml
├── README.md
├── config/
│   ├── dev.toml
│   ├── production.toml
│   └── test.toml
├── src/
│   ├── main.rs                    # Application entry point
│   ├── lib.rs                     # Re-exports
│   │
│   ├── actors/
│   │   ├── mod.rs
│   │   ├── order_processor.rs     # Order processing actor
│   │   ├── payment_service.rs     # Payment service actor
│   │   ├── inventory_service.rs   # Inventory service actor
│   │   └── shipping_service.rs    # Shipping service actor
│   │
│   ├── services/
│   │   ├── mod.rs
│   │   ├── api_gateway.rs         # API gateway service
│   │   ├── order_service.rs       # Order orchestration
│   │   ├── payment_service.rs     # Payment service
│   │   ├── inventory_service.rs   # Inventory service
│   │   └── shipping_service.rs    # Shipping service
│   │
│   ├── channels/
│   │   ├── mod.rs
│   │   ├── order_queue.rs         # Order processing queue
│   │   └── event_publisher.rs     # Event pub/sub
│   │
│   ├── pools/
│   │   ├── mod.rs
│   │   ├── order_pool.rs          # Order processor pool
│   │   ├── payment_pool.rs        # Payment processor pool
│   │   ├── inventory_pool.rs      # Inventory actor pool
│   │   └── shipping_pool.rs       # Shipping actor pool
│   │
│   ├── circuit_breakers/
│   │   ├── mod.rs
│   │   ├── payment_breaker.rs     # Payment gateway breaker
│   │   ├── inventory_breaker.rs   # Inventory DB breaker
│   │   └── shipping_breaker.rs    # Carrier API breaker
│   │
│   ├── registry/
│   │   ├── mod.rs
│   │   └── service_registry.rs    # Service discovery
│   │
│   ├── types/
│   │   ├── mod.rs
│   │   ├── order.rs               # Order types
│   │   ├── payment.rs             # Payment types
│   │   ├── inventory.rs           # Inventory types
│   │   └── shipping.rs            # Shipping types
│   │
│   └── application.rs             # Application supervisor
│
├── tests/
│   ├── integration/
│   │   ├── order_flow_test.rs     # End-to-end order flow
│   │   ├── circuit_breaker_test.rs# Circuit breaker scenarios
│   │   ├── pool_scaling_test.rs   # Auto-scaling tests
│   │   └── service_discovery_test.rs
│   │
│   └── unit/
│       ├── order_processor_test.rs
│       ├── payment_service_test.rs
│       ├── inventory_service_test.rs
│       └── shipping_service_test.rs
│
└── docker/
    ├── Dockerfile
    ├── docker-compose.yml         # Full stack deployment
    └── docker-compose.dev.yml     # Development stack
```

---

## Configuration Example

**config/production.toml**:
```toml
[application]
name = "order-processing"
version = "1.0.0"

[node]
id = "order-node-1"
listen_address = "0.0.0.0:9001"

# Service Registry
[services.registry]
endpoint = "grpc://service-registry:9000"
heartbeat_interval = "10s"
ttl = "60s"

# Order Service
[services.order]
enabled = true
[services.order.pool]
min_size = 5
max_size = 50
initial_size = 10
scaling_threshold = 0.8
scale_down_threshold = 0.3

[services.order.channel]
backend = "redis"  # Or "in_memory" for dev
redis_url = "redis://redis:6379"
capacity = 1000

# Payment Service
[services.payment]
enabled = true
[services.payment.pool]
min_size = 3
max_size = 20

[services.payment.circuit_breaker]
failure_threshold = 50
success_threshold = 3
timeout = "30s"

[services.payment.stripe]
api_key_env = "STRIPE_API_KEY"
webhook_secret_env = "STRIPE_WEBHOOK_SECRET"

# Inventory Service
[services.inventory]
enabled = true
[services.inventory.pool]
min_size = 5
max_size = 30

[services.inventory.database]
url_env = "INVENTORY_DB_URL"
max_connections = 20

# Shipping Service
[services.shipping]
enabled = true
[services.shipping.pool]
min_size = 3
max_size = 15

[services.shipping.carriers]
fedex_api_key_env = "FEDEX_API_KEY"
ups_api_key_env = "UPS_API_KEY"
```

---

## Testing Strategy

### Unit Tests
- Individual actor behaviors
- Circuit breaker state transitions
- Pool scaling logic
- Service registry operations

### Integration Tests
- Full order processing flow
- Circuit breaker failover scenarios
- Pool auto-scaling under load
- Service discovery and health checks
- Channel message delivery guarantees

### Load Tests
- Process 1000 orders/second
- Measure pool scaling behavior
- Circuit breaker under high error rates
- Service registry under churn

### Chaos Tests
- Kill random actors mid-processing
- Network partitions between services
- External API failures
- Database connection failures

---

## Success Criteria

1. ✅ **Throughput**: Process 1000 orders/second
2. ✅ **Latency**: P95 < 500ms end-to-end
3. ✅ **Availability**: 99.9% uptime
4. ✅ **Fault Tolerance**: Survive 2 service failures
5. ✅ **Auto-Scaling**: Scale from 5 to 50 actors in < 30s
6. ✅ **Recovery**: Circuit breaker opens/closes automatically
7. ✅ **Discovery**: Services find each other via registry
8. ✅ **Observability**: Full metrics and tracing

---

## Next Steps

1. Implement Channel abstraction (in-process backend first)
2. Implement ElasticPool with auto-scaling
3. Implement ServiceRegistry with health checks
4. Implement CircuitBreaker pattern
5. Create order-processing actors (OrderProcessor, PaymentService, etc.)
6. Wire up supervision trees
7. Add integration tests
8. Docker deployment
9. Load testing
10. Production hardening
