# PlexSpaces Usage Guide

This guide provides practical examples and patterns for using PlexSpaces in production.

## Table of Contents

1. [Basic Usage](#basic-usage)
2. [Security and Tenant Isolation](#security-and-tenant-isolation)
3. [RequestContext Patterns](#requestcontext-patterns)
4. [Repository and Service Patterns](#repository-and-service-patterns)
5. [Configuration Management](#configuration-management)
6. [Best Practices](#best-practices)

## Basic Usage

### Creating a Node

```rust
use plexspaces_node::NodeBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a node with configuration
    let node = NodeBuilder::new()
        .with_node_id("node1".to_string())
        .with_listen_address("0.0.0.0:9001".to_string())
        .build()
        .await?;
    
    // Start the node
    node.start().await?;
    
    Ok(())
}
```

### Creating and Using Actors

```rust
use plexspaces::*;
use plexspaces_behavior::GenServerBehavior;
use plexspaces_core::RequestContext;

struct Counter {
    count: i32,
}

#[async_trait]
impl GenServerBehavior for Counter {
    type Request = CounterRequest;
    type Reply = i32;

    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        request: Self::Request,
    ) -> Result<Self::Reply, BehaviorError> {
        match request {
            CounterRequest::Increment(amount) => {
                self.count += amount;
                Ok(self.count)
            }
            CounterRequest::Get => Ok(self.count),
        }
    }
}

enum CounterRequest {
    Increment(i32),
    Get,
}

// Usage
let node = PlexSpacesNode::new("node1".to_string()).await?;
let actor_id = "counter@node1".to_string();
let counter = Counter { count: 0 };
// Spawn using ActorFactory
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use std::sync::Arc;

let behavior = Box::new(counter);
let mut mailbox_config = mailbox_config_default();
mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id)).await?;
let actor = Actor::new(actor_id.clone(), behavior, mailbox, "default".to_string(), None);

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await?;

let actor_ref = node.get_actor_ref(&actor_id).await?;
let reply = actor_ref.ask(CounterRequest::Get, Duration::from_secs(5)).await?;
```

## Security and Tenant Isolation

### RequestContext Usage

All operations in PlexSpaces require a `RequestContext` for tenant isolation:

```rust
use plexspaces_core::RequestContext;

// Create context from JWT claims (typically done in middleware)
let ctx = RequestContext::new("tenant-123".to_string())
    .with_namespace("production".to_string())
    .with_user_id("user-456".to_string())
    .with_correlation_id("corr-789".to_string());

// Pass context to services/repositories
let result = blob_service.upload_blob(
    &ctx,
    "my-file.txt",
    data,
    Some("text/plain".to_string()),
    None, None, HashMap::new(), HashMap::new(), None
).await?;
```

### JWT Middleware Integration

The JWT middleware automatically extracts tenant_id from JWT claims:

```rust
// JWT token contains:
// {
//   "sub": "user-123",
//   "tenant_id": "tenant-456",
//   "roles": ["admin", "user"]
// }

// Middleware extracts tenant_id and creates RequestContext
// All subsequent operations are automatically scoped to tenant-456
```

### Repository Pattern with Tenant Isolation

All repository methods require RequestContext:

```rust
use plexspaces_blob::BlobRepository;
use plexspaces_core::RequestContext;

// Get blob (automatically filtered by tenant_id)
let metadata = repository.get(&ctx, "blob-123").await?;

// List blobs (only returns blobs for tenant in context)
let (blobs, total) = repository.list(&ctx, &filters, 10, 0).await?;

// Save blob (validates tenant_id matches context)
repository.save(&ctx, &metadata).await?;
```

### Service Layer with Tenant Isolation

All service methods require RequestContext:

```rust
use plexspaces_blob::BlobService;
use plexspaces_core::RequestContext;

// Upload blob (tenant_id from context)
let metadata = blob_service.upload_blob(
    &ctx,
    "file.txt",
    data,
    None, None, None, HashMap::new(), HashMap::new(), None
).await?;

// Download blob (only if belongs to tenant in context)
let data = blob_service.download_blob(&ctx, &metadata.blob_id).await?;

// List blobs (only for tenant in context)
let (blobs, total) = blob_service.list_blobs(
    &ctx,
    &ListFilters::default(),
    10,
    1
).await?;
```

## RequestContext Patterns

### Creating RequestContext

```rust
use plexspaces_core::RequestContext;

// Basic context (required tenant_id)
let ctx = RequestContext::new("tenant-123".to_string());

// With namespace
let ctx = RequestContext::new("tenant-123".to_string())
    .with_namespace("production".to_string());

// With user information
let ctx = RequestContext::new("tenant-123".to_string())
    .with_namespace("production".to_string())
    .with_user_id("user-456".to_string())
    .with_correlation_id("corr-789".to_string());

// With metadata
let ctx = RequestContext::new("tenant-123".to_string())
    .with_metadata("source".to_string(), "api".to_string())
    .with_metadata("version".to_string(), "v1".to_string());
```

### Extracting from gRPC Metadata

```rust
use tonic::Request;
use plexspaces_core::RequestContext;

fn extract_context_from_request<T>(request: &Request<T>) -> Result<RequestContext, String> {
    let metadata = request.metadata();
    
    // Extract tenant_id from headers (set by JWT middleware)
    let tenant_id = metadata.get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| "Missing x-tenant-id header".to_string())?;
    
    let namespace = metadata.get("x-namespace")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default");
    
    let user_id = metadata.get("x-user-id")
        .and_then(|v| v.to_str().ok());
    
    let mut ctx = RequestContext::new(tenant_id.to_string())
        .with_namespace(namespace.to_string());
    
    if let Some(uid) = user_id {
        ctx = ctx.with_user_id(uid.to_string());
    }
    
    Ok(ctx)
}
```

### Converting to/from Proto

```rust
use plexspaces_core::RequestContext;
use plexspaces_proto::v1::common::RequestContext as ProtoRequestContext;

// Convert to proto (for gRPC)
let proto_ctx = ctx.to_proto();

// Convert from proto
let ctx = RequestContext::from_proto(&proto_ctx)?;
```

## Repository and Service Patterns

### Repository Pattern

All repositories follow the same pattern:

```rust
#[async_trait]
pub trait MyRepository: Send + Sync {
    async fn get(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        id: &str,
    ) -> Result<Option<MyEntity>>;
    
    async fn save(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        entity: &MyEntity,
    ) -> Result<()>;
    
    async fn list(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        filters: &ListFilters,
        page_size: i64,
        offset: i64,
    ) -> Result<(Vec<MyEntity>, i64)>;
}
```

### SQL Query Pattern

All SQL queries must filter by tenant_id:

```rust
// ✅ Correct: Filters by tenant_id and namespace
sqlx::query("SELECT * FROM my_table WHERE id = $1 AND tenant_id = $2 AND namespace = $3")
    .bind(id)
    .bind(ctx.tenant_id())
    .bind(ctx.namespace())
    .fetch_optional(&*self.pool)
    .await

// ❌ Wrong: Missing tenant_id filter
sqlx::query("SELECT * FROM my_table WHERE id = $1")
    .bind(id)
    .fetch_optional(&*self.pool)
    .await
```

### Service Layer Pattern

All services follow the same pattern:

```rust
impl MyService {
    pub async fn create_entity(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        name: &str,
        data: Vec<u8>,
    ) -> Result<MyEntity> {
        // Validate tenant_id
        // Create entity with tenant_id from context
        let entity = MyEntity {
            id: generate_id(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
            name: name.to_string(),
            data,
        };
        
        // Save via repository (validates tenant_id)
        self.repository.save(ctx, &entity).await?;
        
        Ok(entity)
    }
    
    pub async fn get_entity(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        id: &str,
    ) -> Result<MyEntity> {
        // Repository automatically filters by tenant_id
        self.repository.get(ctx, id).await?
            .ok_or_else(|| MyError::NotFound(id.to_string()))
    }
}
```

## Configuration Management

### Loading Configuration

```rust
use plexspaces_node::config_loader::ConfigLoader;

// Load from YAML file with environment variable substitution
let loader = ConfigLoader::new();
let spec = loader.load_release_spec_with_env_precedence("config/release.yaml").await?;
```

### Environment Variable Precedence

Environment variables override file configuration:

```bash
# Set environment variables
export PLEXSPACES_NODE_ID=node1
export PLEXSPACES_GRPC_ADDRESS=0.0.0.0:9001
export PLEXSPACES_JWT_SECRET=my-secret-key

# Load config (env vars override file values)
let spec = loader.load_release_spec_with_env_precedence("config/release.yaml").await?;
```

### Environment Variable Substitution

Use `${VAR_NAME}` or `${VAR_NAME:-default}` in YAML:

```yaml
node:
  id: "${PLEXSPACES_NODE_ID:-node1}"
  listen_address: "${PLEXSPACES_LISTEN_ADDR:-0.0.0.0:9001}"

runtime:
  security:
    jwt:
      secret: "${JWT_SECRET}"  # Required - no default
      issuer: "${JWT_ISSUER:-https://auth.example.com}"
```

### Security Validation

The config loader validates that secrets are not hardcoded:

```rust
// ❌ This will fail validation:
// secret: "hardcoded-secret-key"

// ✅ This passes validation:
// secret: "${JWT_SECRET}"
```

## Best Practices

### 1. Always Use RequestContext

**✅ Good:**
```rust
async fn my_function(ctx: &RequestContext, id: &str) -> Result<()> {
    let entity = repository.get(ctx, id).await?;
    // ...
}
```

**❌ Bad:**
```rust
async fn my_function(tenant_id: &str, id: &str) -> Result<()> {
    let entity = repository.get_by_tenant(tenant_id, id).await?;
    // ...
}
```

### 2. Validate Tenant ID at Service Boundaries

**✅ Good:**
```rust
async fn save(&self, ctx: &RequestContext, entity: &MyEntity) -> Result<()> {
    // Validate tenant_id matches context
    if entity.tenant_id != ctx.tenant_id() {
        return Err(MyError::InvalidTenant);
    }
    // ...
}
```

### 3. Always Filter Queries by Tenant ID

**✅ Good:**
```rust
sqlx::query("SELECT * FROM table WHERE id = $1 AND tenant_id = $2")
    .bind(id)
    .bind(ctx.tenant_id())
```

**❌ Bad:**
```rust
sqlx::query("SELECT * FROM table WHERE id = $1")
    .bind(id)
```

### 4. Use Environment Variables for Secrets

**✅ Good:**
```yaml
runtime:
  security:
    jwt:
      secret: "${JWT_SECRET}"  # From environment
```

**❌ Bad:**
```yaml
runtime:
  security:
    jwt:
      secret: "hardcoded-secret"  # Will fail validation
```

### 5. Propagate RequestContext Through Call Chain

**✅ Good:**
```rust
async fn handler(ctx: &RequestContext, request: Request) -> Result<Response> {
    let result = service.process(ctx, request.data).await?;
    let entity = repository.save(ctx, &result).await?;
    Ok(Response::from(entity))
}
```

### 6. Log Tenant ID for Audit

**✅ Good:**
```rust
tracing::info!(
    tenant_id = %ctx.tenant_id(),
    namespace = %ctx.namespace(),
    user_id = ?ctx.user_id(),
    "Operation completed"
);
```

### 7. Handle Missing Tenant ID Gracefully

**✅ Good:**
```rust
let tenant_id = metadata.get("x-tenant-id")
    .and_then(|v| v.to_str().ok())
    .ok_or_else(|| Error::MissingTenantId)?;
```

## Security Best Practices

### 1. Never Bypass Tenant Isolation

**❌ Never do this:**
```rust
// Bypassing tenant isolation
let all_data = sqlx::query("SELECT * FROM table").fetch_all(&*pool).await?;
```

### 2. Validate Tenant ID at Every Boundary

```rust
// At service boundary
if entity.tenant_id != ctx.tenant_id() {
    return Err(Error::Unauthorized);
}

// At repository boundary
if metadata.tenant_id != ctx.tenant_id() {
    return Err(Error::InvalidTenant);
}
```

### 3. Use Parameterized Queries

**✅ Good:**
```rust
sqlx::query("SELECT * FROM table WHERE id = $1 AND tenant_id = $2")
    .bind(id)
    .bind(ctx.tenant_id())
```

**❌ Bad:**
```rust
let query = format!("SELECT * FROM table WHERE id = '{}' AND tenant_id = '{}'", id, ctx.tenant_id());
sqlx::query(&query)  // SQL injection risk
```

### 4. Never Log Sensitive Data

**✅ Good:**
```rust
tracing::info!(
    tenant_id = %ctx.tenant_id(),
    blob_id = %blob_id,
    "Blob accessed"
);
```

**❌ Bad:**
```rust
tracing::info!("Blob data: {:?}", blob_data);  // Don't log sensitive data
```

## Error Handling

### Tenant Isolation Errors

```rust
use plexspaces_core::RequestContext;

// Repository returns None if tenant doesn't match
let result = repository.get(&ctx, "blob-123").await?;
match result {
    Some(blob) => Ok(blob),
    None => Err(BlobError::NotFound("blob-123".to_string())),
}

// Service validates tenant_id
if metadata.tenant_id != ctx.tenant_id() {
    return Err(BlobError::InvalidInput(
        "Metadata tenant_id does not match context".to_string()
    ));
}
```

## Examples

### Complete Example: Blob Service with Tenant Isolation

```rust
use plexspaces_blob::{BlobService, BlobRepository};
use plexspaces_core::RequestContext;
use std::collections::HashMap;

async fn upload_file(
    blob_service: &BlobService,
    ctx: &RequestContext,
    filename: &str,
    data: Vec<u8>,
) -> Result<String> {
    // Upload blob (tenant_id from context)
    let metadata = blob_service.upload_blob(
        ctx,
        filename,
        data,
        Some("application/octet-stream".to_string()),
        None,  // blob_group
        None,  // kind
        HashMap::new(),  // metadata
        HashMap::new(),  // tags
        None,  // expires_after
    ).await?;
    
    Ok(metadata.blob_id)
}

async fn download_file(
    blob_service: &BlobService,
    ctx: &RequestContext,
    blob_id: &str,
) -> Result<Vec<u8>> {
    // Download blob (automatically filtered by tenant_id)
    blob_service.download_blob(ctx, blob_id).await
}

async fn list_files(
    blob_service: &BlobService,
    ctx: &RequestContext,
    page: i64,
) -> Result<(Vec<BlobMetadata>, i64)> {
    // List blobs (only for tenant in context)
    blob_service.list_blobs(
        ctx,
        &ListFilters::default(),
        10,  // page_size
        page,
    ).await
}
```

## Related Documentation

- [Security Guide](security.md): Comprehensive security configuration and best practices
- [Installation Guide](installation.md): Setup and deployment instructions
- [Getting Started](getting-started.md): Quick start guide
- [Concepts Guide](concepts.md): Core concepts and architecture
