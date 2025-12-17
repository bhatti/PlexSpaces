# PlexSpaces Security Guide

## Overview

PlexSpaces provides comprehensive security features for production deployments, including:

- **Node-to-Node Authentication**: Mutual TLS (mTLS) for secure inter-node communication
- **User API Authentication**: JWT-based authentication for user-facing APIs
- **Tenant Isolation**: Mandatory tenant isolation for all operations
- **Security Validation**: Automatic validation that secrets are not hardcoded in config files

## Security Architecture

### Defense in Depth

PlexSpaces uses multiple layers of security to prevent unauthorized access:

1. **Transport Layer Security (mTLS)**: Prevents man-in-the-middle attacks and ensures node identity
2. **Authentication (JWT)**: Verifies user identity and extracts tenant context
3. **Authorization (Tenant Isolation)**: Enforces data access boundaries
4. **Query Filtering**: Database-level enforcement of tenant boundaries
5. **Role-Based Access Control**: Fine-grained permissions based on user roles
6. **Request Context**: Propagates security context through the entire call chain

### Authentication vs Authorization

- **Authentication (AuthN)**: Verifies identity (who you are)
  - Node-to-node: mTLS certificates
  - User APIs: JWT tokens
- **Authorization (AuthZ)**: Verifies permissions (what you can do)
  - Tenant isolation enforced at repository/service layer
  - All queries filtered by tenant_id
  - Role-based access control (RBAC) for fine-grained permissions

### Tenant Isolation

Tenant isolation is **mandatory** in PlexSpaces. All operations require a `tenant_id`:

- **RequestContext**: Go-style context carries tenant_id through call chain
- **Repository Pattern**: All repository methods require RequestContext
- **Service Layer**: All service methods require RequestContext
- **SQL Queries**: All queries automatically filter by tenant_id

## Node-to-Node Authentication (mTLS)

### Overview

mTLS provides mutual authentication between nodes in the cluster. Each node has:
- A certificate signed by a CA
- A private key
- The CA certificate for verifying other nodes

### Configuration

Configure mTLS in your `release.yaml`:

```yaml
runtime:
  security:
    mtls:
      enable_mtls: true
      auto_generate: true  # Auto-generate certs for local dev
      cert_dir: "/app/certs"  # Directory for auto-generated certs
      # Or specify paths for production:
      # ca_certificate_path: "/certs/ca.crt"
      # server_certificate_path: "/certs/server.crt"
      # server_key_path: "/certs/server.key"
```

### Certificate Generation

For local development, PlexSpaces can auto-generate certificates:

```rust
use plexspaces_grpc_middleware::cert_gen;

// Auto-generate CA and server certificates
let certs = cert_gen::generate_certificates(
    "/app/certs",
    "node-1",
    vec!["localhost".to_string()],
).await?;
```

### Certificate Rotation

Configure certificate rotation interval:

```yaml
runtime:
  security:
    mtls:
      certificate_rotation_interval: "720h"  # Rotate every 30 days
```

### Object Registry Integration

Nodes register their public certificates in the object-registry:

1. Node generates certificate
2. Node registers with object-registry (includes public cert)
3. Other nodes fetch certificate for mTLS verification
4. Certificate rotation updates registry entry

### Production Setup

For production, use proper CA-signed certificates:

1. **Generate CA**:
   ```bash
   openssl genrsa -out ca.key 4096
   openssl req -new -x509 -days 365 -key ca.key -out ca.crt
   ```

2. **Generate Server Certificate**:
   ```bash
   openssl genrsa -out server.key 4096
   openssl req -new -key server.key -out server.csr
   openssl x509 -req -days 365 -in server.csr -CA ca.crt -CAkey ca.key -out server.crt
   ```

3. **Configure in release.yaml**:
   ```yaml
   runtime:
     security:
       mtls:
         enable_mtls: true
         auto_generate: false
         ca_certificate_path: "/certs/ca.crt"
         server_certificate_path: "/certs/server.crt"
         server_key_path: "/certs/server.key"
   ```

## User API Authentication (JWT)

### Overview

JWT authentication provides secure access to user-facing APIs. The JWT middleware:

- Validates JWT tokens
- Extracts tenant_id from claims
- Adds tenant_id to request headers
- Propagates tenant_id via RequestContext

### Configuration

Configure JWT in your `release.yaml`:

```yaml
runtime:
  security:
    jwt:
      enable_jwt: true
      secret: "${JWT_SECRET}"  # MUST be env var, never hardcoded
      issuer: "https://auth.example.com"
      jwks_url: "https://auth.example.com/.well-known/jwks.json"
      allowed_audiences:
        - "plexspaces-api"
      tenant_id_claim: "tenant_id"  # JWT claim name for tenant_id
      user_id_claim: "sub"  # JWT claim name for user_id
```

### Secret Management

**CRITICAL**: Secrets must be in environment variables, never in config files.

```bash
# Set JWT secret via env var
export JWT_SECRET="your-secret-key-here"

# Config file uses env var reference
# secret: "${JWT_SECRET}"  # ✅ Correct
# secret: "hardcoded-secret"  # ❌ WRONG - will fail validation
```

The config loader validates that secrets are not hardcoded:

```rust
// This will fail validation:
let config = ConfigLoader::new()
    .load_release_spec("config.yaml").await?;
// Error: "JWT secret must be an environment variable reference"
```

### Tenant Extraction

The JWT middleware extracts tenant_id from JWT claims:

```json
{
  "sub": "user-123",
  "tenant_id": "tenant-456",
  "roles": ["admin", "user"]
}
```

The middleware adds headers:
- `x-tenant-id`: Extracted tenant_id
- `x-user-id`: User ID from "sub" claim
- `x-user-roles`: Comma-separated roles

### Token Validation

JWT tokens are validated against:

1. **Secret** (for symmetric signing)
2. **JWKS URL** (for asymmetric signing with public keys)
3. **Issuer** (must match configured issuer)
4. **Audience** (must be in allowed_audiences)
5. **Expiration** (must not be expired)

### Example JWT Token

```json
{
  "sub": "user-123",
  "tenant_id": "tenant-456",
  "iss": "https://auth.example.com",
  "aud": "plexspaces-api",
  "exp": 1735689600,
  "iat": 1735603200
}
```

## Tenant Isolation

### RequestContext

All operations require a `RequestContext`:

```rust
use plexspaces_core::RequestContext;

// Create context from request
let ctx = RequestContext::new("tenant-123".to_string())
    .with_namespace("production".to_string())
    .with_user_id("user-456".to_string());

// Pass to repository/service
let result = repository.get(&ctx, "resource-id").await?;
```

### Repository Pattern

All repository methods require RequestContext:

```rust
#[async_trait]
pub trait BlobRepository {
    async fn get(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        blob_id: &str,
    ) -> BlobResult<Option<BlobMetadata>>;
}
```

### Service Layer

All service methods require RequestContext:

```rust
impl BlobService {
    pub async fn upload_blob(
        &self,
        ctx: &RequestContext,  // Required for tenant isolation
        name: &str,
        data: Vec<u8>,
        // ...
    ) -> BlobResult<BlobMetadata>;
}
```

### SQL Query Patterns

All SQL queries automatically filter by tenant_id:

```sql
-- ✅ Correct: Filters by tenant_id and namespace
SELECT * FROM blob_metadata
WHERE blob_id = $1 AND tenant_id = $2 AND namespace = $3

-- ❌ Wrong: Missing tenant_id filter
SELECT * FROM blob_metadata
WHERE blob_id = $1
```

### Tenant Validation

Services validate tenant_id matches:

```rust
// Repository validates tenant_id matches context
if metadata.tenant_id != ctx.tenant_id() {
    return Err(BlobError::InvalidInput(
        "Metadata tenant_id does not match context"
    ));
}
```

## Local Testing

### Disabling Auth for Tests

For integration tests, you can disable auth:

```yaml
# docker-compose.test.yml
services:
  plexspaces-node:
    environment:
      - PLEXSPACES_ALLOW_DISABLE_AUTH=true
      - PLEXSPACES_DISABLE_AUTH=true  # Only for integration tests
```

**WARNING**: Never disable auth in production!

### Integration Test Setup

```rust
use plexspaces_core::RequestContext;

#[tokio::test]
async fn test_blob_operations() {
    // Create test context
    let ctx = RequestContext::new("test-tenant".to_string())
        .with_namespace("test".to_string());
    
    // Test operations
    let metadata = blob_service.upload_blob(
        &ctx,
        "test.txt",
        b"test data".to_vec(),
        None, None, None, HashMap::new(), HashMap::new(), None
    ).await.unwrap();
    
    // Verify tenant isolation
    let data = blob_service.download_blob(&ctx, &metadata.blob_id).await.unwrap();
    assert_eq!(data, b"test data");
}
```

### Security Test Examples

```rust
#[tokio::test]
async fn test_tenant_isolation() {
    let ctx1 = RequestContext::new("tenant-1".to_string());
    let ctx2 = RequestContext::new("tenant-2".to_string());
    
    // Upload blob for tenant-1
    let metadata = blob_service.upload_blob(
        &ctx1, "test.txt", b"data".to_vec(),
        None, None, None, HashMap::new(), HashMap::new(), None
    ).await.unwrap();
    
    // Try to access from tenant-2 (should fail)
    let result = blob_service.download_blob(&ctx2, &metadata.blob_id).await;
    assert!(result.is_err()); // Should be NotFound or AccessDenied
}
```

## Best Practices

### Secret Management

1. **Never hardcode secrets** in config files
2. **Use environment variables** for all secrets
3. **Validate secrets** are not in config files (automatic)
4. **Rotate secrets** regularly
5. **Use secret management** (Vault, AWS Secrets Manager, etc.) in production

### Certificate Rotation

1. **Set rotation interval** in config
2. **Monitor certificate expiration**
3. **Update object-registry** when rotating
4. **Graceful rotation** (old certs valid during transition)

### Multi-Tenant Security

1. **Always use RequestContext** - never bypass tenant isolation
2. **Validate tenant_id** at service boundaries
3. **Filter by tenant_id** in all SQL queries
4. **Audit tenant access** (log tenant_id in all operations)
5. **Test tenant isolation** in integration tests

### Audit Logging

All operations should log tenant_id:

```rust
tracing::info!(
    tenant_id = %ctx.tenant_id(),
    namespace = %ctx.namespace(),
    blob_id = %blob_id,
    "Blob accessed"
);
```

### Security Headers

The JWT middleware adds security headers:

- `x-tenant-id`: Tenant identifier
- `x-user-id`: User identifier
- `x-user-roles`: User roles
- `x-request-id`: Request ID for tracing

### Error Handling

Security errors should not leak information:

```rust
// ✅ Good: Generic error message
return Err(BlobError::NotFound(blob_id));

// ❌ Bad: Leaks tenant information
return Err(BlobError::InvalidInput(
    format!("Blob belongs to tenant-{} but you are tenant-{}", ...)
));
```

## Environment Variables

### Security Variables

```bash
# mTLS
PLEXSPACES_MTLS_ENABLED=true
PLEXSPACES_MTLS_CA_CERT_PATH=/certs/ca.crt
PLEXSPACES_MTLS_SERVER_CERT_PATH=/certs/server.crt
PLEXSPACES_MTLS_SERVER_KEY_PATH=/certs/server.key
PLEXSPACES_MTLS_AUTO_GENERATE=true
PLEXSPACES_MTLS_CERT_DIR=/app/certs

# JWT
PLEXSPACES_JWT_ENABLED=true
PLEXSPACES_JWT_SECRET=...  # Secret - env var only
PLEXSPACES_JWT_ISSUER=https://auth.example.com
PLEXSPACES_JWT_JWKS_URL=https://auth.example.com/.well-known/jwks.json
PLEXSPACES_JWT_TENANT_ID_CLAIM=tenant_id

# Security
PLEXSPACES_ALLOW_DISABLE_AUTH=false  # Only for local testing
PLEXSPACES_DISABLE_AUTH=false  # Only if allow_disable_auth=true
```

## Preventing Unauthorized Access

PlexSpaces uses a multi-layered security approach to prevent unauthorized access:

### 1. mTLS: Preventing Node Impersonation

**Problem**: An attacker could impersonate a legitimate node in the cluster.

**Solution**: Mutual TLS (mTLS) ensures both client and server authenticate each other.

```rust
// Node-to-node communication requires valid certificate
// Without valid certificate, connection is rejected
let client = TlsConnector::builder()
    .add_root_certificate(ca_cert)
    .client_identity(server_identity)
    .build()?;
```

**How it works**:
- Each node has a unique certificate signed by a trusted CA
- Before establishing connection, both nodes verify each other's certificates
- Invalid or expired certificates result in connection rejection
- Certificate rotation ensures compromised certificates are quickly replaced

**Prevents**:
- ✅ Node impersonation attacks
- ✅ Man-in-the-middle attacks
- ✅ Unauthorized cluster access

### 2. JWT: Verifying User Identity

**Problem**: An attacker could send requests without proper authentication.

**Solution**: JWT tokens verify user identity and extract tenant context.

```rust
// JWT middleware validates token and extracts tenant_id
let claims = jwt::decode(&token, &decoding_key, &validation)?;
let tenant_id = claims.claims.get("tenant_id")
    .ok_or(AuthError::MissingTenantId)?;
```

**How it works**:
- User authenticates with identity provider (OAuth2/OIDC)
- Identity provider issues JWT token with tenant_id claim
- PlexSpaces validates JWT signature and expiration
- Tenant_id extracted from claims and added to RequestContext
- Invalid tokens result in 401 Unauthorized

**Prevents**:
- ✅ Unauthenticated requests
- ✅ Token tampering (signature validation)
- ✅ Expired token reuse (expiration check)

### 3. RequestContext: Propagating Security Context

**Problem**: Tenant context could be lost or tampered with during request processing.

**Solution**: RequestContext carries tenant_id through the entire call chain.

```rust
// RequestContext created from JWT claims
let ctx = RequestContext::new(tenant_id)
    .with_user_id(user_id)
    .with_namespace(namespace);

// Context passed to all service/repository methods
let result = blob_service.upload_blob(&ctx, name, data).await?;
```

**How it works**:
- RequestContext created at API boundary (from JWT)
- Context is immutable and passed by reference
- All service methods require RequestContext
- Context cannot be modified mid-request
- Missing context results in compilation error (type safety)

**Prevents**:
- ✅ Tenant context loss
- ✅ Context tampering (immutable)
- ✅ Missing tenant_id (compile-time check)

### 4. Narrowed Queries: Database-Level Enforcement

**Problem**: An attacker could bypass application-level checks and access other tenants' data.

**Solution**: All SQL queries automatically filter by tenant_id and namespace.

```rust
// Repository method automatically filters by tenant_id
async fn get(&self, ctx: &RequestContext, blob_id: &str) -> Result<Option<BlobMetadata>> {
    // Query ALWAYS includes tenant_id and namespace filters
    sqlx::query("SELECT * FROM blob_metadata WHERE blob_id = $1 AND tenant_id = $2 AND namespace = $3")
        .bind(blob_id)
        .bind(ctx.tenant_id())  // From RequestContext
        .bind(ctx.namespace())  // From RequestContext
        .fetch_optional(&*self.pool)
        .await
}
```

**How it works**:
- All repository methods require RequestContext
- SQL queries ALWAYS include `WHERE tenant_id = $X AND namespace = $Y`
- Database enforces tenant boundaries (even if application code has bugs)
- No way to bypass tenant filtering (enforced at type level)

**Prevents**:
- ✅ Cross-tenant data access
- ✅ SQL injection attacks (parameterized queries)
- ✅ Bypassing application-level checks

### 5. Role-Based Access Control (RBAC)

**Problem**: Users within a tenant might need different permission levels.

**Solution**: JWT claims include roles, which are validated at service boundaries.

```rust
// JWT middleware extracts roles from claims
let roles: Vec<String> = claims.claims.get("roles")
    .and_then(|v| serde_json::from_value(v.clone()).ok())
    .unwrap_or_default();

// Roles added to headers for service layer
headers.insert("x-user-roles", roles.join(","));
```

**How it works**:
- Roles extracted from JWT claims (e.g., `["admin", "user"]`)
- Roles added to RequestContext metadata
- Service layer validates roles before operations
- Different roles have different permissions

**Example**:
```rust
// Service method checks role
if !ctx.has_metadata("roles") || !ctx.get_metadata("roles").unwrap().contains("admin") {
    return Err(BlobError::Unauthorized("Admin role required"));
}
```

**Prevents**:
- ✅ Unauthorized operations (e.g., delete requires admin)
- ✅ Privilege escalation
- ✅ Unauthorized data modification

### 6. Complete Security Flow

Here's how all layers work together to prevent unauthorized access:

```
1. User Request
   ↓
2. mTLS Handshake (if node-to-node)
   - Validates node certificate
   - Establishes encrypted connection
   ↓
3. JWT Validation (if user API)
   - Validates token signature
   - Checks expiration
   - Extracts tenant_id, user_id, roles
   ↓
4. RequestContext Creation
   - tenant_id from JWT (required)
   - user_id from JWT (optional)
   - roles from JWT (optional)
   - request_id generated (ULID)
   ↓
5. Service Layer
   - Validates RequestContext
   - Checks roles (if needed)
   - Passes context to repository
   ↓
6. Repository Layer
   - All queries filter by tenant_id
   - Database enforces boundaries
   - Returns only tenant's data
   ↓
7. Response
   - Data scoped to tenant
   - Audit log includes tenant_id
```

### Security Guarantees

With all layers in place, PlexSpaces guarantees:

1. **No Unauthenticated Access**: All requests require valid JWT (user APIs) or mTLS (node-to-node)
2. **No Cross-Tenant Access**: Database queries always filter by tenant_id
3. **No Context Tampering**: RequestContext is immutable and type-checked
4. **No Privilege Escalation**: Roles validated at service boundaries
5. **No SQL Injection**: All queries use parameterized statements
6. **Audit Trail**: All operations log tenant_id for compliance

### Example: Unauthorized Access Attempt

Let's trace what happens when an attacker tries to access another tenant's data:

```rust
// Attacker sends request with tenant-1 JWT
let attacker_ctx = RequestContext::new("tenant-1".to_string());

// Attacker tries to access tenant-2's blob
let blob_id = "blob-123";  // Belongs to tenant-2

// Repository query automatically filters by tenant_id
let result = repository.get(&attacker_ctx, blob_id).await?;
// SQL: SELECT * FROM blob_metadata 
//      WHERE blob_id = 'blob-123' 
//      AND tenant_id = 'tenant-1'  ← From RequestContext
//      AND namespace = 'default'
// Result: None (blob belongs to tenant-2, not tenant-1)

// Attacker gets NotFound error, not tenant-2's data
assert!(result.is_none());
```

**What prevented the attack**:
- ✅ JWT validation ensured attacker can only use tenant-1 context
- ✅ RequestContext carried tenant-1 (immutable)
- ✅ SQL query filtered by tenant-1 (database-level enforcement)
- ✅ No way to bypass tenant filtering (type-safe)

## Troubleshooting

### Common Issues

1. **"JWT secret must be an environment variable reference"**
   - Solution: Use `${JWT_SECRET}` in config, set env var

2. **"Missing required tenant_id in RequestContext"**
   - Solution: Ensure JWT middleware extracts tenant_id from claims

3. **"Metadata tenant_id does not match context tenant_id"**
   - Solution: Ensure metadata tenant_id matches RequestContext

4. **Certificate validation fails**
   - Solution: Check CA certificate is correct, certificate not expired

## References

- [CONFIG_STREAMLINING_PLAN_V2.md](../CONFIG_STREAMLINING_PLAN_V2.md) - Configuration plan
- [CONFIG_STREAMLINING_STATUS.md](../CONFIG_STREAMLINING_STATUS.md) - Implementation status
- [RequestContext](../crates/core/src/request_context.rs) - RequestContext implementation
- [JWT Middleware](../crates/grpc-middleware/src/auth.rs) - JWT middleware
- [mTLS Certificate Generation](../crates/grpc-middleware/src/cert_gen.rs) - Certificate generation
